use crate::panels;
use agge_core::{
    Index, Kind, Project, Resource, Result, Settings, fail,
    store::{Loaded, Store},
    valid_id,
};
use agge_runtime::Media;
use agge_studio::{AudioOutput, Canvas, Play};
use eframe::egui::{self, Color32, Context, RichText, Ui};
use std::path::PathBuf;

pub const MINT: Color32 = Color32::from_rgb(149, 234, 210);
pub const MUTED: Color32 = Color32::from_rgb(153, 165, 185);

pub struct Studio {
    file: String,
    work: Option<Workbench>,
    message: String,
}
impl Studio {
    pub fn new(file: Option<PathBuf>) -> Self {
        let mut studio = Self {
            file: "game.agge".into(),
            work: None,
            message: String::new(),
        };
        if let Some(file) = file {
            studio.file = file.to_string_lossy().into_owned();
            studio.open(false);
        }
        studio
    }
    fn open(&mut self, create: bool) {
        let result = (|| -> Result<Workbench> {
            let store = Store::new(self.file.trim());
            if create {
                store.init(&Project::default())?;
            }
            Workbench::open(store)
        })();
        match result {
            Ok(work) => {
                self.work = Some(work);
                self.message.clear();
            }
            Err(error) => self.message = error.to_string(),
        }
    }
}
impl eframe::App for Studio {
    fn update(&mut self, ctx: &Context, _: &mut eframe::Frame) {
        if let Some(workbench) = &mut self.work {
            workbench.ui(ctx);
            return;
        }
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space((ui.available_height() * 0.27).max(16.));
                ui.label(RichText::new("AGGE Studio").size(44.).color(MINT).strong());
                ui.label(
                    RichText::new("Artificial General Game Engine")
                        .size(16.)
                        .color(MUTED),
                );
                ui.add_space(20.);
                ui.add(
                    egui::TextEdit::singleline(&mut self.file)
                        .hint_text("Path to .agge project")
                        .desired_width(380.),
                );
                if ui.button("Open project").clicked() {
                    self.open(false);
                }
                if ui.button("Create project").clicked() {
                    self.open(true);
                }
                if !self.message.is_empty() {
                    ui.colored_label(Color32::LIGHT_RED, &self.message);
                }
            });
        });
    }
}

