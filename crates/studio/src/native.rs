use agge_core::{
    Color, Kind, Light, Project, Resource, Result, Tilemap, fail, read_asset, store::Store,
};
use agge_runtime::{Bounds, Draw, Input, Media, World, draw_tilemap, shade_lights};
use eframe::egui::{
    self, Color32, ColorImage, FontId, Key, Painter, Pos2, Rect, TextureHandle, TextureOptions, Ui,
    Vec2,
};
use std::{
    collections::BTreeMap,
    io::Cursor,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

pub fn color([r, g, b, a]: Color) -> Color32 {
    Color32::from_rgba_unmultiplied(r, g, b, a)
}

pub fn options(size: [f32; 2]) -> eframe::NativeOptions {
    eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size(size),
        ..Default::default()
    }
}

pub fn run_player(file: PathBuf) -> Result<()> {
    let store = Store::new(file);
    let project = store.load()?.project;
    let play = Play::new(project, store.root().to_owned(), None)?;
    let title = play.world.project().settings.title.clone();
    let dims = play
        .world
        .project()
        .settings
        .size
        .map(|n| (n as f32).clamp(280., 1200.));
    eframe::run_native(
        &title,
        options(dims),
        Box::new(move |_| {
            Ok(Box::new(Player {
                play,
                canvas: Canvas::default(),
            }))
        }),
    )
    .map_err(|e| fail(e.to_string()))
}

struct Player {
    play: Play,
    canvas: Canvas,
}
impl eframe::App for Player {
    fn update(&mut self, ctx: &egui::Context, _: &mut eframe::Frame) {
        self.play.update(ctx);
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(Color32::BLACK))
            .show(ctx, |ui| {
                self.canvas.play(ui, &self.play);
                if let Some(fault) = self.play.world.fault() {
                    egui::Window::new("Game stopped").show(ctx, |ui| {
                        ui.label(fault.to_string());
                    });
                }
            });
    }
}

pub struct Play {
    pub world: World,
    pub media: Media,
    pub paused: bool,
    pub audio: AudioOutput,
    root: PathBuf,
    last: Instant,
    elapsed: f64,
    pending: Input,
}
impl Play {
    pub fn new(project: Project, root: PathBuf, room: Option<&str>) -> Result<Self> {
        let media = Media::load(&project, &root)?;
        let world = World::preview(project, room)?;
        let mut play = Self {
            world,
            media,
            paused: false,
            audio: AudioOutput::default(),
            root,
            last: Instant::now(),
            elapsed: 0.,
            pending: Input::new(),
        };
        if play.world.fault().is_none() {
            play.audio();
        }
        Ok(play)
    }
    fn audio(&mut self) {
        for id in self.world.drain_audio() {
            if let Some(Resource::Audio(audio)) = self.world.project().get(Kind::Audio, &id) {
                self.audio.fire(&self.root, &audio.file, audio.gain);
            }
        }
    }
    pub fn step(&mut self, ctx: &egui::Context) {
        if self.world.fault().is_none() {
            let (mut input, pressed) = actions(ctx, self.world.project());
            input.extend(pressed);
            self.advance(&input);
        }
    }
    fn advance(&mut self, input: &Input) {
        if self.world.tick(input).is_ok() {
            self.audio();
        }
    }
    pub fn update(&mut self, ctx: &egui::Context) {
        if self.world.fault().is_some() {
            return;
        }
        let now = Instant::now();
        if self.paused {
            self.last = now;
            self.elapsed = 0.;
            self.pending.clear();
            return;
        }
        let dt = 1. / self.world.project().settings.hz as f64;
        self.elapsed += (now - self.last).as_secs_f64().min(0.25);
        self.last = now;
        let (held, pressed) = actions(ctx, self.world.project());
        self.pending.extend(pressed);
        for _ in 0..12 {
            if self.elapsed < dt {
                break;
            }
            self.elapsed -= dt;
            let mut input = held.clone();
            input.append(&mut self.pending);
            self.advance(&input);
            if self.world.fault().is_some() {
                break;
            }
        }
        self.elapsed = self.elapsed.min(dt);
        ctx.request_repaint_after(Duration::from_secs_f64((dt - self.elapsed).max(0.001)));
    }
}

