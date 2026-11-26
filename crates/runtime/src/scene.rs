use crate::{
    Bounds, World,
    asset::{audio_info, png_decoder},
};
use agge_core::{
    Animation, Color, Instance, Kind, Light, Object, Project, Resource, Result, Room, Shape, Tile,
    Tilemap, fail, read_asset,
};
use image::{ImageEncoder, RgbaImage, codecs::png::PngEncoder};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
    sync::Arc,
};

#[derive(Clone, Debug)]
pub enum Draw<'a> {
    Rect(Bounds, Color),
    Circle(Bounds, Color),
    Sprite(Bounds, &'a str, usize, Color),
    TileLayer(&'a Tilemap, Bounds),
    Text([f32; 2], String, u32, Color),
}

#[derive(Default)]
pub struct Media {
    pub sprites: BTreeMap<String, Arc<RgbaImage>>,
}
impl Media {
    pub fn load(project: &Project, root: &Path) -> Result<Self> {
        let mut media = Self::default();
        let mut sheets = BTreeMap::<&str, Arc<RgbaImage>>::new();
        let mut checked_audio = BTreeSet::new();
        for ((kind, id), resource) in &project.resources {
            if let Resource::Sprite(sprite) = resource {
                let image = match sheets.entry(&sprite.file) {
                    std::collections::btree_map::Entry::Occupied(entry) => Arc::clone(entry.get()),
                    std::collections::btree_map::Entry::Vacant(entry) => {
                        Arc::clone(entry.insert(Arc::new(decode_sprite(root, id, &sprite.file)?)))
                    }
                };
                if sprite
                    .frames
                    .iter()
                    .any(|&frame| !within_image(frame, &image))
                {
                    return Err(fail(format!(
                        "{kind}/{id}: sprite frame extends outside {}×{} image",
                        image.width(),
                        image.height()
                    )));
                }
                media.sprites.insert(id.clone(), image);
            }
            if let Resource::Audio(audio) = resource
                && checked_audio.insert(audio.file.as_str())
            {
                let bytes =
                    read_asset(root, &audio.file).map_err(|e| fail(format!("{kind}/{id}: {e}")))?;
                audio_info(&audio.file, bytes).map_err(|e| fail(format!("{kind}/{id}: {e}")))?;
            }
        }
        Ok(media)
    }
    pub fn load_sprite(&mut self, root: &Path, id: &str, file: &str) -> Result<()> {
        self.sprites
            .insert(id.into(), Arc::new(decode_sprite(root, id, file)?));
        Ok(())
    }

    pub fn frame(
        &self,
        project: &Project,
        sprite: &str,
        frame: usize,
    ) -> Option<([u32; 4], &RgbaImage)> {
        let image = self.sprites.get(sprite)?.as_ref();
        let Resource::Sprite(data) = project.get(Kind::Sprite, sprite)? else {
            return None;
        };
        let region = if data.frames.is_empty() && frame == 0 {
            [0, 0, image.width(), image.height()]
        } else {
            *data.frames.get(frame)?
        };
        within_image(region, image).then_some((region, image))
    }
}

fn decode_sprite(root: &Path, id: &str, file: &str) -> Result<RgbaImage> {
    let decode = || -> Result<RgbaImage> {
        let decoder = png_decoder(read_asset(root, file)?)?;
        Ok(image::DynamicImage::from_decoder(decoder)?.into_rgba8())
    };
    decode().map_err(|e| fail(format!("sprite/{id}: {e}")))
}

fn within_image([x, y, w, h]: [u32; 4], image: &RgbaImage) -> bool {
    w > 0 && h > 0 && x.saturating_add(w) <= image.width() && y.saturating_add(h) <= image.height()
}

pub fn edit_scene<'a>(
    project: &'a Project,
    room: &'a Room,
    view: Bounds,
    draw: &mut impl FnMut(Draw<'a>),
) {
    tiles(project, room, view, draw);
    let mut candidates = room_placements(project, room)
        .filter(|(_, _, bounds, _)| bounds.overlaps(view))
        .collect::<Vec<_>>();
    sort_draws(&mut candidates, room.y_sort);
    for (_, object, bounds, _) in candidates {
        draw(edit_object_draw(project, object, bounds));
    }
}

fn edit_object_draw<'a>(project: &'a Project, object: &'a Object, bounds: Bounds) -> Draw<'a> {
    let clip = object
        .animation
        .as_deref()
        .and_then(|id| project.get(Kind::Animation, id));
    let animation = match clip {
        Some(Resource::Animation(clip)) => Some((clip, 0)),
        _ => None,
    };
    object_draw(object, bounds, animation)
}

fn room_placements<'project, 'room>(
    project: &'project Project,
    room: &'room Room,
) -> impl Iterator<Item = (u64, &'project Object, Bounds, &'room Instance)> {
    room.instances
        .iter()
        .enumerate()
        .filter_map(|(order, placement)| {
            let Resource::Object(object) = project.get(Kind::Object, &placement.1)? else {
                return None;
            };
            let bounds = Bounds::new(placement.2, placement.3, object.size[0], object.size[1]);
            Some((order as u64, object, bounds, placement))
        })
}

/// Pick the frontmost room placement whose drawn shape contains the point.
pub fn pick_placement<'a>(project: &Project, room: &'a Room, [x, y]: [f32; 2]) -> Option<&'a str> {
    room_placements(project, room)
        .filter(|(_, object, bounds, _)| {
            if !(bounds.left..bounds.right).contains(&x)
                || !(bounds.top..bounds.bottom).contains(&y)
            {
                return false;
            }
            if matches!(edit_object_draw(project, object, *bounds), Draw::Circle(..)) {
                let u = (x - bounds.left) / (bounds.right - bounds.left);
                let v = (y - bounds.top) / (bounds.bottom - bounds.top);
                inside_ellipse(u, v)
            } else {
                true
            }
        })
        .max_by(|a, b| draw_order(a, b, room.y_sort))
        .map(|(_, _, _, placement)| placement.0.as_str())
}

