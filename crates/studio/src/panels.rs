mod atlas;
mod room_camera;

use crate::{
    editor::{MINT, MUTED, Workbench},
    viewport::{Camera, PRIMARY, stroke_cells},
};
use agge_core::{
    AnimationMode, Color, FrameGrid, Instance, Kind, Label, Light, Meter, Object, Project,
    Resource, Room, Shape, Tilemap, WHITE,
};
use agge_runtime::{Bounds, Draw, Media, draw_hud, edit_scene};
use agge_studio::{color, fit};
use eframe::egui::{self, Color32, Pos2, Rect, RichText, Sense, Stroke, StrokeKind, Ui, Vec2};
use std::time::{Duration, Instant};

pub struct Tools {
    selected: Option<String>,
    place: String,
    snap: u32,
    brush: char,
    palette_key: String,
    frame: usize,
    sheet: bool,
    frame_grid: FrameGrid,
    atlas_keys: String,
    atlas_first: usize,
    atlas_solid: String,
    palette_error: Option<String>,
    animation: Option<Instant>,
    burst: Option<Instant>,
    media_key: Option<String>,
    media: Media,
    media_error: Option<String>,
    drag_offset: Vec2,
    camera: Camera,
    stroke: Option<[usize; 2]>,
    map_texture: Option<(Tilemap, egui::TextureHandle)>,
}
impl Default for Tools {
    fn default() -> Self {
        Self {
            selected: None,
            place: String::new(),
            snap: 8,
            brush: '#',
            palette_key: String::new(),
            frame: 0,
            sheet: true,
            frame_grid: FrameGrid {
                at: [0, 0],
                cell: [16, 16],
                grid: [1, 1],
                gap: [0, 0],
            },
            atlas_keys: String::new(),
            atlas_first: 0,
            atlas_solid: String::new(),
            palette_error: None,
            animation: None,
            burst: None,
            media_key: None,
            media: Media::default(),
            media_error: None,
            drag_offset: Vec2::ZERO,
            camera: Camera::default(),
            stroke: None,
            map_texture: None,
        }
    }
}
impl Tools {
    pub fn select_placement(&mut self, id: &str) {
        self.selected = Some(id.into());
        self.place.clear();
    }

    pub fn refresh_media(&mut self) {
        self.media_key = None;
        self.map_texture = None;
    }

    pub fn reset(&mut self) {
        *self = Self::default();
    }
}

fn field<T: egui::emath::Numeric>(ui: &mut Ui, label: &str, value: &mut T) {
    ui.horizontal(|ui| {
        ui.label(label);
        ui.add(egui::DragValue::new(value).speed(0.2));
    });
}
fn coordinates<T: egui::emath::Numeric>(ui: &mut Ui, label: &str, values: &mut [T; 2]) {
    ui.horizontal(|ui| {
        ui.label(label);
        for value in values {
            ui.add(egui::DragValue::new(value).speed(0.25));
        }
    });
}
fn tint(ui: &mut Ui, label: &str, value: &mut Color) {
    ui.horizontal(|ui| {
        ui.label(label);
        ui.color_edit_button_srgba_unmultiplied(value);
    });
}
fn text(ui: &mut Ui, label: &str, value: &mut String) {
    ui.label(label);
    ui.text_edit_singleline(value);
}
fn choose(ui: &mut Ui, label: &str, kind: Kind, project: &Project, value: &mut String, none: &str) {
    egui::ComboBox::from_id_salt(label)
        .selected_text(if value.is_empty() {
            none
        } else {
            value.as_str()
        })
        .show_ui(ui, |ui| {
            ui.selectable_value(value, String::new(), none);
            for (_, id) in project.resources.keys().filter(|(k, _)| *k == kind) {
                ui.selectable_value(value, id.clone(), id);
            }
        });
}
fn link(ui: &mut Ui, label: &str, kind: Kind, project: &Project, value: &mut Option<String>) {
    ui.horizontal(|ui| {
        ui.label(label);
        let mut id = value.clone().unwrap_or_default();
        choose(ui, label, kind, project, &mut id, "None");
        *value = (!id.is_empty()).then_some(id);
    });
}

