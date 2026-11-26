use crate::{Result, fail};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fmt,
    path::{Component, Path},
    str::FromStr,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Room,
    Object,
    Sprite,
    Animation,
    Tilemap,
    Script,
    Audio,
    Particle,
    Palette,
    Pixel,
}

impl Kind {
    pub const ALL: [Self; 10] = [
        Self::Room,
        Self::Object,
        Self::Sprite,
        Self::Animation,
        Self::Tilemap,
        Self::Script,
        Self::Audio,
        Self::Particle,
        Self::Palette,
        Self::Pixel,
    ];
    pub fn name(self) -> &'static str {
        match self {
            Self::Room => "room",
            Self::Object => "object",
            Self::Sprite => "sprite",
            Self::Animation => "animation",
            Self::Tilemap => "tilemap",
            Self::Script => "script",
            Self::Audio => "audio",
            Self::Particle => "particle",
            Self::Palette => "palette",
            Self::Pixel => "pixel",
        }
    }

    pub fn is_source(self) -> bool {
        matches!(self, Self::Script | Self::Pixel)
    }
}
impl fmt::Display for Kind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}
impl FromStr for Kind {
    type Err = Box<dyn std::error::Error + Send + Sync>;
    fn from_str(s: &str) -> Result<Self> {
        Self::ALL
            .into_iter()
            .find(|kind| kind.name() == s)
            .ok_or_else(|| fail(format!("unknown resource kind: {s}")))
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Settings {
    pub title: String,
    pub start: String,
    pub size: [u32; 2],
    pub hz: u32,
    pub seed: u64,
    #[serde(skip_serializing_if = "no")]
    pub pointer: bool,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub bindings: BTreeMap<String, Vec<String>>,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            title: "New game".into(),
            start: "main".into(),
            size: [640, 360],
            hz: 60,
            seed: 1,
            pointer: false,
            bindings: BTreeMap::new(),
        }
    }
}

