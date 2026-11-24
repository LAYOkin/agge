use crate::{Bounds, World};
use agge_core::{
    Color, Kind, Light, Object, Project, Resource, Result, Room, Shape, Tilemap, fail, read_asset,
};
use image::{
    ImageEncoder, RgbaImage,
    codecs::png::{PngDecoder, PngEncoder},
};
use std::{collections::BTreeMap, io::Cursor, path::Path};

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
    pub sprites: BTreeMap<String, RgbaImage>,
}
impl Media {
    pub fn load(project: &Project, root: &Path) -> Result<Self> {
        let mut media = Self::default();
        for ((kind, id), resource) in &project.resources {
            if let Resource::Sprite(sprite) = resource {
                media.load_sprite(root, id, &sprite.file)?;
                let image = &media.sprites[id];
                if sprite
                    .frames
                    .iter()
                    .any(|&frame| !within_image(frame, image))
                {
                    return Err(fail(format!(
                        "{kind}/{id}: sprite frame extends outside {}×{} image",
                        image.width(),
                        image.height()
                    )));
                }
            }
            if let Resource::Audio(audio) = resource {
                let bytes =
                    read_asset(root, &audio.file).map_err(|e| fail(format!("{kind}/{id}: {e}")))?;
                let valid = if audio.file.ends_with(".wav") {
                    bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WAVE")
                } else {
                    bytes.starts_with(b"OggS")
                };
                if !valid {
                    return Err(fail(format!("{kind}/{id}: invalid audio container")));
                }
            }
        }
        Ok(media)
    }
    pub fn load_sprite(&mut self, root: &Path, id: &str, file: &str) -> Result<()> {
        use image::ImageDecoder;
        let data = read_asset(root, file).map_err(|e| fail(format!("sprite/{id}: {e}")))?;
        let mut decoder =
            PngDecoder::new(Cursor::new(data)).map_err(|e| fail(format!("sprite/{id}: {e}")))?;
        let mut limits = image::Limits::default();
        limits.max_image_width = Some(8192);
        limits.max_image_height = Some(8192);
        limits.max_alloc = Some(64 * 1024 * 1024);
        decoder.set_limits(limits)?;
        if decoder.total_bytes() > 64 * 1024 * 1024 {
            return Err(fail(format!("sprite/{id}: decoded image exceeds 64 MiB")));
        }
        self.sprites.insert(
            id.into(),
            image::DynamicImage::from_decoder(decoder)?.into_rgba8(),
        );
        Ok(())
    }

    pub fn frame(
        &self,
        project: &Project,
        sprite: &str,
        frame: usize,
    ) -> Option<([u32; 4], &RgbaImage)> {
        let image = self.sprites.get(sprite)?;
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
    for i in &room.instances {
        if let Some(Resource::Object(object)) = project.get(Kind::Object, &i.1) {
            let bounds = Bounds::new(i.2, i.3, object.size[0], object.size[1]);
            if bounds.overlaps(view) {
                object_draw(project, object, bounds, 0, draw);
            }
        }
    }
}

impl World {
    pub fn draw_scene<'a>(&'a self, view: Bounds, draw: &mut impl FnMut(Draw<'a>)) {
        tiles(&self.project, self.room(), view, draw);
        for entity in self.entities.values() {
            let object = self.object(&entity.object);
            let bounds = entity.bounds(object);
            if !object.hidden && bounds.overlaps(view) {
                object_draw(
                    &self.project,
                    object,
                    bounds,
                    self.snapshot_tick() - entity.born,
                    draw,
                );
            }
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
        for label in &self.room().ui {
            let text = if label.value.is_empty() {
                label.text.clone()
            } else {
                format!(
                    "{}{}",
                    label.text,
                    self.globals.get(&label.value).copied().unwrap_or(0.)
                )
            };
            draw(Draw::Text(label.at, text, label.size, label.color));
        }
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
                    match (map.atlas.as_deref(), tile.frame()) {
                        (Some(atlas), Some(frame)) => {
                            draw(Draw::Sprite(bounds, atlas, frame, tile.color()))
                        }
                        _ => draw(Draw::Rect(bounds, tile.color())),
                    }
                }
            }
        }
    }
}

fn object_draw<'a>(
    project: &'a Project,
    object: &'a Object,
    bounds: Bounds,
    time: u64,
    draw: &mut impl FnMut(Draw<'a>),
) {
    if let Some(Resource::Animation(anim)) = object
        .animation
        .as_ref()
        .and_then(|id| project.get(Kind::Animation, id))
        && !anim.frames.is_empty()
    {
        let frame = (time.saturating_mul(anim.fps as u64) / project.settings.hz.max(1) as u64)
            % anim.frames.len() as u64;
        draw(Draw::Sprite(
            bounds,
            &anim.sprite,
            anim.frames[frame as usize],
            object.color,
        ));
        return;
    }
    if let Some(id) = &object.sprite {
        draw(Draw::Sprite(bounds, id, 0, object.color));
    } else if object.shape == Shape::Circle {
        draw(Draw::Circle(bounds, object.color));
    } else {
        draw(Draw::Rect(bounds, object.color));
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
    let sa = src[3] as u32;
    let da = dest[3] as u32 * (255 - sa) / 255;
    let alpha = sa + da;
    if alpha == 0 {
        return;
    }
    for i in 0..3 {
        dest[i] = ((dest[i] as u32 * da + src[i] as u32 * sa) / alpha) as u8;
    }
    dest[3] = alpha as u8;
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
        Draw::Circle(b, color) => fill(b, &|u, v| {
            ((u - 0.5).powi(2) + (v - 0.5).powi(2) <= 0.25).then_some(color)
        }),
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
