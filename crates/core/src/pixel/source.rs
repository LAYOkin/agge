use super::{Glyph, Transform, glyph, line_error};
use crate::{Kind, Result, fail, valid_id};
use std::{ops::Range, str::FromStr};

pub struct Program<'a> {
    pub size: [usize; 2],
    pub style: &'a str,
    pub parent: Option<&'a str>,
    pub base: Block<'a>,
    pub stages: Vec<Stage<'a>>,
    pub references: Vec<Reference<'a>>,
}

pub struct Reference<'a> {
    pub kind: Kind,
    pub id: &'a str,
    pub line: usize,
    pub span: Range<usize>,
}

pub struct Block<'a> {
    pub line: usize,
    pub name: &'a str,
    pub from: &'a str,
    pub layers: Vec<Layer<'a>>,
    end: usize,
}

impl Block<'_> {
    fn close(&mut self, at: usize) {
        self.end = at;
        if let Some(layer) = self.layers.last_mut() {
            layer.end = at;
        }
    }
}

pub struct Layer<'a> {
    pub name: &'a str,
    pub line: usize,
    pub draws: Vec<Draw<'a>>,
    body: usize,
    end: usize,
}

pub struct Draw<'a> {
    pub line: usize,
    pub text: &'a str,
}

pub enum Stage<'a> {
    Frame(Block<'a>),
    Motion {
        line: usize,
        prefix: &'a str,
        from: &'a str,
        tracks: Vec<Track<'a>>,
    },
}

pub struct Track<'a> {
    pub layer: Option<&'a str>,
    pub offsets: Vec<[i32; 2]>,
}

pub(super) struct Words<'a>(std::str::SplitAsciiWhitespace<'a>);

impl<'a> Words<'a> {
    pub fn new(text: &'a str) -> Self {
        Self(text.split_ascii_whitespace())
    }

    pub fn next(&mut self) -> Result<&'a str> {
        self.optional().ok_or_else(|| fail("missing argument"))
    }

    pub fn optional(&mut self) -> Option<&'a str> {
        self.0.next()
    }

    pub fn number<T: FromStr>(&mut self) -> Result<T> {
        let word = self.next()?;
        word.parse()
            .map_err(|_| fail(format!("invalid number {word:?}")))
    }

    pub fn id(&mut self) -> Result<&'a str> {
        id(self.next()?)
    }

    pub fn glyph(&mut self) -> Result<Glyph> {
        let word = self.next()?;
        match word.as_bytes() {
            [b'-'] => Ok(0),
            [b] if glyph(*b) => Ok(*b),
            _ => Err(fail(format!("invalid ink {word:?}"))),
        }
    }

    pub fn point(&mut self) -> Result<[i32; 2]> {
        Ok([coord(self.next()?)?, coord(self.next()?)?])
    }

    pub fn transform(&mut self) -> Result<Transform> {
        Ok(match self.optional() {
            None => Transform::Identity,
            Some("mx") => Transform::MirrorX,
            Some("my") => Transform::MirrorY,
            Some("r90") => Transform::Rotate90,
            Some("r180") => Transform::Rotate180,
            Some("r270") => Transform::Rotate270,
            Some(other) => return Err(fail(format!("unknown transform {other:?}"))),
        })
    }

    pub fn done(&mut self) -> Result<()> {
        if let Some(word) = self.optional() {
            return Err(fail(format!("unexpected argument {word:?}")));
        }
        Ok(())
    }
}

pub fn id(word: &str) -> Result<&str> {
    if valid_id(word) {
        Ok(word)
    } else {
        Err(fail(format!("invalid identifier {word:?}")))
    }
}

pub fn coord(word: &str) -> Result<i32> {
    word.parse::<i32>()
        .ok()
        .filter(|n| (-4096..=4096).contains(n))
        .ok_or_else(|| {
            fail(format!(
                "coordinate {word:?} must be between -4096 and 4096"
            ))
        })
}

pub(super) fn pair(word: &str) -> Result<[i32; 2]> {
    let (x, y) = word.split_once(',').ok_or_else(|| fail("expected x,y"))?;
    Ok([coord(x)?, coord(y)?])
}