fn sort_draws<T>(items: &mut [(u64, &Object, Bounds, T)], y_sort: bool) {
    items.sort_unstable_by(|a, b| draw_order(a, b, y_sort));
}

fn draw_order<T>(
    a: &(u64, &Object, Bounds, T),
    b: &(u64, &Object, Bounds, T),
    y_sort: bool,
) -> std::cmp::Ordering {
    a.1.depth
        .cmp(&b.1.depth)
        .then_with(|| {
            if y_sort {
                a.2.bottom.total_cmp(&b.2.bottom)
            } else {
                std::cmp::Ordering::Equal
            }
        })
        .then_with(|| a.0.cmp(&b.0))
}

impl World {
    pub fn draw_scene<'a>(&'a self, view: Bounds, draw: &mut impl FnMut(Draw<'a>)) {
        tiles(&self.project, self.room(), view, draw);
        let candidates = self.entities.values().filter_map(|entity| {
            let object = self.object(&entity.object);
            let bounds = entity.bounds(object);
            (!object.hidden && bounds.overlaps(view)).then_some((entity.id, object, bounds, entity))
        });
        let mut paint = |(_, object, bounds, entity)| {
            draw(object_draw(
                object,
                bounds,
                self.animation(entity)
                    .map(|(_, clip, elapsed)| (clip, elapsed)),
            ));
        };
        if self.has_depth || self.room().y_sort {
            let mut ordered = candidates.collect::<Vec<_>>();
            sort_draws(&mut ordered, self.room().y_sort);
            ordered.into_iter().for_each(&mut paint);
        } else {
            candidates.for_each(&mut paint);
        }
        for spark in &self.particles {
            let Some(Resource::Particle(style)) = self.project.get(Kind::Particle, &spark.style)
            else {
                unreachable!()
            };
            let d = style.radius * 2.;
            let bounds = Bounds::new(spark.x - style.radius, spark.y - style.radius, d, d);
            if bounds.overlaps(view) {
                let mut color = style.color;
                color[3] = (color[3] as u32 * (style.life - spark.age) / style.life) as u8;
                draw(Draw::Circle(bounds, color));
            }
        }
    }
    pub fn draw_ui<'a>(&'a self, draw: &mut impl FnMut(Draw<'a>)) {
        draw_hud(self.room(), Some(&self.globals), draw);
    }

