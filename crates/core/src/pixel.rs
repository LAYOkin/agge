//! Native indexed artwork. Layers and poses share immutable pixel buffers.
mod raster;
pub mod source;

use crate::{Color, Kind, Project, Resource, Result, fail};
use serde::{Deserialize, Serialize};
use source::{Block, Stage};
use std::{collections::BTreeMap, sync::Arc};

pub const MAX_EDGE: usize = 256;
pub const MAX_FRAMES: usize = 64;
pub const MAX_GENERATED_PIXELS: usize = 16 * 1024 * 1024;
const MAX_LAYERS: usize = 16;
const WORK_LIMIT: usize = 48 * 1024 * 1024;
type Glyph = u8;

pub fn sheet_columns(pose_count: usize) -> usize {
    pose_count.clamp(1, 8)
}

fn glyph(value: u8) -> bool {
    value.is_ascii_alphabetic() || b"@+$%&?=!*^~".contains(&value)
}

fn line_error(
    line: usize,
    error: impl std::fmt::Display,
) -> Box<dyn std::error::Error + Send + Sync> {
    fail(format!("{line}: {error}"))
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    pub swatches: BTreeMap<char, String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ramps: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ink: Option<char>,
    #[serde(default = "default_light", skip_serializing_if = "is_default_light")]
    pub light: [i32; 2],
}

fn default_light() -> [i32; 2] {
    [-1, -1]
}

fn is_default_light(light: &[i32; 2]) -> bool {
    *light == default_light()
}

#[derive(Debug)]
pub struct Palette {
    colors: [Color; 128],
    shades: [[Glyph; 2]; 128],
    pub ink: Option<u8>,
    light: [i32; 2],
}

impl Profile {
    pub fn compile(&self) -> Result<Palette> {
        if self.swatches.is_empty()
            || self.light == [0, 0]
            || self.light.iter().any(|n| !(-1..=1).contains(n))
        {
            return Err(fail(
                "define swatches and use a nonzero light direction from -1 to 1",
            ));
        }
        let mut colors = [[0; 4]; 128];
        for (&key, color) in &self.swatches {
            let id = u8::try_from(key)
                .ok()
                .filter(|key| glyph(*key))
                .ok_or_else(|| fail(format!("invalid swatch {key:?}")))?;
            let digits = color.strip_prefix('#').unwrap_or(color);
            if !matches!(digits.len(), 6 | 8) || !digits.bytes().all(|b| b.is_ascii_hexdigit()) {
                return Err(fail(format!("swatch {key} requires RRGGBB or RRGGBBAA")));
            }
            let mut rgba = [255; 4];
            for (i, value) in rgba.iter_mut().take(digits.len() / 2).enumerate() {
                *value = u8::from_str_radix(&digits[i * 2..i * 2 + 2], 16).expect("hex validated");
            }
            if rgba[3] == 0 {
                return Err(fail(format!(
                    "swatch {key} has zero alpha; use an eraser instead"
                )));
            }
            colors[usize::from(id)] = rgba;
        }
        let mut shades = std::array::from_fn(|i| [i as u8; 2]);
        let mut assigned = [false; 128];
        let defined = |b: u8| {
            colors
                .get(usize::from(b))
                .is_some_and(|color| color[3] != 0)
        };
        for ramp in &self.ramps {
            let keys = ramp.as_bytes();
            if !(2..=8).contains(&keys.len()) {
                return Err(fail("each ramp needs 2–8 swatches ordered dark to light"));
            }
            for (i, &key) in keys.iter().enumerate() {
                if !defined(key) || std::mem::replace(&mut assigned[key as usize], true) {
                    return Err(fail(format!("unknown or repeated swatch in ramp {ramp:?}")));
                }
                shades[key as usize] =
                    [keys[i.saturating_sub(1)], keys[(i + 1).min(keys.len() - 1)]];
            }
        }
        let ink = self
            .ink
            .map(|key| {
                u8::try_from(key)
                    .ok()
                    .filter(|&key| defined(key))
                    .ok_or_else(|| fail("outline ink must be a defined swatch"))
            })
            .transpose()?;
        Ok(Palette {
            colors,
            shades,
            ink,
            light: self.light,
        })
    }
}

impl Palette {
    pub fn color(&self, key: u8) -> Color {
        self.colors.get(key as usize).copied().unwrap_or([0; 4])
    }

    pub fn swatches(&self) -> impl Iterator<Item = (u8, Color)> + '_ {
        self.colors
            .iter()
            .enumerate()
            .filter(|(_, color)| color[3] != 0)
            .map(|(key, &color)| (key as u8, color))
    }

    fn require(&self, key: Glyph) -> Result<()> {
        if key != 0 && self.color(key)[3] == 0 {
            return Err(fail(format!("style has no swatch {}", char::from(key))));
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug)]
enum Transform {
    Identity,
    MirrorX,
    MirrorY,
    Rotate90,
    Rotate180,
    Rotate270,
}