#[derive(Default)]
pub struct AudioOutput {
    stream: Option<rodio::OutputStream>,
    clips: BTreeMap<String, Vec<u8>>,
    playing: Vec<rodio::Sink>,
    pub error: Option<String>,
}
impl AudioOutput {
    pub fn fire(&mut self, root: &Path, file: &str, gain: f32) {
        self.playing.retain(|sink| !sink.empty());
        if self.playing.len() >= 64 {
            return;
        }
        if let Err(error) = self.play(root, file, gain) {
            self.error = Some(error.to_string());
        }
    }
    fn play(&mut self, root: &Path, file: &str, gain: f32) -> Result<()> {
        if self.stream.is_none() {
            self.stream = Some(rodio::OutputStreamBuilder::open_default_stream()?);
        }
        if !self.clips.contains_key(file) {
            self.clips.insert(file.into(), read_asset(root, file)?);
        }
        let bytes = self.clips.get(file).expect("loaded audio");
        let source = rodio::Decoder::new(Cursor::new(bytes.clone()))?;
        let sink =
            rodio::Sink::connect_new(self.stream.as_ref().expect("audio stream opened").mixer());
        sink.set_volume(gain);
        sink.append(source);
        self.playing.push(sink);
        self.error = None;
        Ok(())
    }
}

fn actions(ctx: &egui::Context, project: &Project) -> (Input, Input) {
    let mut held = Input::new();
    let mut pressed = Input::new();
    ctx.input(|input| {
        let just_pressed = input
            .events
            .iter()
            .filter_map(|event| match event {
                egui::Event::Key {
                    key,
                    pressed: true,
                    repeat: false,
                    ..
                } => Some(*key),
                _ => None,
            })
            .collect::<Vec<_>>();
        let mut map_action = |action: &str, matches: &dyn Fn(Key) -> bool| {
            if input.keys_down.iter().any(|key| matches(*key)) {
                held.insert(action.into());
            }
            if just_pressed.iter().any(|key| matches(*key)) {
                pressed.insert(action.into());
            }
        };
        for (name, keys) in [
            ("left", [Key::ArrowLeft, Key::A]),
            ("right", [Key::ArrowRight, Key::D]),
            ("up", [Key::ArrowUp, Key::W]),
            ("down", [Key::ArrowDown, Key::S]),
            ("fire", [Key::Space, Key::Z]),
            ("confirm", [Key::Enter, Key::X]),
            ("cancel", [Key::Escape, Key::Backspace]),
        ] {
            if !project.settings.bindings.contains_key(name) {
                map_action(name, &|key| keys.contains(&key));
            }
        }
        for (action, keys) in &project.settings.bindings {
            map_action(action, &|key| {
                keys.iter()
                    .any(|binding| binding.eq_ignore_ascii_case(key.name()))
            });
        }
    });
    (held, pressed)
}
#[derive(Default)]
pub struct Canvas {
    textures: BTreeMap<String, TextureHandle>,
    lighting: Option<Lightmap>,
}

struct Lightmap {
    darkness: f32,
    lights: Vec<Light>,
    viewport: [u32; 2],
    region: Rect,
    texture: TextureHandle,
}