pub type Color = [u8; 4];
pub const WHITE: Color = [255; 4];
pub const TILE_LAYER: u16 = 1;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Instance(pub String, pub String, pub f32, pub f32);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Light {
    pub at: [f32; 2],
    pub radius: f32,
    pub color: Color,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Label {
    pub at: [f32; 2],
    pub text: String,
    #[serde(default = "white", skip_serializing_if = "is_white")]
    pub color: Color,
    #[serde(default = "font_size", skip_serializing_if = "is_font_size")]
    pub size: u32,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub value: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Meter {
    pub at: [f32; 2],
    pub size: [f32; 2],
    pub value: String,
    pub max: f32,
    #[serde(default = "white", skip_serializing_if = "is_white")]
    pub color: Color,
    #[serde(default = "meter_track", skip_serializing_if = "is_meter_track")]
    pub track: Color,
}

fn meter_track() -> Color {
    [22, 28, 43, 255]
}
fn is_meter_track(value: &Color) -> bool {
    *value == meter_track()
}

impl Default for Meter {
    fn default() -> Self {
        Self {
            at: [16., 32.],
            size: [96., 8.],
            value: "health".into(),
            max: 100.,
            color: WHITE,
            track: meter_track(),
        }
    }
}

fn white() -> Color {
    WHITE
}
fn is_white(value: &Color) -> bool {
    *value == WHITE
}
fn font_size() -> u32 {
    16
}
fn is_font_size(value: &u32) -> bool {
    *value == font_size()
}
fn zero(value: &f32) -> bool {
    *value == 0.0
}
fn no(value: &bool) -> bool {
    !*value
}

pub const MAX_ROOM_DIMENSION: u32 = 524_288;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Camera {
    #[serde(skip_serializing_if = "zero_position")]
    pub at: [f32; 2],
    #[serde(skip_serializing_if = "Option::is_none")]
    pub follow: Option<String>,
    #[serde(skip_serializing_if = "zero_position")]
    pub dead: [f32; 2],
    #[serde(skip_serializing_if = "zero")]
    pub smooth: f32,
}

impl Camera {
    fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

pub fn zero_position(position: &[f32; 2]) -> bool {
    *position == [0., 0.]
}

pub fn clamp_view_origin(mut at: [f32; 2], room: [u32; 2], viewport: [u32; 2]) -> [f32; 2] {
    for axis in 0..2 {
        at[axis] = at[axis].clamp(0., room[axis].saturating_sub(viewport[axis]) as f32);
    }
    at
}

pub fn follow_view_origin(
    at: [f32; 2],
    center: [f32; 2],
    room: [u32; 2],
    viewport: [u32; 2],
    dead: [f32; 2],
) -> [f32; 2] {
    let wanted = std::array::from_fn(|axis| {
        let half_dead = dead[axis].max(0.) / 2.;
        let offset = center[axis] - at[axis] - viewport[axis] as f32 / 2.;
        at[axis] + (offset - offset.clamp(-half_dead, half_dead))
    });
    clamp_view_origin(wanted, room, viewport)
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Room {
    pub bg: Color,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<[u32; 2]>,
    #[serde(skip_serializing_if = "Camera::is_default")]
    pub camera: Camera,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub instances: Vec<Instance>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub maps: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub backdrops: Vec<Backdrop>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub lights: Vec<Light>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub ui: Vec<Label>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub meters: Vec<Meter>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub music: Option<String>,
    #[serde(skip_serializing_if = "is_one")]
    pub music_gain: f32,
    #[serde(skip_serializing_if = "no")]
    pub y_sort: bool,
    #[serde(skip_serializing_if = "zero")]
    pub darkness: f32,
}
impl Default for Room {
    fn default() -> Self {
        Self {
            bg: [21, 25, 37, 255],
            size: None,
            camera: Camera::default(),
            instances: vec![],
            maps: vec![],
            backdrops: vec![],
            lights: vec![],
            ui: vec![],
            meters: vec![],
            music: None,
            music_gain: 1.,
            y_sort: false,
            darkness: 0.,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Backdrop {
    pub sprite: String,
    pub frame: usize,
    pub at: [f32; 2],
    pub size: [f32; 2],
    pub scroll: [f32; 2],
    pub repeat: [bool; 2],
    pub tint: Color,
}

impl Default for Backdrop {
    fn default() -> Self {
        Self {
            sprite: String::new(),
            frame: 0,
            at: [0.; 2],
            size: [64.; 2],
            scroll: [1.; 2],
            repeat: [false; 2],
            tint: WHITE,
        }
    }
}

impl Room {
    pub fn dimensions(&self, viewport: [u32; 2]) -> [u32; 2] {
        self.size.unwrap_or(viewport)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Shape {
    #[default]
    Rect,
    Circle,
}
fn rect(value: &Shape) -> bool {
    *value == Shape::Rect
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Object {
    pub size: [f32; 2],
    pub color: Color,
    #[serde(skip_serializing_if = "rect")]
    pub shape: Shape,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sprite: Option<String>,
    #[serde(skip_serializing_if = "no_flip")]
    pub flip: [bool; 2],
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hitbox: Option<[f32; 4]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub animation: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub script: Option<String>,
    #[serde(skip_serializing_if = "no")]
    pub solid: bool,
    #[serde(skip_serializing_if = "no")]
    pub one_way: bool,
    #[serde(skip_serializing_if = "no")]
    pub hidden: bool,
    #[serde(skip_serializing_if = "zero")]
    pub gravity: f32,
    #[serde(skip_serializing_if = "zero_depth")]
    pub depth: i16,
    #[serde(skip_serializing_if = "default_layer")]
    pub layer: u16,
    #[serde(skip_serializing_if = "default_mask")]
    pub mask: u16,
}
impl Default for Object {
    fn default() -> Self {
        Self {
            size: [16., 16.],
            color: WHITE,
            shape: Shape::Rect,
            sprite: None,
            flip: [false; 2],
            hitbox: None,
            animation: None,
            script: None,
            solid: false,
            one_way: false,
            hidden: false,
            gravity: 0.,
            depth: 0,
            layer: TILE_LAYER,
            mask: u16::MAX,
        }
    }
}

fn no_flip(value: &[bool; 2]) -> bool {
    *value == [false; 2]
}

fn zero_depth(value: &i16) -> bool {
    *value == 0
}
fn default_layer(value: &u16) -> bool {
    *value == TILE_LAYER
}
fn default_mask(value: &u16) -> bool {
    *value == u16::MAX
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Sprite {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub file: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pixel: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub poses: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub frames: Vec<[u32; 4]>,
}

impl Sprite {
    pub fn frame_count(&self) -> usize {
        self.frames.len().max(1)
    }

    pub fn pixel_poses<'a>(&'a self, art: &'a crate::pixel::Art) -> Result<Vec<&'a str>> {
        if self.poses.is_empty() {
            return Ok(art.frame_names().collect());
        }
        if self.poses.len() > crate::pixel::MAX_FRAMES {
            return Err(fail("sprite may select up to 64 poses"));
        }
        self.poses
            .iter()
            .map(|name| {
                if !art.has_frame(name) {
                    return Err(fail(format!("absent pixel frame {name:?}")));
                }
                Ok(name.as_str())
            })
            .collect()
    }
}

#[derive(Clone, Copy, Debug)]
pub struct FrameGrid {
    pub at: [u32; 2],
    pub cell: [u32; 2],
    pub grid: [u32; 2],
    pub gap: [u32; 2],
}

impl FrameGrid {
    pub fn frames(self) -> Result<Vec<[u32; 4]>> {
        if self.cell.contains(&0)
            || self.grid.contains(&0)
            || self.grid[0].saturating_mul(self.grid[1]) > 4096
        {
            return Err(fail(
                "sprite grid needs positive dimensions and at most 4096 frames",
            ));
        }
        let pitch = std::array::from_fn::<_, 2, _>(|axis| {
            u64::from(self.cell[axis]) + u64::from(self.gap[axis])
        });
        for (axis, pitch) in pitch.into_iter().enumerate() {
            let end = u64::from(self.at[axis])
                + u64::from(self.grid[axis] - 1) * pitch
                + u64::from(self.cell[axis]);
            if end > 8192 {
                return Err(fail("sprite grid extends beyond 8192 pixels"));
            }
        }
        let mut frames = Vec::with_capacity((self.grid[0] * self.grid[1]) as usize);
        for row in 0..self.grid[1] {
            for column in 0..self.grid[0] {
                frames.push([
                    (u64::from(self.at[0]) + u64::from(column) * pitch[0]) as u32,
                    (u64::from(self.at[1]) + u64::from(row) * pitch[1]) as u32,
                    self.cell[0],
                    self.cell[1],
                ]);
            }
        }
        Ok(frames)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Animation {
    pub sprite: String,
    pub frames: Vec<usize>,
    pub fps: u32,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub holds: BTreeMap<usize, u16>,
    #[serde(default, skip_serializing_if = "AnimationMode::is_loop")]
    pub mode: AnimationMode,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AnimationMode {
    #[default]
    Loop,
    Once,
    PingPong,
}

impl AnimationMode {
    fn is_loop(&self) -> bool {
        *self == Self::Loop
    }
}

impl Animation {
    pub fn slot(&self, elapsed_frames: u64) -> Option<usize> {
        let len = self.frames.len().checked_sub(1)?;
        if len == 0 {
            return Some(0);
        }
        let total = self.duration();
        let phase = match self.mode {
            AnimationMode::Loop => elapsed_frames % total,
            AnimationMode::Once => elapsed_frames.min(total - 1),
            AnimationMode::PingPong => {
                let first = self.hold(0);
                let last = self.hold(len);
                let phase = elapsed_frames % (2 * total - first - last);
                if phase < total {
                    phase
                } else {
                    total - last - 1 - (phase - total)
                }
            }
        };
        let mut extras = 0;
        for (&slot, &hold) in self.holds.range(..=len) {
            let start = slot as u64 + extras;
            if phase < start {
                break;
            }
            let hold = u64::from(hold.max(1));
            if phase < start + hold {
                return Some(slot);
            }
            extras += hold - 1;
        }
        Some((phase - extras) as usize)
    }

    fn hold(&self, slot: usize) -> u64 {
        u64::from(self.holds.get(&slot).copied().unwrap_or(1).max(1))
    }

    pub fn duration(&self) -> u64 {
        self.frames.len() as u64
            + self
                .holds
                .range(..self.frames.len())
                .map(|(_, &hold)| u64::from(hold.saturating_sub(1)))
                .sum::<u64>()
    }

    pub fn elapsed_frames(&self, ticks: u64, hz: u32) -> u64 {
        ticks.saturating_mul(u64::from(self.fps)) / u64::from(hz.max(1))
    }

    pub fn done(&self, elapsed_frames: u64) -> bool {
        self.mode == AnimationMode::Once && elapsed_frames >= self.duration()
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Tile {
    Flat(FlatTile),
    Atlas(AtlasTile),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FlatTile(
    pub Color,
    #[serde(default, skip_serializing_if = "no")] pub bool,
);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AtlasTile(
    pub usize,
    #[serde(default)] pub bool,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub Option<Color>,
);

impl Tile {
    pub fn flat(color: Color, solid: bool) -> Self {
        Self::Flat(FlatTile(color, solid))
    }

    pub fn frame(&self) -> Option<usize> {
        match self {
            Self::Flat(_) => None,
            Self::Atlas(tile) => Some(tile.0),
        }
    }

    pub fn color(&self) -> Color {
        match self {
            Self::Flat(tile) => tile.0,
            Self::Atlas(tile) => tile.2.unwrap_or(WHITE),
        }
    }

    pub fn solid(&self) -> bool {
        match self {
            Self::Flat(tile) => tile.1,
            Self::Atlas(tile) => tile.1,
        }
    }

    pub fn set_style(&mut self, frame: Option<usize>, color: Color, solid: bool) {
        *self = match frame {
            Some(frame) => Self::Atlas(AtlasTile(frame, solid, (color != WHITE).then_some(color))),
            None => Self::flat(color, solid),
        };
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Tilemap {
    pub cell: u32,
    #[serde(default, skip_serializing_if = "is_origin")]
    pub at: [i32; 2],
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub atlas: Option<String>,
    pub tiles: BTreeMap<char, Tile>,
    pub rows: Vec<String>,
}

impl Tilemap {
    /// Replace a palette symbol without disturbing neighboring cells.
    pub fn replace_in(&mut self, area: Option<[usize; 4]>, from: char, to: char) -> Result<()> {
        for key in [from, to] {
            if key != '.' && (!key.is_ascii_graphic() || !self.tiles.contains_key(&key)) {
                return Err(fail("tile replacement needs defined symbols or '.'"));
            }
        }
        let [x, y, w, h] = area.unwrap_or_else(|| {
            [
                0,
                0,
                self.rows.first().map_or(0, String::len),
                self.rows.len(),
            ]
        });
        let replacement = to.to_string();
        for row in self.area_mut([x, y, w, h])? {
            let range = x..x + w;
            if row[range.clone()].contains(from) {
                let next = row[range.clone()].replace(from, &replacement);
                row.replace_range(range, &next);
            }
        }
        Ok(())
    }

    pub fn resize(
        &mut self,
        [width, height]: [usize; 2],
        [dx, dy]: [i32; 2],
        tile: char,
    ) -> Result<()> {
        if !(1..=1024).contains(&width) || !(1..=1024).contains(&height) {
            return Err(fail("map dimensions must be 1–1024 cells per axis"));
        }
        if tile != '.' && (!tile.is_ascii_graphic() || !self.tiles.contains_key(&tile)) {
            return Err(fail(
                "resize tile must be empty space or a defined ASCII tile",
            ));
        }
        if self.rows.iter().any(|row| !row.is_ascii()) {
            return Err(fail("resize requires ASCII map rows"));
        }
        let mut rows = vec![tile.to_string().repeat(width); height];
        let source_x = (0i64 - i64::from(dx)).max(0) as usize;
        let target_x = dx.max(0) as usize;
        let source_y = (0i64 - i64::from(dy)).max(0) as usize;
        let target_y = dy.max(0) as usize;
        for (source, target) in self
            .rows
            .iter()
            .skip(source_y)
            .zip(rows.iter_mut().skip(target_y))
        {
            let count = source
                .len()
                .saturating_sub(source_x)
                .min(width.saturating_sub(target_x));
            if count != 0 {
                target.replace_range(
                    target_x..target_x + count,
                    &source[source_x..source_x + count],
                );
            }
        }
        self.rows = rows;
        Ok(())
    }

    /// Paint a rectangular area, optionally leaving its interior intact.
    pub fn fill_area(&mut self, [x, y, w, h]: [usize; 4], tile: char, border: bool) -> Result<()> {
        let rows = self.area_mut([x, y, w, h])?;
        if !tile.is_ascii() {
            return Err(fail("tile must be ASCII"));
        }
        for (index, row) in rows.iter_mut().enumerate() {
            let replacement = if border && index != 0 && index + 1 != h && w > 2 {
                format!("{tile}{}{tile}", &row[x + 1..x + w - 1])
            } else {
                tile.to_string().repeat(w)
            };
            row.replace_range(x..x + w, &replacement);
        }
        Ok(())
    }

    /// Read character columns and rows without encoding the rest of the map.
    pub fn read_area(&self, [x, y, w, h]: [usize; 4]) -> Result<Vec<&str>> {
        let rows = self.area_rows([x, y, w, h])?;
        Ok(self.rows[rows].iter().map(|row| &row[x..x + w]).collect())
    }

    pub(crate) fn area_mut(&mut self, area: [usize; 4]) -> Result<&mut [String]> {
        let rows = self.area_rows(area)?;
        Ok(&mut self.rows[rows])
    }

    fn area_rows(&self, [x, y, w, h]: [usize; 4]) -> Result<std::ops::Range<usize>> {
        let range = y..y.saturating_add(h);
        let rows = self
            .rows
            .get(range.clone())
            .ok_or_else(|| fail("area exceeds map height"))?;
        if w == 0
            || h == 0
            || rows
                .iter()
                .any(|row| !row.is_ascii() || x.saturating_add(w) > row.len())
        {
            return Err(fail("empty area, non-ASCII map or area exceeds map width"));
        }
        Ok(range)
    }

    /// Replace the edge-connected region containing `at` with a palette tile or empty space.
    pub fn flood(&mut self, at: [usize; 2], tile: char) -> Result<()> {
        let [x, y] = at;
        let width = self.rows.first().map_or(0, String::len);
        let height = self.rows.len();
        if width > 1024
            || height > 1024
            || self
                .rows
                .iter()
                .any(|row| row.len() != width || !row.is_ascii())
        {
            return Err(fail(
                "flood requires a rectangular ASCII map up to 1024×1024",
            ));
        }
        if x >= width || y >= height {
            return Err(fail("flood origin is outside the map"));
        }
        if tile != '.' && (!tile.is_ascii_graphic() || !self.tiles.contains_key(&tile)) {
            return Err(fail(
                "flood tile must be empty space or a defined ASCII tile",
            ));
        }
        let target = self.rows[y].as_bytes()[x];
        if char::from(target) == tile {
            return Ok(());
        }
        let mut buffer = [0; 4];
        let replacement = tile.encode_utf8(&mut buffer);
        let mut pending = vec![at];
        while let Some([x, y]) = pending.pop() {
            let row = &mut self.rows[y];
            if row.as_bytes()[x] != target {
                continue;
            }
            row.replace_range(x..x + 1, replacement);
            if x > 0 {
                pending.push([x - 1, y]);
            }
            if x + 1 < width {
                pending.push([x + 1, y]);
            }
            if y > 0 {
                pending.push([x, y - 1]);
            }
            if y + 1 < height {
                pending.push([x, y + 1]);
            }
        }
        Ok(())
    }

    pub fn set_atlas_palette(
        &mut self,
        sprite: &str,
        frame_count: usize,
        keys: &str,
        first: usize,
        solid: &str,
    ) -> Result<()> {
        let unique = keys.chars().collect::<std::collections::BTreeSet<_>>();
        if keys.is_empty()
            || keys.len() != unique.len()
            || unique.iter().any(|c| *c == '.' || !c.is_ascii_graphic())
        {
            return Err(fail(
                "atlas keys must be distinct printable ASCII characters other than '.'",
            ));
        }
        if solid.chars().any(|key| !unique.contains(&key)) {
            return Err(fail("solid keys must be in atlas keys"));
        }
        if first.saturating_add(unique.len()) > frame_count {
            return Err(fail("atlas palette exceeds the sprite's frame count"));
        }
        if self.atlas.as_deref().is_some_and(|atlas| atlas != sprite)
            && self
                .tiles
                .iter()
                .any(|(key, tile)| tile.frame().is_some() && !unique.contains(key))
        {
            return Err(fail(
                "include every existing textured key when switching atlases",
            ));
        }
        self.atlas = Some(sprite.into());
        for (index, key) in keys.chars().enumerate() {
            self.tiles.insert(
                key,
                Tile::Atlas(AtlasTile(first + index, solid.contains(key), None)),
            );
        }
        Ok(())
    }
}
fn is_origin(value: &[i32; 2]) -> bool {
    *value == [0, 0]
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Audio {
    pub file: String,
    #[serde(default = "one", skip_serializing_if = "is_one")]
    pub gain: f32,
}
fn one() -> f32 {
    1.0
}
fn is_one(value: &f32) -> bool {
    *value == 1.0
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Particle {
    pub count: u32,
    pub life: u32,
    pub speed: f32,
    pub radius: f32,
    pub color: Color,
    #[serde(default, skip_serializing_if = "zero")]
    pub gravity: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fade: Option<u32>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Resource {
    Room(Room),
    Object(Object),
    Sprite(Sprite),
    Animation(Animation),
    Tilemap(Tilemap),
    Script(String),
    Audio(Audio),
    Particle(Particle),
    Palette(crate::pixel::Profile),
    Pixel(String),
}
impl Resource {
    pub fn kind(&self) -> Kind {
        match self {
            Self::Room(_) => Kind::Room,
            Self::Object(_) => Kind::Object,
            Self::Sprite(_) => Kind::Sprite,
            Self::Animation(_) => Kind::Animation,
            Self::Tilemap(_) => Kind::Tilemap,
            Self::Script(_) => Kind::Script,
            Self::Audio(_) => Kind::Audio,
            Self::Particle(_) => Kind::Particle,
            Self::Palette(_) => Kind::Palette,
            Self::Pixel(_) => Kind::Pixel,
        }
    }
    pub fn parse(kind: Kind, value: &str) -> Result<Self> {
        Ok(match kind {
            Kind::Room => Self::Room(serde_json::from_str(value)?),
            Kind::Object => Self::Object(serde_json::from_str(value)?),
            Kind::Sprite => Self::Sprite(serde_json::from_str(value)?),
            Kind::Animation => Self::Animation(serde_json::from_str(value)?),
            Kind::Tilemap => Self::Tilemap(serde_json::from_str(value)?),
            Kind::Script => Self::Script(
                value
                    .replace("\r\n", "\n")
                    .trim_end_matches('\n')
                    .to_owned(),
            ),
            Kind::Audio => Self::Audio(serde_json::from_str(value)?),
            Kind::Particle => Self::Particle(serde_json::from_str(value)?),
            Kind::Palette => Self::Palette(serde_json::from_str(value)?),
            Kind::Pixel => Self::Pixel(
                value
                    .replace("\r\n", "\n")
                    .trim_end_matches('\n')
                    .to_owned(),
            ),
        })
    }
    pub fn encode(&self, pretty: bool) -> String {
        fn encode<T: Serialize>(value: &T, pretty: bool) -> String {
            if pretty {
                serde_json::to_string_pretty(value)
            } else {
                serde_json::to_string(value)
            }
            .expect("project types serialize")
        }
        match self {
            Self::Room(x) => encode(x, pretty),
            Self::Object(x) => encode(x, pretty),
            Self::Sprite(x) => encode(x, pretty),
            Self::Animation(x) => encode(x, pretty),
            Self::Tilemap(x) => encode(x, pretty),
            Self::Script(x) | Self::Pixel(x) => x.clone(),
            Self::Audio(x) => encode(x, pretty),
            Self::Particle(x) => encode(x, pretty),
            Self::Palette(x) => encode(x, pretty),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Project {
    pub settings: Settings,
    pub resources: BTreeMap<(Kind, String), Resource>,
}
impl Default for Project {
    fn default() -> Self {
        let settings = Settings::default();
        Self {
            resources: BTreeMap::from([(
                (Kind::Room, settings.start.clone()),
                Resource::Room(Room::default()),
            )]),
            settings,
        }
    }
}
impl Project {
    pub fn get(&self, kind: Kind, id: &str) -> Option<&Resource> {
        self.resources.get(&(kind, id.into()))
    }
    pub fn get_mut(&mut self, kind: Kind, id: &str) -> Option<&mut Resource> {
        self.resources.get_mut(&(kind, id.into()))
    }
    pub fn put(&mut self, id: String, resource: Resource) {
        self.resources.insert((resource.kind(), id), resource);
    }
    pub fn parse(text: &str) -> Result<Self> {
        let mut lines = text
            .strip_prefix('\u{feff}')
            .unwrap_or(text)
            .lines()
            .enumerate();
        let (_, header) = lines.next().ok_or_else(|| fail("missing agge/1 header"))?;
        let settings = serde_json::from_str(
            header
                .strip_prefix("agge/1 ")
                .ok_or_else(|| fail("expected agge/1 header"))?,
        )?;
        let mut project = Self {
            settings,
            resources: BTreeMap::new(),
        };
        while let Some((line, entry)) = lines.next() {
            if entry.trim().is_empty() {
                continue;
            }
            let (kind, body) = entry
                .split_once(' ')
                .ok_or_else(|| fail(format!("line {}: expected resource", line + 1)))?;
            let kind = Kind::from_str(kind)?;
            let (id, resource) = if kind.is_source() {
                let mut source = Vec::new();
                loop {
                    let (_, code) = lines
                        .next()
                        .ok_or_else(|| fail(format!("line {}: unterminated {kind}", line + 1)))?;
                    if code == "---" {
                        break;
                    }
                    source.push(code);
                }
                (body, Resource::parse(kind, &source.join("\n"))?)
            } else {
                let (id, value) = body
                    .split_once(' ')
                    .ok_or_else(|| fail(format!("line {}: missing JSON", line + 1)))?;
                (
                    id,
                    Resource::parse(kind, value)
                        .map_err(|e| fail(format!("line {}: {e}", line + 1)))?,
                )
            };
            if !valid_id(id) {
                return Err(fail(format!("line {}: invalid ID {id}", line + 1)));
            }
            if project
                .resources
                .insert((kind, id.into()), resource)
                .is_some()
            {
                return Err(fail(format!("line {}: duplicate {kind} {id}", line + 1)));
            }
        }
        Ok(project)
    }
    pub fn asset_paths(&self) -> std::collections::BTreeSet<&str> {
        self.resources
            .values()
            .filter_map(|resource| match resource {
                Resource::Sprite(sprite) if !sprite.file.is_empty() => Some(sprite.file.as_str()),
                Resource::Audio(audio) => Some(audio.file.as_str()),
                _ => None,
            })
            .collect()
    }

    pub fn encode(&self) -> String {
        use std::fmt::Write;
        let mut out = format!(
            "agge/1 {}\n",
            serde_json::to_string(&self.settings).expect("settings serialize")
        );
        for ((kind, id), resource) in &self.resources {
            let value = resource.encode(false);
            if kind.is_source() {
                writeln!(out, "{kind} {id}\n{value}\n---").expect("string write");
            } else {
                writeln!(out, "{kind} {id} {value}").expect("string write");
            }
        }
        out
    }
}

pub fn valid_id(id: &str) -> bool {
    (1..=48).contains(&id.len())
        && id.as_bytes()[0].is_ascii_lowercase()
        && id
            .bytes()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'_')
}

pub fn asset_path(file: &str) -> Result<&Path> {
    let path = Path::new(file);
    if file.is_empty()
        || file.contains(['\\', ':'])
        || file.split('/').any(|part| {
            part.is_empty() || part == "." || part == ".." || part.ends_with(['.', ' '])
        })
        || !path
            .components()
            .all(|part| matches!(part, Component::Normal(_)))
    {
        return Err(fail(format!("invalid relative asset path: {file}")));
    }
    Ok(path)
}

const MAX_ASSET: u64 = 32 * 1024 * 1024;

fn open_asset(root: &Path, file: &str) -> Result<std::fs::File> {
    let root = root.canonicalize()?;
    let path = root.join(asset_path(file)?).canonicalize()?;
    if !path.starts_with(&root) || !path.is_file() {
        return Err(fail(format!(
            "asset is outside the project or not a file: {file}"
        )));
    }
    let handle = std::fs::File::open(path)?;
    if handle.metadata()?.len() > MAX_ASSET {
        return Err(fail(format!("asset exceeds 32 MiB: {file}")));
    }
    Ok(handle)
}

pub fn read_asset(root: &Path, file: &str) -> Result<Vec<u8>> {
    use std::io::Read;
    let mut bytes = Vec::new();
    open_asset(root, file)?
        .take(MAX_ASSET + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_ASSET {
        return Err(fail(format!("asset exceeds 32 MiB: {file}")));
    }
    Ok(bytes)
}

pub(crate) fn fingerprint_asset(root: &Path, file: &str) -> Result<String> {
    use std::io::Read;
    let mut input = open_asset(root, file)?.take(MAX_ASSET + 1);
    let mut hash = blake3::Hasher::new();
    if std::io::copy(&mut input, &mut hash)? > MAX_ASSET {
        return Err(fail(format!("asset exceeds 32 MiB: {file}")));
    }
    Ok(hash.finalize().to_hex().to_string())
}