pub fn properties(ui: &mut Ui, resource: &mut Resource, project: &Project, tools: &mut Tools) {
    match resource {
        Resource::Room(room) => {
            tint(ui, "Background", &mut room.bg);
            field(ui, "Darkness", &mut room.darkness);
            link(ui, "Music", Kind::Audio, project, &mut room.music);
            ui.checkbox(&mut room.y_sort, "Sort actors by feet");
            room_camera::properties(ui, room, project.settings.size);
            ui.collapsing("Tilemap layers", |ui| {
                let mut removal = None;
                for (i, map) in room.maps.iter_mut().enumerate() {
                    ui.horizontal(|ui| {
                        ui.push_id(i, |ui| {
                            choose(ui, "layer", Kind::Tilemap, project, map, "Select map")
                        });
                        if ui.small_button("×").clicked() {
                            removal = Some(i);
                        }
                    });
                }
                if let Some(i) = removal {
                    room.maps.remove(i);
                }
                if ui.button("Add layer").clicked()
                    && let Some((_, id)) = project
                        .resources
                        .keys()
                        .find(|(kind, _)| *kind == Kind::Tilemap)
                {
                    room.maps.push(id.clone());
                }
            });
            ui.collapsing("Lights", |ui| {
                let mut remove = None;
                for (i, light) in room.lights.iter_mut().enumerate() {
                    ui.push_id(i, |ui| {
                        coordinates(ui, "Position", &mut light.at);
                        field(ui, "Radius", &mut light.radius);
                        tint(ui, "Color", &mut light.color);
                        if ui.small_button("Remove light").clicked() {
                            remove = Some(i);
                        }
                        ui.separator();
                    });
                }
                if let Some(i) = remove {
                    room.lights.remove(i);
                }
                if ui.button("Add light").clicked() {
                    room.lights.push(Light {
                        at: [80., 80.],
                        radius: 100.,
                        color: [130, 200, 255, 255],
                    });
                }
            });
            ui.collapsing("HUD labels", |ui| {
                let mut remove = None;
                for (i, label) in room.ui.iter_mut().enumerate() {
                    ui.push_id(i, |ui| {
                        text(ui, "Text", &mut label.text);
                        text(ui, "Global value", &mut label.value);
                        coordinates(ui, "Position", &mut label.at);
                        field(ui, "Size", &mut label.size);
                        tint(ui, "Color", &mut label.color);
                        if ui.small_button("Remove label").clicked() {
                            remove = Some(i);
                        }
                        ui.separator();
                    });
                }
                if let Some(i) = remove {
                    room.ui.remove(i);
                }
                if ui.button("Add label").clicked() {
                    room.ui.push(Label {
                        at: [16., 16.],
                        text: "Label".into(),
                        color: WHITE,
                        size: 16,
                        value: String::new(),
                    });
                }
            });
            ui.collapsing("HUD meters", |ui| {
                let mut remove = None;
                for (i, meter) in room.meters.iter_mut().enumerate() {
                    ui.push_id(i, |ui| {
                        text(ui, "Global value", &mut meter.value);
                        coordinates(ui, "Position", &mut meter.at);
                        coordinates(ui, "Size", &mut meter.size);
                        field(ui, "Maximum", &mut meter.max);
                        tint(ui, "Fill", &mut meter.color);
                        tint(ui, "Track", &mut meter.track);
                        if ui.small_button("Remove meter").clicked() {
                            remove = Some(i);
                        }
                        ui.separator();
                    });
                }
                if let Some(i) = remove {
                    room.meters.remove(i);
                }
                if ui.button("Add meter").clicked() {
                    room.meters.push(Meter::default());
                }
            });
            ui.separator();
            ui.heading("Placements");
            egui::ScrollArea::vertical()
                .max_height(140.)
                .id_salt("placements")
                .show(ui, |ui| {
                    for i in &room.instances {
                        if ui
                            .selectable_label(
                                tools.selected.as_ref() == Some(&i.0),
                                format!("{} · {}", i.0, i.1),
                            )
                            .clicked()
                        {
                            tools.select_placement(&i.0);
                        }
                    }
                });
            if let Some(i) = room
                .instances
                .iter()
                .position(|i| tools.selected.as_ref() == Some(&i.0))
            {
                let inst = &mut room.instances[i];
                let tracked = room.camera.follow.as_deref() == Some(inst.0.as_str());
                text(ui, "ID", &mut inst.0);
                if tracked {
                    room.camera.follow = Some(inst.0.clone());
                }
                tools.selected = Some(inst.0.clone());
                choose(
                    ui,
                    "object",
                    Kind::Object,
                    project,
                    &mut inst.1,
                    "Select object",
                );
                field(ui, "X", &mut inst.2);
                field(ui, "Y", &mut inst.3);
                if ui.button("Remove placement").clicked() {
                    room.instances.remove(i);
                    if tracked {
                        room.camera.follow = None;
                    }
                    tools.selected = None;
                }
            }
        }
        Resource::Object(object) => {
            coordinates(ui, "Size", &mut object.size);
            tint(ui, "Tint", &mut object.color);
            ui.horizontal(|ui| {
                ui.selectable_value(&mut object.shape, Shape::Rect, "Rectangle");
                ui.selectable_value(&mut object.shape, Shape::Circle, "Ellipse");
            });
            link(ui, "Sprite", Kind::Sprite, project, &mut object.sprite);
            link(
                ui,
                "Animation",
                Kind::Animation,
                project,
                &mut object.animation,
            );
            link(ui, "Script", Kind::Script, project, &mut object.script);
            ui.horizontal(|ui| {
                ui.label("Collision");
                let initial = if object.one_way {
                    2
                } else if object.solid {
                    1
                } else {
                    0
                };
                let mut mode = initial;
                egui::ComboBox::from_id_salt("object_collision")
                    .selected_text(["None", "Solid", "One-way platform"][mode])
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut mode, 0, "None");
                        ui.selectable_value(&mut mode, 1, "Solid");
                        ui.selectable_value(&mut mode, 2, "One-way platform");
                    });
                if initial != mode {
                    object.solid = mode == 1;
                    object.one_way = mode == 2;
                }
            });
            ui.checkbox(&mut object.hidden, "Hidden in game");
            field(ui, "Gravity", &mut object.gravity);
            field(ui, "Draw depth", &mut object.depth);
            ui.collapsing("Collision filtering", |ui| {
                field(ui, "Layer bits", &mut object.layer);
                field(ui, "Mask bits", &mut object.mask);
                ui.small("Tiles use layer value 1. A mask of 0 ignores all blockers.");
            });
        }
        Resource::Sprite(sprite) => atlas::sprite_properties(ui, sprite, tools),
        Resource::Animation(animation) => {
            choose(
                ui,
                "animation sprite",
                Kind::Sprite,
                project,
                &mut animation.sprite,
                "Select sprite",
            );
            field(ui, "Frames per second", &mut animation.fps);
            ui.horizontal(|ui| {
                ui.selectable_value(&mut animation.mode, AnimationMode::Loop, "Loop");
                ui.selectable_value(&mut animation.mode, AnimationMode::Once, "Once");
                ui.selectable_value(&mut animation.mode, AnimationMode::PingPong, "Ping-pong");
            });
            for (i, frame) in animation.frames.iter_mut().enumerate() {
                ui.push_id(i, |ui| field(ui, &format!("Frame {}", i + 1), frame));
            }
            ui.horizontal(|ui| {
                if ui.button("Add frame").clicked() {
                    animation.frames.push(0);
                }
                if ui.button("Remove last").clicked() {
                    animation.frames.pop();
                }
            });
        }
        Resource::Tilemap(map) => {
            field(ui, "Cell size", &mut map.cell);
            coordinates(ui, "Offset", &mut map.at);
            let mut dims = [map.rows.first().map_or(1, String::len), map.rows.len()];
            coordinates(ui, "Columns / rows", &mut dims);
            let [w, h] = dims.map(|n| n.clamp(1, 1024));
            if map.rows.first().is_none_or(|r| r.len() != w) || h != map.rows.len() {
                map.rows.resize(h, String::new());
                for row in &mut map.rows {
                    if row.is_ascii() {
                        row.truncate(w);
                        row.extend(std::iter::repeat_n('.', w.saturating_sub(row.len())));
                    }
                }
            }
            atlas::tile_properties(ui, map, project, tools);
        }
        Resource::Audio(audio) => {
            text(ui, "WAV / OGG file", &mut audio.file);
            field(ui, "Gain", &mut audio.gain);
        }
        Resource::Particle(particle) => {
            field(ui, "Count", &mut particle.count);
            field(ui, "Lifetime (ticks)", &mut particle.life);
            field(ui, "Speed", &mut particle.speed);
            field(ui, "Radius", &mut particle.radius);
            field(ui, "Gravity", &mut particle.gravity);
            tint(ui, "Color", &mut particle.color);
        }
        Resource::Script(_) => (),
    }
}