pub struct Draft {
    pub kind: Kind,
    pub id: String,
    pub raw: String,
    pub saved: Option<String>,
}
impl Draft {
    pub fn dirty(&self) -> bool {
        self.saved.as_ref() != Some(&self.raw)
    }
    pub fn resource(&self) -> Result<Resource> {
        Resource::parse(self.kind, &self.raw)
    }
}
pub struct Workbench {
    pub store: Store,
    pub loaded: Loaded,
    pub index: Index,
    pub draft: Option<Draft>,
    pub media: Media,
    pub canvas: Canvas,
    pub play_canvas: Canvas,
    pub audio: AudioOutput,
    pub tool: panels::Tools,
    pub raw: bool,
    pub status: String,
    pub bad: bool,
    pub play: Option<Play>,
    filter: String,
    create: Option<(Kind, String)>,
    rename: Option<String>,
    settings: Option<Settings>,
    inspect: bool,
    jump_line: Option<usize>,
}
impl Workbench {
    fn open(store: Store) -> Result<Self> {
        let loaded = store.load()?;
        let index = Index::build(&loaded.project);
        let (media, status, bad) = match Media::load(&loaded.project, store.root()) {
            Ok(media) => (media, "Ready".into(), false),
            Err(error) => (Media::default(), error.to_string(), true),
        };
        let mut this = Self {
            store,
            loaded,
            index,
            draft: None,
            media,
            canvas: Canvas::default(),
            play_canvas: Canvas::default(),
            audio: AudioOutput::default(),
            tool: panels::Tools::default(),
            raw: false,
            status,
            bad,
            play: None,
            filter: String::new(),
            create: None,
            rename: None,
            settings: None,
            inspect: false,
            jump_line: None,
        };
        let start = this.loaded.project.settings.start.clone();
        this.select(Kind::Room, &start);
        Ok(this)
    }
    fn select(&mut self, kind: Kind, id: &str) {
        if self
            .draft
            .as_ref()
            .is_some_and(|d| d.kind == kind && d.id == id)
        {
            return;
        }
        if self.dirty() {
            self.note("Apply or discard the current changes first", true);
            return;
        }
        if let Some(resource) = self.loaded.project.get(kind, id) {
            let raw = resource.encode(true);
            self.draft = Some(Draft {
                kind,
                id: id.into(),
                saved: Some(raw.clone()),
                raw,
            });
            self.raw = false;
            self.tool.reset();
            self.rename = None;
            self.jump_line = None;
        }
    }
    fn dirty(&self) -> bool {
        self.draft.as_ref().is_some_and(Draft::dirty)
    }
    pub fn note(&mut self, message: impl Into<String>, bad: bool) {
        self.status = message.into();
        self.bad = bad;
    }
    fn reload(&mut self) -> Result<()> {
        self.loaded = self.store.load()?;
        self.index = Index::build(&self.loaded.project);
        self.canvas = Canvas::default();
        self.tool.refresh_media();
        self.media = Media::load(&self.loaded.project, self.store.root()).unwrap_or_else(|error| {
            self.note(error.to_string(), true);
            Media::default()
        });
        self.audio = AudioOutput::default();
        let key = self.draft.take().map(|d| (d.kind, d.id));
        if let Some((kind, id)) = key
            && let Some(resource) = self.loaded.project.get(kind, &id)
        {
            let raw = resource.encode(true);
            self.draft = Some(Draft {
                kind,
                id,
                saved: Some(raw.clone()),
                raw,
            });
        }
        if self.draft.is_none() {
            let start = self.loaded.project.settings.start.clone();
            self.select(Kind::Room, &start);
        }
        Ok(())
    }
    fn save(&mut self) -> Result<()> {
        let Some(draft) = &self.draft else {
            return Ok(());
        };
        if !draft.dirty() {
            return Ok(());
        }
        let resource = draft.resource()?;
        let id = draft.id.clone();
        if draft.saved.is_none() && self.loaded.project.get(draft.kind, &id).is_some() {
            return Err(fail("resource already exists"));
        }
        let receipt = self.store.update(Some(&self.loaded.revision), |project| {
            project.put(id, resource);
            Ok(())
        })?;
        self.note(format!("Saved {}", &receipt.revision[..8]), false);
        self.reload()
    }
    fn history(&mut self, redo: bool) -> Result<()> {
        if self.dirty() {
            return Err(fail("apply or discard changes first"));
        }
        self.store.travel(Some(&self.loaded.revision), redo)?;
        self.note(
            if redo {
                "Redid transaction"
            } else {
                "Undid transaction"
            },
            false,
        );
        self.reload()
    }
    fn rename(&mut self, to: String) -> Result<()> {
        let Some(draft) = &self.draft else {
            return Ok(());
        };
        if draft.dirty() {
            return Err(fail("apply or discard changes first"));
        }
        self.store.transact(
            Some(&self.loaded.revision),
            vec![agge_core::Edit::Rename {
                kind: draft.kind,
                id: draft.id.clone(),
                to: to.clone(),
            }],
        )?;
        self.draft.as_mut().expect("selected resource").id = to;
        self.note("Renamed resource and updated references", false);
        self.reload()
    }
    fn remove(&mut self) -> Result<()> {
        let Some(draft) = &self.draft else {
            return Ok(());
        };
        if draft.dirty() {
            return Err(fail("apply or discard changes first"));
        }
        self.store.transact(
            Some(&self.loaded.revision),
            vec![agge_core::Edit::Remove {
                kind: draft.kind,
                id: draft.id.clone(),
            }],
        )?;
        self.note("Deleted resource", false);
        self.reload()
    }
    fn play(&mut self) -> Result<()> {
        self.save()?;
        let room = self
            .draft
            .as_ref()
            .filter(|draft| draft.kind == Kind::Room)
            .map(|d| d.id.as_str());
        self.play = Some(Play::new(
            self.loaded.project.clone(),
            self.store.root().to_owned(),
            room,
        )?);
        self.play_canvas = Canvas::default();
        Ok(())
    }
    fn command(&mut self, command: impl FnOnce(&mut Self) -> Result<()>) {
        if let Err(error) = command(self) {
            self.note(error.to_string(), true);
        }
    }