fn tween<'a>(words: &mut Words<'a>) -> Result<Vec<Track<'a>>> {
    let count = words.number::<usize>()?;
    if !(2..=super::MAX_FRAMES).contains(&count) {
        return Err(fail("tween needs 2–64 frames"));
    }
    let delta = words.point()?;
    let layer = words.optional().map(id).transpose()?;
    let offsets = (0..count)
        .map(|i| {
            delta.map(|n| {
                // Round halves away from zero, independent of floating point.
                let divisor = (count - 1) as i32;
                let value = n * i as i32;
                (value + value.signum() * (divisor / 2)) / divisor
            })
        })
        .collect();
    Ok(vec![Track { layer, offsets }])
}

fn motion<'a>(words: &mut Words<'a>) -> Result<Vec<Track<'a>>> {
    let mut tracks: Vec<Track<'a>> = Vec::new();
    while let Some(word) = words.optional() {
        let (layer, sequence) = word
            .split_once('=')
            .ok_or_else(|| fail("motion tracks use layer=x,y/x,y/..."))?;
        let layer = Some(id(layer)?);
        if tracks.len() == super::MAX_LAYERS || tracks.iter().any(|track| track.layer == layer) {
            return Err(fail("duplicate motion track or layer limit exceeded"));
        }
        let offsets = sequence
            .split('/')
            .take(super::MAX_FRAMES + 1)
            .map(pair)
            .collect::<Result<Vec<_>>>()?;
        if !(2..=super::MAX_FRAMES).contains(&offsets.len())
            || tracks
                .first()
                .is_some_and(|first| first.offsets.len() != offsets.len())
        {
            return Err(fail("motion tracks must have the same 2–64 offsets"));
        }
        tracks.push(Track { layer, offsets });
    }
    if tracks.is_empty() {
        return Err(fail("motion needs at least one track"));
    }
    Ok(tracks)
}

pub fn art_key(word: &str) -> Result<(&str, Option<&str>)> {
    if let Some((art, frame)) = word.split_once(':') {
        Ok((id(art)?, Some(id(frame)?)))
    } else {
        Ok((id(word)?, None))
    }
}

fn content(line: &str) -> &str {
    line.split_once('#').map_or(line, |(text, _)| text).trim()
}

fn pose<'p, 's>(base: &'p mut Block<'s>, stages: &'p mut [Stage<'s>]) -> Result<&'p mut Block<'s>> {
    match stages.last_mut() {
        Some(Stage::Frame(block)) => Ok(block),
        Some(Stage::Motion { .. }) => {
            Err(fail("start a frame before drawing after generated poses"))
        }
        None => Ok(base),
    }
}