pub fn canvas(ui: &mut Ui, work: &mut Workbench, id: &str, resource: &mut Resource) {
    match resource {
        Resource::Room(room) => room_canvas(ui, work, room),
        Resource::Object(object) => {
            let display = surface(ui, [256, 192], Sense::hover()).0;
            let mut project = work.loaded.project.clone();
            project.put(id.into(), Resource::Object(object.clone()));
            let scale = (128. / object.size[0].max(object.size[1]).max(1.)).min(4.);
            let dims = Vec2::new(object.size[0], object.size[1]) * scale;
            let origin = display.center() - dims * 0.5;
            let room = Room {
                instances: vec![Instance("preview".into(), id.into(), 0., 0.)],
                ..Room::default()
            };
            edit_scene(
                &project,
                &room,
                Bounds::new(0., 0., 4096., 4096.),
                &mut |shape| {
                    work.canvas.paint(
                        ui.ctx(),
                        &ui.painter_at(display),
                        origin,
                        scale,
                        &work.media,
                        &project,
                        shape,
                    );
                },
            );
        }
        Resource::Sprite(sprite) => atlas::sprite_canvas(ui, work, id, sprite),
        Resource::Animation(animation) => {
            if ui
                .button(if work.tool.animation.is_some() {
                    "Stop preview"
                } else {
                    "Preview animation"
                })
                .clicked()
            {
                work.tool.animation = if work.tool.animation.is_some() {
                    None
                } else {
                    Some(Instant::now())
                };
            }
            let area = surface(ui, [256, 192], Sense::hover()).0;
            if !animation.frames.is_empty() {
                if let Some(start) = work.tool.animation {
                    let elapsed =
                        (start.elapsed().as_secs_f64() * f64::from(animation.fps.max(1))) as u64;
                    work.tool.frame = animation.slot(elapsed).unwrap_or(0);
                    if animation.done(elapsed) {
                        work.tool.animation = None;
                        ui.ctx().request_repaint();
                    } else {
                        ui.ctx().request_repaint_after(Duration::from_millis(16));
                    }
                }
                if ui
                    .add(
                        egui::Slider::new(&mut work.tool.frame, 0..=animation.frames.len() - 1)
                            .text("Frame"),
                    )
                    .changed()
                {
                    work.tool.animation = None;
                }
                let slot = work.tool.frame.min(animation.frames.len() - 1);
                asset_preview(
                    ui,
                    &mut work.canvas,
                    &work.media,
                    &work.loaded.project,
                    area,
                    &animation.sprite,
                    animation.frames[slot],
                );
            }
        }
        Resource::Tilemap(map) => map_canvas(ui, work, map),
        Resource::Audio(audio) => {
            ui.label(RichText::new(&audio.file).monospace().color(MUTED));
            if ui.button("▶ Audition").clicked() {
                work.audio
                    .fire(work.store.root(), &audio.file, audio.gain.clamp(0., 4.));
            }
            if let Some(error) = &work.audio.error {
                ui.colored_label(Color32::LIGHT_RED, error);
            }
        }
        Resource::Particle(particle) => {
            if ui.button("Preview burst").clicked() {
                work.tool.burst = Some(Instant::now());
            }
            let area = surface(ui, [256, 192], Sense::hover()).0;
            if let Some(start) = work.tool.burst {
                let t = start.elapsed().as_secs_f32();
                let limit = particle.life as f32 / work.loaded.project.settings.hz as f32;
                if t < limit {
                    let painter = ui.painter_at(area);
                    let mut tint = particle.color;
                    tint[3] = (tint[3] as f32 * (1. - t / limit)) as u8;
                    for i in 0..particle.count.min(1024) {
                        let angle = i as f32 * 2.3999631;
                        let speed = (angle.sin() * 0.5 + 0.5) * particle.speed * t;
                        let delta = Vec2::new(
                            angle.cos() * speed,
                            angle.sin() * speed + particle.gravity * t * t / 2.,
                        );
                        painter.circle_filled(
                            area.center() + delta,
                            particle.radius.clamp(0.1, 512.),
                            color(tint),
                        );
                    }
                    ui.ctx().request_repaint_after(Duration::from_millis(16));
                }
            }
        }
        Resource::Script(_) => (),
    }
}