impl Transform {
    fn apply(self, [x, y]: [usize; 2], [w, h]: [usize; 2]) -> [usize; 2] {
        match self {
            Self::Identity => [x, y],
            Self::MirrorX => [w - 1 - x, y],
            Self::MirrorY => [x, h - 1 - y],
            Self::Rotate90 => [h - 1 - y, x],
            Self::Rotate180 => [w - 1 - x, h - 1 - y],
            Self::Rotate270 => [y, w - 1 - x],
        }
    }
}

#[derive(Clone, Debug, Default)]
struct Pose {
    layers: Vec<(String, Arc<Vec<Glyph>>)>,
}

#[derive(Debug)]
pub struct Art {
    pub size: [usize; 2],
    pub style: String,
    pub palette: Arc<Palette>,
    base: Pose,
    frames: Vec<(String, Pose)>,
}

impl Art {
    pub fn has_frame(&self, frame: &str) -> bool {
        frame == "base" || self.frames.iter().any(|(name, _)| name == frame)
    }

    pub fn frame_names(&self) -> impl ExactSizeIterator<Item = &str> {
        // Expose authored order, separate from the reusable template pose.
        (0..self.frames.len().max(1)).map(|i| self.frames.get(i).map_or("base", |(name, _)| name))
    }

    fn pose(&self, frame: Option<&str>) -> Result<&Pose> {
        match frame {
            Some("base") => Ok(&self.base),
            None => Ok(self.frames.first().map_or(&self.base, |(_, pose)| pose)),
            Some(frame) => self
                .frames
                .iter()
                .find(|(name, _)| name == frame)
                .map(|(_, pose)| pose)
                .ok_or_else(|| fail(format!("absent pixel frame {frame:?}"))),
        }
    }

    pub fn layers(&self, frame: Option<&str>) -> Result<impl ExactSizeIterator<Item = &str>> {
        Ok(self
            .pose(frame)?
            .layers
            .iter()
            .map(|(name, _)| name.as_str()))
    }

    pub fn area(&self, area: Option<[usize; 4]>) -> Result<[usize; 4]> {
        let [w, h] = self.size;
        let area = area.unwrap_or([0, 0, w, h]);
        let [x, y, width, height] = area;
        if width == 0 || height == 0 || x.saturating_add(width) > w || y.saturating_add(height) > h
        {
            return Err(fail("pixel area is empty or outside the canvas"));
        }
        Ok(area)
    }

    pub fn pixels(
        &self,
        frame: Option<&str>,
        layer: Option<&str>,
        area: Option<[usize; 4]>,
    ) -> Result<Vec<u8>> {
        let [x, y, w, h] = self.area(area)?;
        let pose = self.pose(frame)?;
        let width = self.size[0];
        let mut result = vec![0; w * h];
        if let Some(layer) = layer
            && !pose.layers.iter().any(|(name, _)| name == layer)
        {
            return Err(fail(format!("absent pixel layer {layer:?}")));
        }
        for (name, pixels) in &pose.layers {
            if layer.is_some_and(|selected| selected != name) {
                continue;
            }
            for (row, out) in result.chunks_exact_mut(w).enumerate() {
                let start = (row + y) * width + x;
                for (dest, &pixel) in out.iter_mut().zip(&pixels[start..start + w]) {
                    if pixel != 0 {
                        *dest = pixel;
                    }
                }
            }
        }
        Ok(result)
    }
}

/// Compiles a batch of artworks against one project, sharing reusable components.
pub struct Renderer<'a> {
    project: &'a Project,
    arts: BTreeMap<String, Arc<Art>>,
    profiles: BTreeMap<String, Arc<Palette>>,
    active: Vec<String>,
    spent: usize,
}

impl<'a> Renderer<'a> {
    pub fn new(project: &'a Project) -> Self {
        Self {
            project,
            arts: BTreeMap::new(),
            profiles: BTreeMap::new(),
            active: Vec::new(),
            spent: 0,
        }
    }

    pub fn profile(&mut self, id: &str) -> Result<Arc<Palette>> {
        if let Some(palette) = self.profiles.get(id) {
            return Ok(palette.clone());
        }
        let Some(Resource::Palette(profile)) = self.project.get(Kind::Palette, id) else {
            return Err(fail(format!("absent palette/{id}")));
        };
        let palette = Arc::new(profile.compile()?);
        self.profiles.insert(id.into(), palette.clone());
        Ok(palette)
    }

