use agge_core::{
    Color, Kind, Light, Project, Resource, Result, Tilemap, fail, read_asset, store::Store,
};
use agge_runtime::{
    Bounds, CompiledGame, Draw, Input, Media, Replay, ReplayPlayback, ReplayRecorder, RoomCollider,
    World, draw_tilemap, shade_lights,
};
use eframe::egui::{
    self, Color32, ColorImage, FontId, Key, Painter, Pos2, Rect, Stroke, StrokeKind, TextureHandle,
    TextureOptions, Ui, Vec2,
};
use std::{
    collections::BTreeMap,
    io::Cursor,
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};

pub fn color([r, g, b, a]: Color) -> Color32 {
    Color32::from_rgba_unmultiplied(r, g, b, a)
}

pub fn options(size: [f32; 2]) -> eframe::NativeOptions {
    eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size(size),
        #[cfg(all(windows, feature = "vulkan"))]
        renderer: eframe::Renderer::Wgpu,
        #[cfg(all(not(windows), feature = "vulkan"))]
        renderer: eframe::Renderer::Glow,
        ..Default::default()
    }
}

pub fn run_player(file: PathBuf) -> Result<()> {
    run_player_in_room(file, None)
}

pub fn run_player_in_room(file: PathBuf, room: Option<&str>) -> Result<()> {
    let store = Store::new(file);
    let project = store.load()?.project;
    let play = Play::new(project, store.root().to_owned(), room)?;
    let title = play.world().project().settings.title.clone();
    let dims = play
        .world()
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
                if let Some(fault) = self.play.world().fault() {
                    egui::Window::new("Game stopped").show(ctx, |ui| {
                        ui.label(fault.to_string());
                    });
                }
            });
    }
}

type KeyBindings = BTreeMap<Key, Input>;

enum Simulation {
    Live(Box<World>),
    Replay(Box<ReplayPlayback<Arc<Replay>>>),
}

impl Simulation {
    fn world(&self) -> &World {
        match self {
            Self::Live(world) => world,
            Self::Replay(replay) => replay.world(),
        }
    }
}

pub struct Play {
    simulation: Simulation,
    pub media: Media,
    pub paused: bool,
    pub audio: AudioOutput,
    pub recording: Option<ReplayRecorder>,
    root: PathBuf,
    last: Option<Instant>,
    elapsed: f64,
    pending: Input,
    bindings: KeyBindings,
    music_id: Option<String>,
}
impl Play {
    pub fn new(project: Project, root: PathBuf, room: Option<&str>) -> Result<Self> {
        let bindings = compile_bindings(&project);
        let world = World::preview(project, room)?;
        Self::start(Simulation::Live(Box::new(world)), root, bindings, false)
    }

    pub fn from_replay(project: Project, root: PathBuf, replay: Arc<Replay>) -> Result<Self> {
        let playback = ReplayPlayback::new(&CompiledGame::new(project)?, replay)?;
        Self::start(
            Simulation::Replay(Box::new(playback)),
            root,
            KeyBindings::new(),
            true,
        )
    }

    fn start(
        simulation: Simulation,
        root: PathBuf,
        bindings: KeyBindings,
        paused: bool,
    ) -> Result<Self> {
        let media = Media::load(simulation.world().project(), &root)?;
        let mut play = Self {
            simulation,
            media,
            paused,
            audio: AudioOutput::default(),
            recording: None,
            root,
            last: None,
            elapsed: 0.,
            pending: Input::new(),
            bindings,
            music_id: None,
        };
        if play.world().fault().is_none() {
            play.audio();
        }
        Ok(play)
    }

    pub fn world(&self) -> &World {
        self.simulation.world()
    }

    pub fn playback(&self) -> Option<&ReplayPlayback<Arc<Replay>>> {
        match &self.simulation {
            Simulation::Live(_) => None,
            Simulation::Replay(replay) => Some(replay),
        }
    }

    pub fn can_advance(&self) -> bool {
        self.world().fault().is_none() && self.playback().is_none_or(|replay| !replay.finished())
    }