fn surface(ui: &mut Ui, size: [u32; 2], sense: Sense) -> (Rect, egui::Response) {
    let area = ui.available_rect_before_wrap().shrink(12.);
    let rect = fit(area, size);
    let response = ui.allocate_rect(rect, sense);
    ui.painter_at(rect)
        .rect_filled(rect, 0., Color32::from_rgb(13, 17, 25));
    (rect, response)
}

fn asset_preview(
    ui: &Ui,
    canvas: &mut agge_studio::Canvas,
    media: &Media,
    project: &Project,
    area: Rect,
    id: &str,
    frame: usize,
) {
    if let Some(([_, _, w, h], _)) = media.frame(project, id, frame)
        && w > 0
        && h > 0
    {
        let rect = fit(area.shrink(32.), [w, h]);
        canvas.paint(
            ui.ctx(),
            &ui.painter_at(area),
            rect.min,
            rect.width() / w as f32,
            media,
            project,
            Draw::Sprite(Bounds::new(0., 0., w as f32, h as f32), id, frame, WHITE),
        );
    }
}

fn room_canvas(ui: &mut Ui, work: &mut Workbench, room: &mut Room) {
    let project = &work.loaded.project;
    ui.horizontal(|ui| {
        ui.label("Tool");
        choose(
            ui,
            "room placement",
            Kind::Object,
            project,
            &mut work.tool.place,
            "Select / move",
        );
        ui.label("Snap");
        ui.add(egui::DragValue::new(&mut work.tool.snap).range(1..=128));
        work.tool.camera.toolbar(ui);
    });
    let viewport = project.settings.size.map(|n| n.clamp(1, 4096));
    let size = room
        .dimensions(viewport)
        .map(|n| n.clamp(1, agge_core::MAX_ROOM_DIMENSION));
    let view = work.tool.camera.show(ui, size);
    let area = view.rect;
    let response = &view.response;
    let scale = view.scale;
    if !view.visible.is_positive() {
        return;
    }
    let painter = view.painter.with_clip_rect(view.visible);
    painter.rect_filled(area, 0., color(room.bg));
    view.grid(work.tool.snap.max(1) as f32, 15);
    let snap = work.tool.snap.max(1) as f32;
    let snapped = |v: f32| (v / snap).round() * snap;
    let select_at = |position: Pos2, room: &Room| {
        let p = view.local(position);
        room.instances
            .iter()
            .rev()
            .find(|i| {
                let Some(Resource::Object(Object { size, .. })) = project.get(Kind::Object, &i.1)
                else {
                    return false;
                };
                (i.2..i.2 + size[0]).contains(&p.x) && (i.3..i.3 + size[1]).contains(&p.y)
            })
            .map(|i| i.0.clone())
    };
    if response.clicked_by(PRIMARY)
        && let Some(pos) = response
            .interact_pointer_pos()
            .filter(|p| view.visible.contains(*p))
    {
        if work.tool.place.is_empty() {
            work.tool.selected = select_at(pos, room);
        } else if room.instances.len() < 4096 {
            let used = room
                .instances
                .iter()
                .map(|i| i.0.as_str())
                .collect::<std::collections::BTreeSet<_>>();
            let id = (1..=room.instances.len() + 1)
                .map(|i| format!("item_{i}"))
                .find(|id| !used.contains(id.as_str()))
                .expect("unused placement ID");
            let p = view.local(pos);
            let inside = |coordinate: f32, dimension: u32| {
                snapped(coordinate).clamp(0., ((dimension as f32 - 1.) / snap).floor() * snap)
            };
            room.instances.push(Instance(
                id.clone(),
                work.tool.place.clone(),
                inside(p.x, size[0]),
                inside(p.y, size[1]),
            ));
            work.tool.selected = Some(id);
        }
    }
    if response.drag_started_by(PRIMARY) && work.tool.place.is_empty() {
        let origin = ui
            .input(|i| i.pointer.press_origin())
            .filter(|p| view.visible.contains(*p));
        work.tool.selected = origin.and_then(|p| select_at(p, room));
        if let Some(origin) = origin
            && let Some(i) = room
                .instances
                .iter()
                .find(|i| work.tool.selected.as_ref() == Some(&i.0))
        {
            work.tool.drag_offset = view.local(origin) - Vec2::new(i.2, i.3);
        }
    }
    if response.dragged_by(PRIMARY)
        && work.tool.place.is_empty()
        && let Some(pos) = response.interact_pointer_pos()
        && let Some(i) = room
            .instances
            .iter_mut()
            .find(|i| work.tool.selected.as_ref() == Some(&i.0))
    {
        let p = (pos - area.min) / scale - work.tool.drag_offset;
        i.2 = snapped(p.x);
        i.3 = snapped(p.y);
    }
    edit_scene(project, room, view.visible_bounds(), &mut |shape| {
        work.canvas.paint(
            ui.ctx(),
            &painter,
            area.min,
            scale,
            &work.media,
            project,
            shape,
        );
    });
    let preview = room_camera::guide(&painter, project, room, area.min, scale, viewport, size);
    let clipped = painter.with_clip_rect(preview);
    draw_hud(room, None, &mut |shape| {
        work.canvas.paint(
            ui.ctx(),
            &clipped,
            preview.min,
            scale,
            &work.media,
            project,
            shape,
        );
    });
    for i in &room.instances {
        if work.tool.selected.as_ref() == Some(&i.0)
            && let Some(Resource::Object(object)) = project.get(Kind::Object, &i.1)
        {
            let rect = Rect::from_min_size(
                area.min + Vec2::new(i.2, i.3) * scale,
                Vec2::new(object.size[0], object.size[1]) * scale,
            );
            painter.rect_stroke(
                rect.expand(2.),
                2.,
                Stroke::new(2.0_f32, MINT),
                StrokeKind::Outside,
            );
        }
    }
}