    pub fn render(&mut self, id: &str) -> Result<Arc<Art>> {
        if let Some(art) = self.arts.get(id) {
            return Ok(art.clone());
        }
        if self.active.iter().any(|other| other == id) {
            return Err(fail(format!("pixel dependency cycle at pixel/{id}")));
        }
        if self.active.len() == 16 {
            return Err(fail("pixel dependency depth exceeds 16"));
        }
        let Some(Resource::Pixel(source)) = self.project.get(Kind::Pixel, id) else {
            return Err(fail(format!("absent pixel/{id}")));
        };
        let program = source::parse(source)?;
        self.active.push(id.into());
        let result = self.compile(program);
        self.active.pop();
        let art = Arc::new(result?);
        self.arts.insert(id.into(), art.clone());
        Ok(art)
    }

    fn spend(&mut self, cost: usize) -> Result<()> {
        self.spent = self.spent.saturating_add(cost.max(1));
        if self.spent > WORK_LIMIT {
            return Err(fail("project pixel rendering budget exceeded"));
        }
        Ok(())
    }

    fn compile(&mut self, program: source::Program<'_>) -> Result<Art> {
        let [w, h] = program.size;
        let palette = self.profile(program.style)?;
        let mut base = if let Some(parent) = program.parent {
            let template = self
                .render(parent)
                .map_err(|e| fail(format!("base {parent}: {e}")))?;
            if template.size != program.size {
                return Err(fail("base artwork must have the same canvas size"));
            }
            for (_, layer) in &template.base.layers {
                self.spend(layer.len())?;
                for &key in layer.iter() {
                    palette.require(key)?;
                }
            }
            template.base.clone()
        } else {
            Pose::default()
        };
        self.paint(&mut base, &program.base, program.size, &palette)?;
        let mut art = Art {
            size: program.size,
            style: program.style.into(),
            palette,
            base,
            frames: Vec::new(),
        };
        for stage in program.stages {
            match stage {
                Stage::Frame(block) => {
                    let mut pose = art
                        .pose(Some(block.from))
                        .map_err(|e| line_error(block.line, e))?
                        .clone();
                    self.paint(&mut pose, &block, art.size, &art.palette)?;
                    self.push(&mut art, block.name.into(), pose)
                        .map_err(|e| line_error(block.line, e))?;
                }
                Stage::Motion {
                    line,
                    prefix,
                    from,
                    tracks,
                } => {
                    let result = (|| {
                        let original = art.pose(Some(from))?.clone();
                        for track in &tracks {
                            if let Some(layer) = track.layer
                                && !original.layers.iter().any(|(name, _)| name == layer)
                            {
                                return Err(fail(format!("absent motion layer {layer:?}")));
                            }
                        }
                        for i in 0..tracks[0].offsets.len() {
                            let mut pose = original.clone();
                            for (name, pixels) in &mut pose.layers {
                                let offset = tracks
                                    .iter()
                                    .find(|track| track.layer.is_none_or(|target| target == name))
                                    .map_or([0, 0], |track| track.offsets[i]);
                                if offset != [0, 0] {
                                    self.spend(w * h)?;
                                    let mut shifted = vec![0; w * h];
                                    raster::blit(
                                        &mut shifted,
                                        art.size,
                                        pixels,
                                        art.size,
                                        offset,
                                        Transform::Identity,
                                        false,
                                    );
                                    *pixels = Arc::new(shifted);
                                }
                            }
                            let name = format!("{prefix}_{i}");
                            source::id(&name)?;
                            self.push(&mut art, name, pose)?;
                        }
                        Ok(())
                    })();
                    result.map_err(|e| line_error(line, e))?;
                }
            }
        }
        if art.frames.is_empty() {
            self.spend(w * h)?;
        }
        Ok(art)
    }

    fn push(&mut self, art: &mut Art, name: String, pose: Pose) -> Result<()> {
        if art.frames.len() == MAX_FRAMES || art.frames.iter().any(|(n, _)| n == &name) {
            return Err(fail(format!(
                "duplicate frame {name} or frame limit exceeded"
            )));
        }
        self.spend(art.size[0] * art.size[1])?;
        art.frames.push((name, pose));
        Ok(())
    }

    fn paint(
        &mut self,
        pose: &mut Pose,
        block: &Block<'_>,
        [w, h]: [usize; 2],
        palette: &Palette,
    ) -> Result<()> {
        for layer in &block.layers {
            self.spend(w * h)?;
            let index =
                if let Some(index) = pose.layers.iter().position(|(name, _)| name == layer.name) {
                    index
                } else {
                    if pose.layers.len() == MAX_LAYERS {
                        return Err(line_error(layer.line, "layer limit exceeded"));
                    }
                    pose.layers
                        .push((layer.name.into(), Arc::new(vec![0; w * h])));
                    pose.layers.len() - 1
                };
            let pixels = Arc::make_mut(&mut pose.layers[index].1);
            let mut raster = raster::Raster {
                pixels,
                size: [w, h],
                palette,
            };
            for draw in &layer.draws {
                raster
                    .draw(self, draw.text)
                    .map_err(|e| line_error(draw.line, e))?;
            }
        }
        Ok(())
    }
}