impl Canvas {
    pub fn play(&mut self, ui: &mut Ui, play: &Play) {
        let size = play.world.project().settings.size;
        let screen = fit(ui.available_rect_before_wrap(), size);
        let painter = ui.painter_at(screen);
        painter.rect_filled(screen, 0., color(play.world.room().bg));
        let scale = screen.width() / size[0] as f32;
        let [x, y] = play.world.camera();
        let world_origin = screen.min - Vec2::new(x, y) * scale;
        let mut draw = |shape| {
            self.paint(
                ui.ctx(),
                &painter,
                world_origin,
                scale,
                &play.media,
                play.world.project(),
                shape,
            )
        };
        play.world.draw_scene(play.world.view(), &mut draw);
        self.lighting(&painter, screen, &play.world);
        let mut draw = |shape| {
            self.paint(
                ui.ctx(),
                &painter,
                screen.min,
                scale,
                &play.media,
                play.world.project(),
                shape,
            )
        };
        play.world.draw_ui(&mut draw);
        ui.allocate_rect(screen, egui::Sense::hover());
    }
    fn sprite_texture(
        &mut self,
        ctx: &egui::Context,
        media: &Media,
        id: &str,
    ) -> Option<egui::TextureId> {
        if let Some(texture) = self.textures.get(id) {
            return Some(texture.id());
        }
        let image = media.sprites.get(id)?;
        let texture = ctx.load_texture(
            id,
            ColorImage::from_rgba_unmultiplied(
                [image.width() as usize, image.height() as usize],
                image.as_raw(),
            ),
            TextureOptions::NEAREST,
        );
        let texture_id = texture.id();
        self.textures.insert(id.into(), texture);
        Some(texture_id)
    }