    fn audio(&mut self) {
        let (world, sounds) = match &mut self.simulation {
            Simulation::Live(world) => {
                let sounds = world.drain_audio();
                (&**world, sounds)
            }
            Simulation::Replay(replay) => {
                let sounds = replay.drain_audio();
                (replay.world(), sounds)
            }
        };
        if self.music_id != world.room().music {
            self.music_id.clone_from(&world.room().music);
            let music = self.music_id.as_deref().and_then(|id| {
                match world.project().get(Kind::Audio, id) {
                    Some(Resource::Audio(audio)) => Some((audio.file.as_str(), audio.gain)),
                    _ => None,
                }
            });
            self.audio.music(&self.root, music);
            self.audio.pause_music(self.paused);
        }
        for id in sounds {
            if let Some(Resource::Audio(audio)) = world.project().get(Kind::Audio, &id) {
                self.audio.fire(&self.root, &audio.file, audio.gain);
            }
        }
    }
    pub fn step(&mut self, ctx: &egui::Context) {
        if self.can_advance() {
            let (mut input, pressed, _) = self.read_input(ctx);
            input.extend(pressed);
            self.advance(&input);
        }
    }
    fn advance(&mut self, input: &Input) {
        let ok = match &mut self.simulation {
            Simulation::Live(world) => {
                if let Some(recording) = &mut self.recording {
                    recording.record(input);
                }
                world.tick(input).is_ok()
            }
            Simulation::Replay(replay) => {
                replay.step();
                replay.report().fault.is_none()
            }
        };
        if ok {
            self.audio();
        }
    }

    fn read_input(&self, ctx: &egui::Context) -> (Input, Input, bool) {
        if self.playback().is_some() {
            (Input::new(), Input::new(), false)
        } else {
            actions(ctx, &self.bindings)
        }
    }
    pub fn update(&mut self, ctx: &egui::Context) {
        self.update_at(ctx, Instant::now());
    }
    fn update_at(&mut self, ctx: &egui::Context, now: Instant) {
        let can_advance = self.can_advance();
        self.audio.pause_music(self.paused || !can_advance);
        if !can_advance {
            return;
        }
        if self.paused {
            self.last = None;
            self.elapsed = 0.;
            self.pending.clear();
            return;
        }
        let dt = 1. / self.world().project().settings.hz as f64;
        if let Some(last) = self.last.replace(now) {
            self.elapsed += (now - last).as_secs_f64().min(0.25);
        }
        let (held, pressed, reset_pending) = self.read_input(ctx);
        if reset_pending {
            self.pending.clear();
        }
        self.pending.extend(pressed);
        for _ in 0..12 {
            if self.elapsed < dt {
                break;
            }
            self.elapsed -= dt;
            let mut input = held.clone();
            input.append(&mut self.pending);
            self.advance(&input);
            if !self.can_advance() {
                self.audio.pause_music(true);
                return;
            }
        }
        self.elapsed = self.elapsed.min(dt);
        ctx.request_repaint_after(Duration::from_secs_f64((dt - self.elapsed).max(0.001)));
    }
}

#[derive(Default)]
pub struct AudioOutput {
    stream: Option<rodio::OutputStream>,
    clips: BTreeMap<String, Arc<[u8]>>,
    playing: Vec<rodio::Sink>,
    music: Option<rodio::Sink>,
    music_error: Option<String>,
    pub error: Option<String>,
}
impl AudioOutput {
    pub fn problem(&self) -> Option<&str> {
        self.music_error.as_deref().or(self.error.as_deref())
    }

    fn pause_music(&self, pause: bool) {
        if let Some(music) = &self.music
            && music.is_paused() != pause
        {
            if pause {
                music.pause();
            } else {
                music.play();
            }
        }
    }

    fn music(&mut self, root: &Path, request: Option<(&str, f32)>) {
        self.music = None;
        self.music_error = None;
        if let Some((file, gain)) = request {
            match self.load_music(root, file, gain) {
                Ok(sink) => self.music = Some(sink),
                Err(error) => self.music_error = Some(format!("Music: {error}")),
            }
        }
    }

    fn load_music(&mut self, root: &Path, file: &str, gain: f32) -> Result<rodio::Sink> {
        let source = rodio::Decoder::new_looped(Cursor::new(self.clip(root, file)?))?;
        let sink = self.sink(gain)?;
        sink.append(source);
        Ok(sink)
    }
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
        let source = rodio::Decoder::new(Cursor::new(self.clip(root, file)?))?;
        let sink = self.sink(gain)?;
        sink.append(source);
        self.playing.push(sink);
        self.error = None;
        Ok(())
    }
    fn sink(&mut self, gain: f32) -> Result<rodio::Sink> {
        if self.stream.is_none() {
            self.stream = Some(rodio::OutputStreamBuilder::open_default_stream()?);
        }
        let sink =
            rodio::Sink::connect_new(self.stream.as_ref().expect("audio stream opened").mixer());
        sink.set_volume(gain);
        Ok(sink)
    }
    fn clip(&mut self, root: &Path, file: &str) -> Result<Arc<[u8]>> {
        if !self.clips.contains_key(file) {
            self.clips
                .insert(file.into(), read_asset(root, file)?.into());
        }
        Ok(Arc::clone(&self.clips[file]))
    }
}

