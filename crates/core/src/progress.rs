use crate::{Result, fail, store::write_atomic, valid_id};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File, OpenOptions},
    io::{ErrorKind, Read},
    path::{Path, PathBuf},
};

const MAX_FIELDS: usize = 256;
const MAX_MIGRATIONS: usize = 64;
const MAX_FILE_BYTES: usize = 128 * 1024;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProgressSchema {
    pub id: String,
    pub version: u32,
    pub globals: BTreeMap<String, f32>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub migrations: BTreeMap<u32, ProgressMigration>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ProgressMigration {
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub rename: BTreeMap<String, String>,
    #[serde(skip_serializing_if = "BTreeSet::is_empty")]
    pub remove: BTreeSet<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Progress {
    pub id: String,
    pub version: u32,
    pub values: BTreeMap<String, f32>,
}

impl ProgressSchema {
    pub fn validate(&self) -> Result<()> {
        if !valid_id(&self.id)
            || !(1..=65_535).contains(&self.version)
            || self.globals.is_empty()
            || !valid_values(&self.globals)
            || self.migrations.len() > MAX_MIGRATIONS
        {
            return Err(fail(
                "progress needs a valid ID, version 1–65,535, and 1–256 numeric globals",
            ));
        }
        for (&version, migration) in &self.migrations {
            let mut targets = BTreeSet::new();
            let invalid = version == 0
                || version >= self.version
                || migration.rename.len() + migration.remove.len() > MAX_FIELDS
                || migration.remove.iter().any(|name| !valid_id(name))
                || migration.rename.iter().any(|(from, to)| {
                    !valid_id(from)
                        || !valid_id(to)
                        || from == to
                        || migration.remove.contains(from)
                        || !targets.insert(to)
                });
            if invalid {
                return Err(fail(format!(
                    "invalid progress migration from version {version}"
                )));
            }
        }
        Ok(())
    }

    /// Load a compatible save and merge new defaults after its migrations.
    pub fn initial_values(&self, progress: Option<&Progress>) -> Result<BTreeMap<String, f32>> {
        let mut defaults = self.globals.clone();
        let Some(progress) = progress else {
            return Ok(defaults);
        };
        if progress.id != self.id {
            return Err(fail("progress belongs to a different game"));
        }
        if progress.version == 0
            || progress.version > self.version
            || (self.version - progress.version) as usize > MAX_MIGRATIONS
            || !valid_values(&progress.values)
        {
            return Err(fail("unsupported progress version or invalid saved values"));
        }
        let mut values = progress.values.clone();
        for version in progress.version..self.version {
            let change = self.migrations.get(&version).ok_or_else(|| {
                fail(format!("missing progress migration from version {version}"))
            })?;
            for name in &change.remove {
                values.remove(name);
            }
            // Remove every source first so swaps and rename chains are simultaneous.
            let renamed: Vec<_> = change
                .rename
                .iter()
                .filter_map(|(from, to)| Some((to, values.remove(from)?)))
                .collect();
            for (name, value) in renamed {
                if values.insert(name.clone(), value).is_some() {
                    return Err(fail(format!(
                        "progress migration {version} would overwrite {name}"
                    )));
                }
            }
        }
        for (name, value) in values {
            let dest = defaults
                .get_mut(&name)
                .ok_or_else(|| fail(format!("progress contains undeclared global {name}")))?;
            *dest = value;
        }
        Ok(defaults)
    }

    pub fn capture(&self, globals: &BTreeMap<String, f32>) -> Progress {
        Progress {
            id: self.id.clone(),
            version: self.version,
            values: self
                .globals
                .iter()
                .map(|(name, default)| {
                    (name.clone(), globals.get(name).copied().unwrap_or(*default))
                })
                .collect(),
        }
    }
}

fn valid_values(values: &BTreeMap<String, f32>) -> bool {
    values.len() <= MAX_FIELDS
        && values
            .iter()
            .all(|(key, value)| valid_id(key) && value.is_finite())
}

fn read_bytes(path: &Path) -> Result<Option<Vec<u8>>> {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    let mut bytes = Vec::new();
    file.take(MAX_FILE_BYTES as u64 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > MAX_FILE_BYTES {
        return Err(fail("progress file exceeds 128 KiB"));
    }
    Ok(Some(bytes))
}

/// A versioned local save slot with optimistic conflict detection.
pub struct ProgressSlot {
    path: PathBuf,
    schema: ProgressSchema,
    observed: Option<Vec<u8>>,
}

impl ProgressSlot {
    pub fn open(path: PathBuf, schema: &ProgressSchema) -> Result<(Self, Option<Progress>)> {
        schema.validate()?;
        let observed = read_bytes(&path)?;
        let progress = observed
            .as_deref()
            .map(serde_json::from_slice)
            .transpose()?;
        schema.initial_values(progress.as_ref())?;
        Ok((
            Self {
                path,
                schema: schema.clone(),
                observed,
            },
            progress,
        ))
    }

    pub fn write(&mut self, progress: &Progress) -> Result<()> {
        if progress.version != self.schema.version {
            return Err(fail("only current-version progress can be written"));
        }
        self.schema.initial_values(Some(progress))?;
        if let Some(parent) = self.path.parent().filter(|p| !p.as_os_str().is_empty()) {
            fs::create_dir_all(parent)?;
        }
        let mut lock_path = self.path.as_os_str().to_owned();
        lock_path.push(".lock");
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(lock_path)?;
        lock.try_lock()?;
        if read_bytes(&self.path)? != self.observed {
            return Err(fail(
                "progress changed in another session; restart to load it",
            ));
        }
        let bytes = serde_json::to_vec(progress)?;
        if self.observed.as_deref() != Some(&bytes) {
            write_atomic(&self.path, &bytes, self.observed.is_some())?;
            self.observed = Some(bytes);
        }
        Ok(())
    }
}