    pub fn sheet(&mut self, painter: &Painter, rect: Rect, media: &Media, id: &str) {
        if let Some(texture) = self.sprite_texture(painter.ctx(), media, id) {
            painter.image(
                texture,
                rect,
                Rect::from_min_max(Pos2::ZERO, Pos2::new(1., 1.)),
                Color32::WHITE,
            );
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn paint(
        &mut self,
        ctx: &egui::Context,
        painter: &Painter,
        origin: Pos2,
        scale: f32,
        media: &Media,
        project: &Project,
        draw: Draw<'_>,
    ) {
        let rect = |b: Bounds| {
            Rect::from_min_max(
                origin + Vec2::new(b.left, b.top) * scale,
                origin + Vec2::new(b.right, b.bottom) * scale,
            )
        };
        match draw {
            Draw::TileLayer(map, view) => {
                self.paint_tiles(painter, media, project, map, view, rect)
            }
            Draw::Rect(b, tint) => {
                painter.rect_filled(rect(b), 0., color(tint));
            }
            Draw::Circle(b, tint) => {
                let r = rect(b);
                painter.add(egui::Shape::ellipse_filled(
                    r.center(),
                    r.size() * 0.5,
                    color(tint),
                ));
            }
            Draw::Sprite(b, id, frame, tint) => {
                if let Some(([x, y, w, h], image)) = media.frame(project, id, frame)
                    && let Some(texture) = self.sprite_texture(ctx, media, id)
                {
                    let width = image.width() as f32;
                    let height = image.height() as f32;
                    let uv = Rect::from_min_max(
                        Pos2::new(x as f32 / width, y as f32 / height),
                        Pos2::new((x + w) as f32 / width, (y + h) as f32 / height),
                    );
                    painter.image(texture, rect(b), uv, color(tint));
                } else {
                    painter.rect_filled(rect(b), 0., color(tint));
                }
            }
            Draw::Text([x, y], text, size, tint) => {
                painter.text(
                    origin + Vec2::new(x, y) * scale,
                    egui::Align2::LEFT_TOP,
                    text,
                    FontId::monospace(size as f32 * scale),
                    color(tint),
                );
            }
        }
    }
    fn paint_tiles(
        &mut self,
        painter: &Painter,
        media: &Media,
        project: &Project,
        map: &Tilemap,
        view: Bounds,
        screen_rect: impl Fn(Bounds) -> Rect,
    ) {
        let atlas = map
            .atlas
            .as_deref()
            .and_then(|id| self.sprite_texture(painter.ctx(), media, id));
        let mut flat = egui::Mesh::default();
        let mut images = atlas.map(egui::Mesh::with_texture);
        let mut regions = BTreeMap::new();
        draw_tilemap(map, view, &mut |tile| match tile {
            Draw::Rect(bounds, tint) => flat.add_colored_rect(screen_rect(bounds), color(tint)),
            Draw::Sprite(bounds, id, frame, tint) => {
                let uv = regions.entry(frame).or_insert_with(|| {
                    media
                        .frame(project, id, frame)
                        .map(|([x, y, w, h], image)| {
                            Rect::from_min_max(
                                Pos2::new(
                                    x as f32 / image.width() as f32,
                                    y as f32 / image.height() as f32,
                                ),
                                Pos2::new(
                                    (x + w) as f32 / image.width() as f32,
                                    (y + h) as f32 / image.height() as f32,
                                ),
                            )
                        })
                });
                if let (Some(mesh), Some(uv)) = (&mut images, uv) {
                    mesh.add_rect_with_uv(screen_rect(bounds), *uv, color(tint));
                } else {
                    flat.add_colored_rect(screen_rect(bounds), color(tint));
                }
            }
            _ => unreachable!("tile palette emits only sprites and flat tiles"),
        });
        if !flat.is_empty() {
            painter.add(egui::Shape::mesh(flat));
        }
        if let Some(images) = images.filter(|mesh| !mesh.is_empty()) {
            painter.add(egui::Shape::mesh(images));
        }
    }

    fn lighting(&mut self, painter: &Painter, screen: Rect, world: &World) {
        let room = world.room();
        if room.darkness <= 0. {
            return;
        }
        if room.lights.is_empty() {
            painter.rect_filled(
                screen,
                0.,
                Color32::from_black_alpha((room.darkness * 255.) as u8),
            );
            return;
        }
        let size = world.project().settings.size;
        let bounds = world.view();
        let view = Rect::from_min_max(
            Pos2::new(bounds.left, bounds.top),
            Pos2::new(bounds.right, bounds.bottom),
        );
        if self.lighting.as_ref().is_none_or(|cache| {
            cache.viewport != size
                || cache.darkness != room.darkness
                || cache.lights != room.lights
                || !cache.region.contains_rect(view)
        }) {
            let [w, h] = size.map(|n| (n as usize * 2).clamp(4, 384));
            let extent = view.size() * 2.;
            let step = extent / Vec2::new(w as f32, h as f32);
            let padded = view.min - view.size() / 2.;
            let origin = Pos2::new(
                (padded.x / step.x).floor() * step.x,
                (padded.y / step.y).floor() * step.y,
            );
            let region = Rect::from_min_size(origin, extent);
            let nearby = room
                .lights
                .iter()
                .filter(|light| {
                    let [x, y] = light.at;
                    Rect::from_center_size(Pos2::new(x, y), Vec2::splat(light.radius * 2.))
                        .intersects(region)
                })
                .collect::<Vec<_>>();
            let mut pixels = Vec::with_capacity(w * h * 4);
            for y in 0..h {
                for x in 0..w {
                    let at = [
                        region.left() + (x as f32 + 0.5) * region.width() / w as f32,
                        region.top() + (y as f32 + 0.5) * region.height() / h as f32,
                    ];
                    pixels.extend(shade_lights(room.darkness, nearby.iter().copied(), at));
                }
            }
            let texture = painter.ctx().load_texture(
                "room lighting",
                ColorImage::from_rgba_unmultiplied([w, h], &pixels),
                TextureOptions::LINEAR,
            );
            self.lighting = Some(Lightmap {
                darkness: room.darkness,
                lights: room.lights.clone(),
                viewport: size,
                region,
                texture,
            });
        }
        if let Some(cache) = &self.lighting {
            let uv = |point: Pos2| {
                let offset = point - cache.region.min;
                Pos2::new(
                    offset.x / cache.region.width(),
                    offset.y / cache.region.height(),
                )
            };
            painter.image(
                cache.texture.id(),
                screen,
                Rect::from_min_max(uv(view.min), uv(view.max)),
                Color32::WHITE,
            );
        }
    }
}
pub fn fit(area: Rect, size: [u32; 2]) -> Rect {
    let dims = Vec2::new(size[0] as f32, size[1] as f32);
    let scale = (area.width() / dims.x)
        .min(area.height() / dims.y)
        .max(f32::EPSILON);
    Rect::from_center_size(area.center(), dims * scale)
}