    fn new_resource(&mut self, kind: Kind, id: String) -> Result<()> {
        if !valid_id(&id) || self.loaded.project.get(kind, &id).is_some() {
            return Err(fail("enter a unique lowercase ID"));
        }
        let default = match kind {
            Kind::Room | Kind::Object => "{}",
            Kind::Script => "step:\n  # Add behavior here",
            Kind::Sprite => r#"{"file":"art/sprite.png"}"#,
            Kind::Animation => r#"{"sprite":"sprite","frames":[0],"fps":8}"#,
            Kind::Tilemap => {
                r##"{"cell":16,"tiles":{"#":[[72,101,122,255],true]},"rows":["........","........","........","........","........","........","........","........"]}"##
            }
            Kind::Audio => r#"{"file":"audio/sound.wav"}"#,
            Kind::Particle => {
                r#"{"count":16,"life":30,"speed":90,"radius":2,"color":[140,230,190,255]}"#
            }
        };
        let resource = Resource::parse(kind, default)?;
        self.draft = Some(Draft {
            kind,
            id,
            raw: resource.encode(true),
            saved: None,
        });
        self.raw = false;
        self.tool.reset();
        self.rename = None;
        Ok(())
    }
    fn header(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.label(RichText::new("AGGE").size(24.).strong().color(MINT));
            ui.separator();
            ui.label(
                RichText::new(&self.loaded.project.settings.title)
                    .size(16.)
                    .strong(),
            );
            ui.add_space(12.);
            if let Some(play) = &mut self.play {
                if ui.button("■ Stop").clicked() {
                    self.play = None;
                } else {
                    let fault = play.world.fault();
                    if let Some(fault) = fault {
                        if let Some((script, line)) = fault.location()
                            && ui.button("Open fault source").clicked()
                        {
                            let script = script.to_owned();
                            let message = fault.to_string();
                            self.play = None;
                            self.select(Kind::Script, &script);
                            self.jump_line = Some(line);
                            self.note(message, true);
                            return;
                        }
                    } else {
                        if ui
                            .button(if play.paused { "Resume" } else { "Pause" })
                            .clicked()
                        {
                            play.paused = !play.paused;
                        }
                        if ui
                            .add_enabled(play.paused, egui::Button::new("Step"))
                            .clicked()
                        {
                            play.step(ui.ctx());
                        }
                    }
                    ui.checkbox(&mut self.inspect, "Runtime inspector");
                }
            } else {
                if ui
                    .button(RichText::new("▶ Play").color(MINT))
                    .on_hover_text("F5")
                    .clicked()
                {
                    self.command(Self::play);
                }
                if ui
                    .add_enabled(self.dirty(), egui::Button::new("Apply"))
                    .on_hover_text("Ctrl+S")
                    .clicked()
                {
                    self.command(Self::save);
                }
                if ui
                    .add_enabled(self.dirty(), egui::Button::new("Discard"))
                    .clicked()
                {
                    if let Some(draft) = &mut self.draft {
                        if let Some(raw) = &draft.saved {
                            draft.raw.clone_from(raw);
                        } else {
                            self.draft = None;
                        }
                    }
                    self.note("Discarded draft", false);
                }
                ui.separator();
                if ui.button("Undo").clicked() {
                    self.command(|s| s.history(false));
                }
                if ui.button("Redo").clicked() {
                    self.command(|s| s.history(true));
                }
                if ui
                    .add_enabled(!self.dirty(), egui::Button::new("Project"))
                    .clicked()
                {
                    self.settings = Some(self.loaded.project.settings.clone());
                }
                if ui.button("Reload").clicked() {
                    if self.dirty() {
                        self.note("Apply or discard changes first", true);
                    } else {
                        self.note("Reloaded project", false);
                        self.command(Self::reload);
                    }
                }
            }
        });
    }
    fn library(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.heading("Assets");
            if ui
                .add_enabled(!self.dirty(), egui::Button::new("+ New"))
                .clicked()
            {
                self.create = Some((
                    self.draft.as_ref().map_or(Kind::Room, |d| d.kind),
                    String::new(),
                ));
            }
        });
        ui.add(egui::TextEdit::singleline(&mut self.filter).hint_text("Filter assets"));
        ui.separator();
        egui::ScrollArea::vertical().show(ui, |ui| {
            for kind in Kind::ALL {
                let ids = self
                    .loaded
                    .project
                    .resources
                    .keys()
                    .filter(|(k, id)| *k == kind && id.contains(&self.filter))
                    .map(|(_, id)| id.clone())
                    .collect::<Vec<_>>();
                let label = match kind {
                    Kind::Room => "Rooms",
                    Kind::Object => "Objects",
                    Kind::Sprite => "Sprites",
                    Kind::Animation => "Animations",
                    Kind::Tilemap => "Tilemaps",
                    Kind::Script => "Scripts",
                    Kind::Audio => "Audio",
                    Kind::Particle => "Particles",
                };
                egui::CollapsingHeader::new(RichText::new(label).strong())
                    .default_open(true)
                    .show(ui, |ui| {
                        for id in ids {
                            let selected = self
                                .draft
                                .as_ref()
                                .is_some_and(|d| d.kind == kind && d.id == id);
                            if ui.selectable_label(selected, &id).clicked() {
                                self.select(kind, &id);
                            }
                        }
                    });
            }
        });
    }
    fn dialogs(&mut self, ctx: &Context) {
        if let Some(mut settings) = self.settings.take() {
            let mut shown = true;
            let mut apply = false;
            egui::Window::new("Project settings")
                .collapsible(false)
                .open(&mut shown)
                .show(ctx, |ui| {
                    ui.label("Title");
                    ui.text_edit_singleline(&mut settings.title);
                    ui.horizontal(|ui| {
                        ui.label("Launch room");
                        egui::ComboBox::from_id_salt("start_room")
                            .selected_text(&settings.start)
                            .show_ui(ui, |ui| {
                                for (kind, id) in self
                                    .loaded
                                    .project
                                    .resources
                                    .keys()
                                    .filter(|(kind, _)| *kind == Kind::Room)
                                {
                                    debug_assert_eq!(*kind, Kind::Room);
                                    ui.selectable_value(&mut settings.start, id.clone(), id);
                                }
                            });
                    });
                    ui.horizontal(|ui| {
                        ui.label("Viewport");
                        for value in &mut settings.size {
                            ui.add(egui::DragValue::new(value).range(1..=4096));
                        }
                    });
                    ui.horizontal(|ui| {
                        ui.label("Ticks per second");
                        ui.add(egui::DragValue::new(&mut settings.hz).range(1..=240));
                    });
                    ui.horizontal(|ui| {
                        ui.label("Seed");
                        ui.add(egui::DragValue::new(&mut settings.seed));
                    });
                    apply = ui.button("Apply settings").clicked();
                });
            if apply {
                let result = self.store.update(Some(&self.loaded.revision), |project| {
                    project.settings = settings.clone();
                    Ok(())
                });
                match result {
                    Ok(_) => {
                        self.note("Saved project settings", false);
                        self.command(Self::reload);
                    }
                    Err(error) => {
                        self.note(error.to_string(), true);
                        self.settings = Some(settings);
                    }
                }
            } else if shown {
                self.settings = Some(settings);
            }
        }
        if let Some((mut kind, mut id)) = self.create.take() {
            let mut open = true;
            let mut create = false;
            egui::Window::new("New asset")
                .collapsible(false)
                .resizable(false)
                .open(&mut open)
                .show(ctx, |ui| {
                    egui::ComboBox::from_label("Kind")
                        .selected_text(kind.name())
                        .show_ui(ui, |ui| {
                            for option in Kind::ALL {
                                ui.selectable_value(&mut kind, option, option.name());
                            }
                        });
                    ui.add(egui::TextEdit::singleline(&mut id).hint_text("resource_id"));
                    create = ui.button("Create draft").clicked();
                });
            if create {
                self.command(|s| s.new_resource(kind, id));
            } else if open {
                self.create = Some((kind, id));
            }
        }
        if let Some(mut name) = self.rename.take() {
            let mut open = true;
            let mut rename = false;
            egui::Window::new("Rename asset")
                .collapsible(false)
                .resizable(false)
                .open(&mut open)
                .show(ctx, |ui| {
                    ui.text_edit_singleline(&mut name);
                    rename = ui.button("Rename and update references").clicked();
                });
            if rename {
                self.command(|s| s.rename(name));
            } else if open {
                self.rename = Some(name);
            }
        }
    }
    fn ui(&mut self, ctx: &Context) {
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::F5)) {
            if self.play.is_some() {
                self.play = None;
            } else {
                self.command(Self::play);
            }
        }
        if self.play.is_none()
            && ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::S))
        {
            self.command(Self::save);
        }
        egui::TopBottomPanel::top("command_bar")
            .frame(
                egui::Frame::new()
                    .inner_margin(egui::Margin::symmetric(16, 9))
                    .fill(Color32::from_rgb(26, 31, 42)),
            )
            .show(ctx, |ui| self.header(ui));
        egui::TopBottomPanel::bottom("status_bar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                let status = self.play.as_ref().and_then(|play| {
                    play.world
                        .fault()
                        .map(ToString::to_string)
                        .or_else(|| play.audio.error.clone())
                });
                ui.label(
                    RichText::new(status.as_deref().unwrap_or(&self.status)).color(
                        if self.bad || status.is_some() {
                            Color32::LIGHT_RED
                        } else {
                            MUTED
                        },
                    ),
                );
                ui.separator();
                ui.label(
                    RichText::new(format!(
                        "{} assets · {} diagnostics · {}",
                        self.loaded.project.resources.len(),
                        self.index.diagnostics.len(),
                        &self.loaded.revision[..8]
                    ))
                    .color(MUTED)
                    .monospace(),
                );
            });
        });
        if let Some(play) = &mut self.play {
            play.update(ctx);
            if self.inspect {
                let state = play.world.snapshot(None, None);
                egui::SidePanel::right("runtime_inspector")
                    .default_width(253.)
                    .show(ctx, |ui| {
                        ui.heading("Runtime");
                        ui.label(
                            RichText::new(format!("{} · tick {}", state.room, state.tick))
                                .monospace()
                                .color(MINT),
                        );
                        if play.world.room().size.is_some() || state.camera != [0., 0.] {
                            ui.monospace(format!(
                                "Camera {:.1}, {:.1}",
                                state.camera[0], state.camera[1]
                            ));
                        }
                        ui.label(format!(
                            "{} entities · {} particles",
                            state.entities.len(),
                            state.particles
                        ));
                        ui.separator();
                        egui::ScrollArea::vertical().show(ui, |ui| {
                            for (key, value) in state.globals {
                                ui.monospace(format!("{key} = {value}"));
                            }
                            for e in state.entities {
                                ui.push_id(e.id, |ui| {
                                    ui.collapsing(
                                        e.placement.as_deref().unwrap_or(&e.object),
                                        |ui| {
                                            ui.monospace(format!("{} · {}", e.id, e.object));
                                            ui.label(format!("Position {:.1}, {:.1}", e.x, e.y));
                                            ui.label(format!("Velocity {:.1}, {:.1}", e.vx, e.vy));
                                            if play.world.grounded(e) {
                                                ui.label("Grounded");
                                            }
                                            for (key, value) in &e.vars {
                                                ui.monospace(format!("{key} = {value}"));
                                            }
                                        },
                                    )
                                });
                            }
                        });
                    });
            }
            egui::CentralPanel::default()
                .frame(egui::Frame::NONE.fill(Color32::BLACK))
                .show(ctx, |ui| self.play_canvas.play(ui, play));
            return;
        }
        egui::SidePanel::left("assets")
            .default_width(212.)
            .min_width(155.)
            .show(ctx, |ui| self.library(ui));

        let mut draft = self.draft.take();
        let mut rename = false;
        let mut remove = false;
        if let Some(entry) = &mut draft {
            let mut resource = entry.resource();
            egui::SidePanel::right("inspector")
                .default_width(290.)
                .min_width(240.)
                .show(ctx, |ui| {
                    ui.heading("Inspector");
                    ui.label(
                        RichText::new(format!("{}/{}", entry.kind, entry.id))
                            .monospace()
                            .color(MINT),
                    );
                    ui.horizontal(|ui| {
                        if entry.kind != Kind::Script {
                            ui.checkbox(&mut self.raw, "Source");
                        }
                        if !entry.dirty() {
                            rename = ui.button("Rename").clicked();
                            remove = ui.button("Delete").clicked();
                        }
                    });
                    ui.separator();
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        if self.raw || entry.kind == Kind::Script {
                            let key = format!("{}/{}", entry.kind, entry.id);
                            if entry.kind == Kind::Script
                                && !entry.dirty()
                                && let Some(program) = self.index.programs.get(&entry.id)
                            {
                                ui.collapsing("Outline", |ui| {
                                    for (event, location) in &program.locations {
                                        if ui
                                            .link(format!(
                                                "{}:  ·  line {}",
                                                event.name(),
                                                location.line
                                            ))
                                            .clicked()
                                        {
                                            self.jump_line = Some(location.line);
                                        }
                                    }
                                    for (name, function) in &program.functions {
                                        if ui
                                            .link(format!(
                                                "{}({})  ·  line {}",
                                                name,
                                                function.parameters.join(", "),
                                                function.line
                                            ))
                                            .clicked()
                                        {
                                            self.jump_line = Some(function.line);
                                        }
                                    }
                                });
                            }
                            for e in self
                                .index
                                .edges
                                .iter()
                                .filter(|e| e.from == key || e.to == key)
                                .take(40)
                            {
                                ui.small(format!("{} → {} · {}", e.from, e.to, e.at));
                            }
                            for d in self.index.diagnostics.iter().filter(|d| {
                                d.at == key
                                    || d.at
                                        .strip_prefix(&key)
                                        .is_some_and(|suffix| suffix.starts_with('/'))
                            }) {
                                ui.colored_label(Color32::LIGHT_RED, &d.message);
                            }
                        } else if let Ok(resource) = &mut resource {
                            panels::properties(ui, resource, &self.loaded.project, &mut self.tool);
                        }
                    });
                });
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.heading(&entry.id);
                    if entry.dirty() {
                        ui.label(RichText::new("Unsaved").color(Color32::YELLOW));
                    }
                });
                ui.separator();
                if self.raw || entry.kind == Kind::Script || resource.is_err() {
                    if let Err(error) = &resource {
                        ui.colored_label(Color32::LIGHT_RED, error.to_string());
                    }
                    egui::ScrollArea::both().show(ui, |ui| {
                        edit_source(ui, entry, self.jump_line.take());
                    });
                } else if let Ok(resource) = &mut resource {
                    panels::canvas(ui, self, &entry.id, resource);
                    entry.raw = resource.encode(true);
                }
            });
        } else {
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.label("Select or create an asset");
            });
        }
        self.draft = draft;
        if rename {
            self.rename = self.draft.as_ref().map(|d| d.id.clone());
        }
        if remove {
            self.command(Self::remove);
        }
        self.dialogs(ctx);
    }
}

fn edit_source(ui: &mut Ui, entry: &mut Draft, jump_line: Option<usize>) {
    let id = ui.make_persistent_id(("source", entry.kind.name(), entry.id.as_str()));
    let mut output = egui::TextEdit::multiline(&mut entry.raw)
        .id(id)
        .code_editor()
        .min_size(ui.available_size())
        .desired_width(f32::INFINITY)
        .show(ui);
    if let Some(line) = jump_line {
        let offset = entry
            .raw
            .split_inclusive('\n')
            .take(line.saturating_sub(1))
            .map(|part| part.chars().count())
            .sum();
        let cursor = egui::text::CCursor::new(offset);
        output
            .state
            .cursor
            .set_char_range(Some(egui::text::CCursorRange::one(cursor)));
        output.state.store(ui.ctx(), output.response.id);
        output.response.request_focus();
        let rect = output
            .galley
            .pos_from_cursor(cursor)
            .translate(output.galley_pos.to_vec2());
        ui.scroll_to_rect(rect, Some(egui::Align::Center));
    }
}