    /// Visit the current colliders in the runtime camera view.
    pub fn visit_colliders(&self, mut visit: impl FnMut(RoomCollider)) {
        let view = self.view();
        tile_colliders(self.project(), self.room(), view, &mut visit);
        for entity in self.entities.values() {
            let object = self.object(&entity.object);
            visit_object_collider(object, entity.bounds(object), view, &mut visit);
        }
    }
}

pub fn draw_hud<'a>(
    room: &Room,
    globals: Option<&BTreeMap<String, f32>>,
    draw: &mut impl FnMut(Draw<'a>),
) {
    for meter in &room.meters {
        let [x, y] = meter.at;
        let [width, height] = meter.size;
        let fraction = globals.map_or(0.5, |globals| {
            (globals.get(&meter.value).copied().unwrap_or(0.) / meter.max).clamp(0., 1.)
        });
        draw(Draw::Rect(Bounds::new(x, y, width, height), meter.track));
        if fraction > 0. {
            draw(Draw::Rect(
                Bounds::new(x, y, width * fraction, height),
                meter.color,
            ));
        }
    }
    for label in &room.ui {
        let text = match globals {
            Some(globals) if !label.value.is_empty() => format!(
                "{}{}",
                label.text,
                globals.get(&label.value).copied().unwrap_or(0.)
            ),
            _ => label.text.clone(),
        };
        draw(Draw::Text(label.at, text, label.size, label.color));
    }
}
fn tiles<'a>(project: &'a Project, room: &Room, view: Bounds, draw: &mut impl FnMut(Draw<'a>)) {
    for id in &room.maps {
        let Some(Resource::Tilemap(map)) = project.get(Kind::Tilemap, id) else {
            continue;
        };
        draw(Draw::TileLayer(map, view));
    }
}

pub fn draw_tilemap<'a>(map: &'a Tilemap, view: Bounds, draw: &mut impl FnMut(Draw<'a>)) {
    visible_tiles(map, view, |bounds, tile| {
        match (map.atlas.as_deref(), tile.frame()) {
            (Some(atlas), Some(frame)) => draw(Draw::Sprite(bounds, atlas, frame, tile.color())),
            _ => draw(Draw::Rect(bounds, tile.color())),
        }
    });
}

fn visible_tiles(map: &Tilemap, view: Bounds, mut visit: impl FnMut(Bounds, &Tile)) {
    let cell = map.cell.max(1) as f32;
    let [left, top] = map.at.map(|position| position as f32);
    if view.right <= left || view.bottom <= top {
        return;
    }
    let column = |x: f32| ((x - left) / cell).max(0.);
    let row = |y: f32| ((y - top) / cell).max(0.);
    let x0 = column(view.left).floor() as usize;
    let x1 = column(view.right).ceil() as usize;
    let y0 = row(view.top).floor() as usize;
    let y1 = (row(view.bottom).ceil() as usize).min(map.rows.len());
    for y in y0..y1 {
        let text = map.rows[y].as_bytes();
        if let Some(visible) = text.get(x0..x1.min(text.len())) {
            for (i, &symbol) in visible.iter().enumerate() {
                if let Some(tile) = map.tiles.get(&(symbol as char)) {
                    let bounds = Bounds::new(
                        left + (x0 + i) as f32 * cell,
                        top + y as f32 * cell,
                        cell,
                        cell,
                    );
                    visit(bounds, tile);
                }
            }
        }
    }
}

pub enum RoomCollider {
    Solid(Bounds),
    OneWay(Bounds),
}

