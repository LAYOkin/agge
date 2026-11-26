use crate::{Edit, Index, Project, Result, fail, model::fingerprint_asset, valid_id};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
};

const PROJECT_LIMIT: usize = 2 * 1024 * 1024;
const HISTORY_LIMIT: usize = 12;

#[derive(Clone, Debug)]
pub struct Loaded {
    pub project: Project,
    pub revision: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct History {
    revision: String,
    undo: Vec<String>,
    redo: Vec<String>,
}
impl History {
    fn empty(revision: String) -> Self {
        Self {
            revision,
            undo: vec![],
            redo: vec![],
        }
    }
}
#[derive(Serialize, Deserialize)]
struct Journal {
    before: History,
    after: History,
}

#[derive(Default, Serialize, Deserialize)]
struct Metadata {
    facts: BTreeMap<String, Value>,
    checkpoints: BTreeMap<String, BTreeMap<String, String>>,
}

#[derive(Debug, Serialize)]
pub struct Delta {
    pub revision: String,
    pub changed: Vec<String>,
    pub removed: Vec<String>,
}
#[derive(Debug, Serialize)]
pub struct Receipt {
    pub revision: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub changed: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub removed: Vec<String>,
}

pub struct Store {
    file: PathBuf,
}

impl Store {
    pub fn new(file: impl Into<PathBuf>) -> Self {
        Self { file: file.into() }
    }
    pub fn path(&self) -> &Path {
        &self.file
    }
    pub fn root(&self) -> &Path {
        self.file
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."))
    }
    fn sibling(&self, suffix: &str) -> PathBuf {
        let mut path = self.file.as_os_str().to_owned();
        path.push(suffix);
        PathBuf::from(path)
    }
    fn lock(&self) -> Result<File> {
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(self.sibling(".lock"))?;
        file.lock()?;
        Ok(file)
    }
    fn source(&self) -> Result<String> {
        let mut source = String::new();
        File::open(&self.file)?
            .take(PROJECT_LIMIT as u64 + 1)
            .read_to_string(&mut source)?;
        if source.len() > PROJECT_LIMIT {
            return Err(fail("project exceeds 2 MiB"));
        }
        Ok(source)
    }
    pub fn init(&self, project: &Project) -> Result<()> {
        Index::build(project).require_valid()?;
        fs::create_dir_all(self.root())?;
        let _lock = self.lock()?;
        let source = project.encode();
        if source.len() > PROJECT_LIMIT {
            return Err(fail("project exceeds 2 MiB"));
        }
        write_atomic(&self.file, source.as_bytes(), false)
    }
    pub fn load(&self) -> Result<Loaded> {
        let source = self.source()?;
        Ok(Loaded {
            revision: revision(&source),
            project: Project::parse(&source)?,
        })
    }
    pub fn transact(&self, base: Option<&str>, ops: Vec<Edit>) -> Result<Receipt> {
        self.transact_checked(base, ops, false, |_| Ok(true))
            .map(|(receipt, _)| receipt)
    }
    pub fn transact_checked(
        &self,
        base: Option<&str>,
        ops: Vec<Edit>,
        preview: bool,
        verify: impl FnOnce(&Project) -> Result<bool>,
    ) -> Result<(Receipt, bool)> {
        self.update_checked(base, preview, verify, move |project| {
            for (i, op) in ops.into_iter().enumerate() {
                op.apply(project)
                    .map_err(|e| fail(format!("op {}: {e}", i + 1)))?;
            }
            Ok(())
        })
    }
    pub fn update(
        &self,
        base: Option<&str>,
        edit: impl FnOnce(&mut Project) -> Result<()>,
    ) -> Result<Receipt> {
        self.update_checked(base, false, |_| Ok(true), edit)
            .map(|(receipt, _)| receipt)
    }
    fn update_checked(
        &self,
        base: Option<&str>,
        preview: bool,
        verify: impl FnOnce(&Project) -> Result<bool>,
        edit: impl FnOnce(&mut Project) -> Result<()>,
    ) -> Result<(Receipt, bool)> {
        let _lock = self.lock()?;
        let source = self.source()?;
        let revision = check_base(&source, base)?;
        let mut project = Project::parse(&source)?;
        let old = Index::manifest(&project);
        edit(&mut project)?;
        let next = project.encode();
        if next.len() > PROJECT_LIMIT {
            return Err(fail("project exceeds 2 MiB"));
        }
        let new = Index::build(&project);
        new.require_valid()?;
        let mut receipt = receipt(&old, &new, &next);
        let passed = verify(&project)?;
        if preview || !passed || old == new.symbols {
            receipt.revision = revision;
            return Ok((receipt, passed));
        }
        check_base(&self.source()?, Some(&revision))?;
        let before = self.history(revision)?;
        let mut after = before.clone();
        after.revision = receipt.revision.clone();
        push(&mut after.undo, source);
        after.redo.clear();
        self.commit(before, after, &next)?;
        Ok((receipt, passed))
    }
    pub fn travel(&self, base: Option<&str>, redo: bool) -> Result<Receipt> {
        let _lock = self.lock()?;
        let source = self.source()?;
        let revision = check_base(&source, base)?;
        let before = self.history(revision)?;
        let mut after = before.clone();
        let next = if redo {
            after.redo.pop()
        } else {
            after.undo.pop()
        }
        .ok_or_else(|| {
            fail(if redo {
                "nothing to redo"
            } else {
                "nothing to undo"
            })
        })?;
        let old = Index::manifest(&Project::parse(&source)?);
        let new = Index::build(&Project::parse(&next)?);
        new.require_valid()?;
        let receipt = receipt(&old, &new, &next);
        after.revision = receipt.revision.clone();
        push(
            if redo {
                &mut after.undo
            } else {
                &mut after.redo
            },
            source,
        );
        self.commit(before, after, &next)?;
        Ok(receipt)
    }
    fn history(&self, revision: String) -> Result<History> {
        let journal: Option<Journal> = read_json(&self.sibling(".history"))?;
        if let Some(journal) = journal {
            if journal.after.revision == revision {
                return Ok(journal.after);
            }
            if journal.before.revision == revision {
                return Ok(journal.before);
            }
        }
        Ok(History::empty(revision))
    }
    fn commit(&self, before: History, after: History, source: &str) -> Result<()> {
        if source.len() > PROJECT_LIMIT {
            return Err(fail("project exceeds 2 MiB"));
        }
        write_json(&self.sibling(".history"), &Journal { before, after })?;
        write_atomic(&self.file, source.as_bytes(), true)
    }
    pub fn delta(&self, session: &str, advance: bool) -> Result<Delta> {
        require_key(session)?;
        let _lock = advance.then(|| self.lock()).transpose()?;
        let loaded = self.load()?;
        let mut symbols = Index::manifest(&loaded.project);
        for file in loaded.project.asset_paths() {
            let fingerprint =
                fingerprint_asset(self.root(), file).unwrap_or_else(|_| "unavailable".into());
            symbols.insert(format!("asset/{file}"), fingerprint);
        }
        let path = self.sibling(".knowledge");
        let mut metadata: Metadata = read_json(&path)?.unwrap_or_default();
        let empty = BTreeMap::new();
        let old = metadata.checkpoints.get(session).unwrap_or(&empty);
        let (changed, removed) = difference(old, &symbols);
        if advance {
            if metadata.checkpoints.len() >= 64 && !metadata.checkpoints.contains_key(session) {
                return Err(fail("checkpoint limit reached"));
            }
            metadata.checkpoints.insert(session.into(), symbols);
            write_json(&path, &metadata)?;
        }
        Ok(Delta {
            revision: loaded.revision,
            changed,
            removed,
        })
    }
    pub fn facts(&self, key: Option<&str>) -> Result<BTreeMap<String, Value>> {
        let metadata: Metadata = read_json(&self.sibling(".knowledge"))?.unwrap_or_default();
        Ok(match key {
            Some(key) => metadata
                .facts
                .get(key)
                .map(|value| BTreeMap::from([(key.into(), value.clone())]))
                .unwrap_or_default(),
            None => metadata.facts,
        })
    }
    pub fn remember(&self, key: &str, value: Option<Value>) -> Result<()> {
        require_key(key)?;
        let _lock = self.lock()?;
        self.source()?;
        let path = self.sibling(".knowledge");
        let mut metadata: Metadata = read_json(&path)?.unwrap_or_default();
        if let Some(value) = value {
            if value.to_string().len() > 8192
                || metadata.facts.len() >= 256 && !metadata.facts.contains_key(key)
            {
                return Err(fail("knowledge limit reached"));
            }
            metadata.facts.insert(key.into(), value);
        } else {
            metadata.facts.remove(key);
        }
        write_json(&path, &metadata)
    }
}
fn require_key(key: &str) -> Result<()> {
    if valid_id(key) {
        Ok(())
    } else {
        Err(fail("expected a lowercase session or knowledge identifier"))
    }
}
fn revision(source: &str) -> String {
    URL_SAFE_NO_PAD.encode(&blake3::hash(source.as_bytes()).as_bytes()[..16])
}
fn check_base(source: &str, base: Option<&str>) -> Result<String> {
    let revision = revision(source);
    if base.is_some_and(|base| base != revision) {
        return Err(fail(format!("stale revision, current {revision}")));
    }
    Ok(revision)
}
fn push(stack: &mut Vec<String>, value: String) {
    if stack.len() == HISTORY_LIMIT {
        stack.remove(0);
    }
    stack.push(value);
}
fn difference(
    old: &BTreeMap<String, String>,
    new: &BTreeMap<String, String>,
) -> (Vec<String>, Vec<String>) {
    (
        new.iter()
            .filter(|(key, hash)| old.get(*key) != Some(*hash))
            .map(|(key, _)| key.clone())
            .collect(),
        old.keys()
            .filter(|key| !new.contains_key(*key))
            .cloned()
            .collect(),
    )
}
fn receipt(old: &BTreeMap<String, String>, new: &Index, source: &str) -> Receipt {
    let (changed, removed) = difference(old, &new.symbols);
    Receipt {
        revision: revision(source),
        changed,
        removed,
    }
}
fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Result<Option<T>> {
    match File::open(path) {
        Ok(file) => Ok(Some(serde_json::from_reader(std::io::BufReader::new(
            file,
        ))?)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}
fn write_json(path: &Path, value: &impl Serialize) -> Result<()> {
    write_atomic(path, &serde_json::to_vec(value)?, true)
}
fn write_atomic(path: &Path, bytes: &[u8], replace: bool) -> Result<()> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let mut temp = tempfile::NamedTempFile::new_in(parent)?;
    temp.write_all(bytes)?;
    temp.as_file().sync_all()?;
    if replace {
        temp.persist(path)?;
    } else {
        temp.persist_noclobber(path)?;
    }
    #[cfg(unix)]
    File::open(parent)?.sync_all()?;
    Ok(())
}