fn compile_bindings(project: &Project) -> KeyBindings {
    let mut bindings = KeyBindings::new();
    for (action, keys) in [
        ("left", [Key::ArrowLeft, Key::A]),
        ("right", [Key::ArrowRight, Key::D]),
        ("up", [Key::ArrowUp, Key::W]),
        ("down", [Key::ArrowDown, Key::S]),
        ("fire", [Key::Space, Key::Z]),
        ("confirm", [Key::Enter, Key::X]),
        ("cancel", [Key::Escape, Key::Backspace]),
    ] {
        if !project.settings.bindings.contains_key(action) {
            for key in keys {
                bindings.entry(key).or_default().insert(action.into());
            }
        }
    }
    for (action, names) in &project.settings.bindings {
        for &key in Key::ALL {
            if names
                .iter()
                .any(|name| name.eq_ignore_ascii_case(key.name()))
            {
                bindings.entry(key).or_default().insert(action.clone());
            }
        }
    }
    bindings
}

fn actions(ctx: &egui::Context, bindings: &KeyBindings) -> (Input, Input, bool) {
    ctx.input(|input| {
        if !input.focused {
            return (Input::new(), Input::new(), true);
        }
        let focus_loss = input
            .events
            .iter()
            .rposition(|event| matches!(event, egui::Event::WindowFocused(false)));
        let events = &input.events[focus_loss.map_or(0, |index| index + 1)..];
        let pressed = events
            .iter()
            .filter_map(|event| match event {
                egui::Event::Key {
                    key,
                    pressed: true,
                    repeat: false,
                    ..
                } => bindings.get(key),
                _ => None,
            })
            .flatten()
            .cloned()
            .collect();
        let held = input
            .keys_down
            .iter()
            .filter_map(|key| bindings.get(key))
            .flatten()
            .cloned()
            .collect();
        (held, pressed, focus_loss.is_some())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_replay_starts_paused_and_stops_at_the_last_recorded_tick() {
        let project = Project::parse(
            r#"agge/1 {}
room main {"instances":[["hero","hero",0,0]]}
object hero {"script":"drive"}
script drive
step:
  move down("fire"), 0
---
"#,
        )
        .unwrap();
        let spec = serde_json::from_str(
            r#"{"ticks":2,"input":[[1,2,"fire"]],"expect":[{"at":2,"field":"entity/hero/x","is":2}]}"#,
        ).unwrap();
        let mut play = Play::from_replay(project, PathBuf::new(), Arc::new(spec)).unwrap();
        let ctx = egui::Context::default();
        assert!(play.paused);
        play.update(&ctx);
        assert_eq!(play.playback().unwrap().report().ticks, 0);
        for _ in 0..3 {
            play.step(&ctx);
        }
        assert!(!play.can_advance());
        let report = play.playback().unwrap().report();
        assert_eq!(report.ticks, 2);
        assert_eq!(report.assertions, 1);
        assert!(report.passed(), "{report:?}");
    }

    #[test]
    fn compiled_bindings_preserve_defaults_and_case_insensitive_overrides() {
        let mut project = Project::default();
        project.settings.bindings = BTreeMap::from([
            ("left".into(), vec!["sPaCe".into()]),
            ("jump".into(), vec!["Space".into(), "unknown".into()]),
            ("cancel".into(), vec![]),
        ]);
        let bindings = compile_bindings(&project);
        assert_eq!(
            bindings[&Key::Space],
            Input::from(["fire".into(), "jump".into(), "left".into()])
        );
        assert_eq!(bindings[&Key::ArrowRight], Input::from(["right".into()]));
        for key in [Key::A, Key::ArrowLeft, Key::Escape, Key::Backspace] {
            assert!(!bindings.contains_key(&key));
        }
    }

    #[test]
    fn play_drops_buffered_input_when_the_window_loses_focus() {
        let mut play = Play::new(Project::default(), PathBuf::new(), None).unwrap();
        play.recording = Some(ReplayRecorder::new("main"));
        let ctx = egui::Context::default();
        let start = Instant::now();
        let update = |play: &mut Play, time, events| {
            let _ = ctx.run(
                egui::RawInput {
                    events,
                    focused: true,
                    ..Default::default()
                },
                |ctx| play.update_at(ctx, start + Duration::from_millis(time)),
            );
        };
        let press = |key| egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        };

        update(&mut play, 0, vec![press(Key::Space)]);
        assert!(play.pending.contains("fire"));
        update(
            &mut play,
            20,
            vec![
                press(Key::ArrowLeft),
                egui::Event::WindowFocused(false),
                egui::Event::WindowFocused(true),
                press(Key::ArrowRight),
            ],
        );
        let replay = play.recording.as_ref().unwrap().replay();
        assert_eq!(replay.ticks, 1);
        assert_eq!(replay.input.len(), 1);
        assert_eq!(replay.input[0].2, "right");
    }

    #[test]
    fn play_clock_does_not_catch_up_after_startup_or_pause() {
        let mut play = Play::new(Project::default(), PathBuf::new(), None).unwrap();
        let ctx = egui::Context::default();
        let first_frame = Instant::now() + Duration::from_secs(2);
        let frame_time = Duration::from_millis(20);
        play.update_at(&ctx, first_frame);
        assert_eq!(play.world().snapshot(None, None).tick, 0);
        play.update_at(&ctx, first_frame + frame_time);
        assert_eq!(play.world().snapshot(None, None).tick, 1);

        play.paused = true;
        play.update_at(&ctx, first_frame + Duration::from_secs(1));
        play.paused = false;
        let resume = first_frame + Duration::from_secs(3);
        play.update_at(&ctx, resume);
        assert_eq!(play.world().snapshot(None, None).tick, 1);
        play.update_at(&ctx, resume + frame_time);
        assert_eq!(play.world().snapshot(None, None).tick, 2);
    }

    #[test]
    fn cinder_music_decoder_repeats_from_the_first_sample() {
        let wav = include_bytes!("../../../examples/cinder/audio/cinder.wav");
        let source = rodio::Decoder::new_looped(Cursor::new(wav.as_slice())).unwrap();
        let samples = source.take(129_024).collect::<Vec<_>>();
        assert_eq!(samples.len(), 129_024);
        assert_eq!(samples[..1024], samples[128_000..]);
        assert!(samples[..1024].iter().any(|&value| value != 0.));
    }
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
        self.play_with_colliders(ui, play, false);
    }

    pub fn play_with_colliders(&mut self, ui: &mut Ui, play: &Play, show_colliders: bool) {
        let size = play.world().project().settings.size;
        let screen = fit(ui.available_rect_before_wrap(), size);
        let painter = ui.painter_at(screen);
        painter.rect_filled(screen, 0., color(play.world().room().bg));
        let scale = screen.width() / size[0] as f32;
        let [x, y] = play.world().camera();
        let world_origin = screen.min - Vec2::new(x, y) * scale;
        let mut draw = |shape| {
            self.paint(
                ui.ctx(),
                &painter,
                world_origin,
                scale,
                &play.media,
                play.world().project(),
                shape,
            )
        };
        play.world().draw_scene(play.world().view(), &mut draw);
        self.lighting(&painter, screen, play.world());
        if show_colliders {
            play.world().visit_colliders(|collider| {
                paint_collider(&painter, world_origin, scale, collider);
            });
        }
        let mut draw = |shape| {
            self.paint(
                ui.ctx(),
                &painter,
                screen.min,
                scale,
                &play.media,
                play.world().project(),
                shape,
            )
        };
        play.world().draw_ui(&mut draw);
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

pub fn paint_collider(painter: &Painter, origin: Pos2, scale: f32, collider: RoomCollider) {
    let to_screen = |x, y| origin + Vec2::new(x, y) * scale;
    match collider {
        RoomCollider::Solid(bounds) => {
            painter.rect_stroke(
                Rect::from_min_max(
                    to_screen(bounds.left, bounds.top),
                    to_screen(bounds.right, bounds.bottom),
                ),
                0.,
                Stroke::new(1.0_f32, Color32::from_rgb(255, 174, 101)),
                StrokeKind::Inside,
            );
        }
        RoomCollider::OneWay(bounds) => {
            painter.line_segment(
                [
                    to_screen(bounds.left, bounds.top),
                    to_screen(bounds.right, bounds.top),
                ],
                Stroke::new(2.0_f32, Color32::from_rgb(115, 229, 241)),
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