/// Visit the authored colliders visible in the room editor.
pub fn room_colliders(
    project: &Project,
    room: &Room,
    view: Bounds,
    mut visit: impl FnMut(RoomCollider),
) {
    tile_colliders(project, room, view, &mut visit);
    for (_, object, bounds, _) in room_placements(project, room) {
        visit_object_collider(object, bounds, view, &mut visit);
    }
}

fn tile_colliders(
    project: &Project,
    room: &Room,
    view: Bounds,
    visit: &mut impl FnMut(RoomCollider),
) {
    for id in &room.maps {
        if let Some(Resource::Tilemap(map)) = project.get(Kind::Tilemap, id) {
            visible_tiles(map, view, |bounds, tile| {
                if tile.solid() {
                    visit(RoomCollider::Solid(bounds));
                }
            });
        }
    }
}

fn visit_object_collider(
    object: &Object,
    bounds: Bounds,
    view: Bounds,
    visit: &mut impl FnMut(RoomCollider),
) {
    if bounds.overlaps(view) {
        if object.solid {
            visit(RoomCollider::Solid(bounds));
        } else if object.one_way {
            visit(RoomCollider::OneWay(bounds));
        }
    }
}

fn object_draw<'a>(
    object: &'a Object,
    bounds: Bounds,
    animation: Option<(&'a Animation, u64)>,
) -> Draw<'a> {
    if let Some((anim, elapsed)) = animation
        && let Some(slot) = anim.slot(elapsed)
    {
        return Draw::Sprite(bounds, &anim.sprite, anim.frames[slot], object.color);
    }
    if let Some(id) = &object.sprite {
        Draw::Sprite(bounds, id, 0, object.color)
    } else if object.shape == Shape::Circle {
        Draw::Circle(bounds, object.color)
    } else {
        Draw::Rect(bounds, object.color)
    }
}

pub fn shade(room: &Room, x: f32, y: f32) -> Color {
    shade_lights(room.darkness, &room.lights, [x, y])
}

pub fn shade_lights<'a>(
    darkness: f32,
    lights: impl IntoIterator<Item = &'a Light>,
    [x, y]: [f32; 2],
) -> Color {
    let mut alpha = darkness;
    let mut tint = [0.; 3];
    for light in lights {
        let distance = (x - light.at[0]).hypot(y - light.at[1]);
        let weight = (1. - distance / light.radius).max(0.);
        alpha *= 1. - weight;
        for (value, channel) in tint.iter_mut().zip(light.color) {
            *value += channel as f32 * weight * light.color[3] as f32 / 255. * 0.38;
        }
    }
    [
        tint[0].min(255.) as u8,
        tint[1].min(255.) as u8,
        tint[2].min(255.) as u8,
        (alpha * 255.) as u8,
    ]
}

pub fn capture(world: &World, media: &Media, area: [u32; 4]) -> Result<Vec<u8>> {
    let [x, y, w, h] = area;
    let size = world.project.settings.size;
    if !(1..=2048).contains(&w)
        || !(1..=2048).contains(&h)
        || x.saturating_add(w) > size[0]
        || y.saturating_add(h) > size[1]
    {
        return Err(fail(
            "capture must be within the game view and at most 2048×2048",
        ));
    }
    let screen = Bounds::new(x as f32, y as f32, w as f32, h as f32);
    let [camera_x, camera_y] = world.camera();
    let view = Bounds::new(
        screen.left + camera_x,
        screen.top + camera_y,
        w as f32,
        h as f32,
    );
    let mut buffer = RgbaImage::from_pixel(w, h, image::Rgba(world.room().bg));
    world.draw_scene(view, &mut |draw| {
        raster(&mut buffer, view, draw, media, &world.project)
    });
    if world.room().darkness > 0. {
        for (px, py, color) in buffer.enumerate_pixels_mut() {
            let light = shade(world.room(), view.left + px as f32, view.top + py as f32);
            blend(&mut color.0, light);
        }
    }
    world.draw_ui(&mut |draw| raster(&mut buffer, screen, draw, media, &world.project));
    encode_png(&buffer)
}