fn map_canvas(ui: &mut Ui, work: &mut Workbench, map: &mut Tilemap) {
    let w = map.rows.first().map_or(1, |row| row.len().clamp(1, 1024));
    let h = map.rows.len().clamp(1, 1024);
    ui.horizontal(|ui| {
        ui.label(RichText::new(format!("{} × {} · brush {}", w, h, work.tool.brush)).color(MUTED));
        work.tool.camera.toolbar(ui);
    });
    let view = work.tool.camera.show(ui, [w as u32, h as u32]);
    let response = &view.response;
    let active = response.clicked_by(PRIMARY)
        || response.dragged_by(PRIMARY)
        || response.drag_stopped_by(PRIMARY);
    let brush = work.tool.brush;
    if let Some(cell) = response
        .interact_pointer_pos()
        .and_then(|p| view.cell(p, w, h))
        .filter(|_| active)
        && (brush == '.' || map.tiles.contains_key(&brush))
    {
        let start = work
            .tool
            .stroke
            .or_else(|| {
                ui.input(|i| i.pointer.press_origin())
                    .and_then(|p| view.cell(p, w, h))
            })
            .unwrap_or(cell);
        let mut utf8 = [0; 4];
        let ch = brush.encode_utf8(&mut utf8);
        stroke_cells(start, cell, |x, y| {
            if let Some(row) = map.rows.get_mut(y)
                && row.is_ascii()
                && x < row.len()
            {
                row.replace_range(x..x + 1, ch);
            }
        });
        work.tool.stroke = response.dragged_by(PRIMARY).then_some(cell);
    } else {
        work.tool.stroke = None;
    }
    if !view.visible.is_positive() {
        return;
    }
    let painter = view.painter.with_clip_rect(view.visible);
    painter.rect_filled(view.rect, 0., Color32::from_rgb(19, 25, 36));
    let project = &work.loaded.project;
    if view.scale >= 2.
        && map.atlas.is_some()
        && map.tiles.values().any(|tile| tile.frame().is_some())
    {
        let local = view.visible_bounds();
        let cell = map.cell.max(1) as f32;
        let offset = Vec2::new(map.at[0] as f32, map.at[1] as f32);
        let window = Bounds {
            left: offset.x + local.left * cell,
            top: offset.y + local.top * cell,
            right: offset.x + local.right * cell,
            bottom: offset.y + local.bottom * cell,
        };
        let scale = view.scale / cell;
        let origin = view.rect.min - offset * scale;
        work.canvas.paint(
            ui.ctx(),
            &painter,
            origin,
            scale,
            &work.media,
            project,
            Draw::TileLayer(map, window),
        );
    } else {
        atlas::palette_preview(
            &painter,
            view.rect,
            [w, h],
            map,
            project,
            &work.media,
            &mut work.tool.map_texture,
        );
    }
    view.grid(1., 32);
}