pub fn parse<'source>(source: &'source str) -> Result<Program<'source>> {
    if source.len() > 128 * 1024 {
        return Err(fail("pixel source exceeds 128 KiB"));
    }
    let mut size = None;
    let mut style = None;
    let mut parent = None;
    let mut base = Block {
        line: 1,
        name: "base",
        from: "base",
        layers: Vec::new(),
        end: source.len(),
    };
    let mut stages = Vec::new();
    let mut references = Vec::new();
    let mut offset = 0;
    for (index, raw) in source.split_inclusive('\n').enumerate() {
        let line = index + 1;
        let start = offset;
        offset += raw.len();
        let text = content(raw);
        if text.is_empty() {
            continue;
        }
        let mut words = Words::new(text);
        let command = words.next()?;
        let result = (|| {
            let mut link = |kind, name: &'source str| {
                let tail = &text[command.len()..];
                let at = start
                    + (raw.len() - raw.trim_start().len())
                    + command.len()
                    + (tail.len() - tail.trim_start().len());
                references.push(Reference {
                    kind,
                    id: name,
                    line,
                    span: at..at + name.len(),
                });
            };
            match command {
                "canvas" | "style" | "base" => {
                    if !stages.is_empty() || !base.layers.is_empty() {
                        return Err(fail(
                            "canvas, style and base belong before layers and frames",
                        ));
                    }
                    match command {
                        "canvas" if size.is_none() => {
                            let dims = [words.number()?, words.number()?];
                            if dims.iter().any(|n| !(1..=super::MAX_EDGE).contains(n)) {
                                return Err(fail("canvas must be 1–256 pixels per edge"));
                            }
                            size = Some(dims);
                        }
                        "style" if style.is_none() => {
                            let name = words.id()?;
                            link(Kind::Palette, name);
                            style = Some(name);
                        }
                        "base" if parent.is_none() => {
                            let name = words.id()?;
                            link(Kind::Pixel, name);
                            parent = Some(name);
                        }
                        _ => return Err(fail(format!("duplicate {command} directive"))),
                    }
                }
                "frame" | "tween" | "motion" => {
                    if let Ok(current) = pose(&mut base, &mut stages) {
                        current.close(start);
                    }
                    if stages.len() == super::MAX_FRAMES {
                        return Err(fail("too many frame directives"));
                    }
                    let name = words.id()?;
                    if name == "base" {
                        return Err(fail("base is reserved for the template pose"));
                    }
                    if command == "frame" {
                        let from = id(words.optional().unwrap_or("base"))?;
                        stages.push(Stage::Frame(Block {
                            line,
                            name,
                            from,
                            layers: Vec::new(),
                            end: source.len(),
                        }));
                    } else {
                        let from = words.id()?;
                        let tracks = if command == "tween" {
                            tween(&mut words)?
                        } else {
                            motion(&mut words)?
                        };
                        stages.push(Stage::Motion {
                            line,
                            prefix: name,
                            from,
                            tracks,
                        });
                    }
                }
                "layer" => {
                    let block = pose(&mut base, &mut stages)?;
                    let name = words.id()?;
                    if block.layers.len() == super::MAX_LAYERS
                        || block.layers.iter().any(|layer| layer.name == name)
                    {
                        return Err(fail(format!(
                            "duplicate layer {name} or layer limit exceeded"
                        )));
                    }
                    if let Some(previous) = block.layers.last_mut() {
                        previous.end = start;
                    }
                    block.layers.push(Layer {
                        name,
                        line,
                        draws: Vec::new(),
                        body: offset,
                        end: source.len(),
                    });
                }
                _ => {
                    let layer = pose(&mut base, &mut stages)?
                        .layers
                        .last_mut()
                        .ok_or_else(|| fail("start a layer before drawing"))?;
                    if command == "use" {
                        let (part, _) = art_key(words.next()?)?;
                        link(Kind::Pixel, part);
                    }
                    layer.draws.push(Draw { line, text });
                    return Ok(());
                }
            }
            words.done()
        })();
        result.map_err(|error| line_error(line, error))?;
    }
    Ok(Program {
        size: size.ok_or_else(|| fail("missing canvas width and height"))?,
        style: style.ok_or_else(|| fail("missing style palette"))?,
        parent,
        base,
        stages,
        references,
    })
}

/// Replace a layer's local commands or append commands to its authored pose.
/// Inherited layers can receive local commands without copying the parent source.
pub fn patch(
    source: &str,
    frame: Option<&str>,
    layer: &str,
    draws: &str,
    replace: bool,
) -> Result<String> {
    id(layer)?;
    let program = parse(source)?;
    let block = match frame {
        None | Some("base") => &program.base,
        Some(name) => program
            .stages
            .iter()
            .find_map(|stage| match stage {
                Stage::Frame(block) if block.name == name => Some(block),
                _ => None,
            })
            .ok_or_else(|| {
                fail(format!(
                    "no authored frame {name:?}; edit the generator or add a frame"
                ))
            })?,
    };
    let draws = draws.replace("\r\n", "\n");
    if draws.lines().any(|line| {
        matches!(
            content(line).split_ascii_whitespace().next(),
            Some("canvas" | "style" | "base" | "frame" | "tween" | "motion" | "layer" | "---")
        )
    }) {
        return Err(fail("pixel patch accepts drawing commands only"));
    }
    let existing = block.layers.iter().find(|block| block.name == layer);
    let (range, mut insert) = match existing {
        Some(block) if replace => (block.body..block.end, String::new()),
        Some(block) => (block.end..block.end, String::new()),
        None => (block.end..block.end, format!("layer {layer}\n")),
    };
    if range.start > 0 && !source[..range.start].ends_with('\n') {
        insert.insert(0, '\n');
    }
    for line in draws.lines() {
        let line = line.trim();
        if !line.is_empty() {
            insert.push_str("  ");
            insert.push_str(line);
            insert.push('\n');
        }
    }
    let mut next = source.to_owned();
    next.replace_range(range, &insert);
    next.truncate(next.trim_end_matches('\n').len());
    Ok(next)
}