fn blend(dest: &mut Color, src: Color) {
    let source_alpha = u32::from(src[3]);
    // Keep both weights on the same scale until the final division.
    let source_weight = source_alpha * 255;
    let dest_weight = u32::from(dest[3]) * (255 - source_alpha);
    let alpha = source_weight + dest_weight;
    if alpha == 0 {
        return;
    }
    for i in 0..3 {
        dest[i] =
            ((u32::from(dest[i]) * dest_weight + u32::from(src[i]) * source_weight) / alpha) as u8;
    }
    dest[3] = ((alpha + 127) / 255) as u8;
}

fn inside_ellipse(u: f32, v: f32) -> bool {
    (u - 0.5).powi(2) + (v - 0.5).powi(2) <= 0.25
}

fn raster(buffer: &mut RgbaImage, view: Bounds, draw: Draw<'_>, media: &Media, project: &Project) {
    let mut fill = |b: Bounds, color: &dyn Fn(f32, f32) -> Option<Color>| {
        let width = b.right - b.left;
        let height = b.bottom - b.top;
        if width <= 0. || height <= 0. {
            return;
        }
        for y in ((b.top - view.top).floor().max(0.) as u32)
            ..((b.bottom - view.top).ceil().max(0.) as u32).min(buffer.height())
        {
            for x in ((b.left - view.left).floor().max(0.) as u32)
                ..((b.right - view.left).ceil().max(0.) as u32).min(buffer.width())
            {
                let u = (x as f32 + view.left + 0.5 - b.left) / width;
                let v = (y as f32 + view.top + 0.5 - b.top) / height;
                if (0. ..1.).contains(&u)
                    && (0. ..1.).contains(&v)
                    && let Some(src) = color(u, v)
                {
                    blend(&mut buffer.get_pixel_mut(x, y).0, src);
                }
            }
        }
    };
    match draw {
        Draw::Rect(b, color) => fill(b, &|_, _| Some(color)),
        Draw::Circle(b, color) => fill(b, &|u, v| inside_ellipse(u, v).then_some(color)),
        Draw::Sprite(b, id, frame, tint) => {
            if let Some(([sx, sy, w, h], image)) = media.frame(project, id, frame) {
                fill(b, &|u, v| {
                    let mut color = image
                        .get_pixel(
                            sx + ((u * w as f32) as u32).min(w - 1),
                            sy + ((v * h as f32) as u32).min(h - 1),
                        )
                        .0;
                    for (c, t) in color.iter_mut().zip(tint) {
                        *c = (*c as u32 * t as u32 / 255) as u8;
                    }
                    Some(color)
                });
            } else {
                fill(b, &|_, _| Some(tint));
            }
        }
        Draw::TileLayer(map, visible) => {
            draw_tilemap(map, visible, &mut |tile| {
                raster(buffer, view, tile, media, project)
            });
        }
        Draw::Text([x, y], text, size, color) => {
            use font8x8::UnicodeFonts;
            let cell = (size as f32 / 8.).max(1.);
            for (row, line) in text.lines().enumerate() {
                for (i, c) in line.chars().enumerate() {
                    let glyph = font8x8::BASIC_FONTS
                        .get(c)
                        .unwrap_or([0, 24, 36, 4, 8, 0, 8, 0]);
                    let b = Bounds::new(
                        x + i as f32 * cell * 8.,
                        y + row as f32 * cell * 8.,
                        cell * 8.,
                        cell * 8.,
                    );
                    fill(b, &|u, v| {
                        (glyph[((v * 8.) as usize).min(7)] & (1 << ((u * 8.) as u32).min(7)) != 0)
                            .then_some(color)
                    });
                }
            }
        }
    }
}

pub fn encode_png(image: &RgbaImage) -> Result<Vec<u8>> {
    let mut bytes = vec![];
    PngEncoder::new(&mut bytes).write_image(
        image.as_raw(),
        image.width(),
        image.height(),
        image::ExtendedColorType::Rgba8,
    )?;
    Ok(bytes)
}
