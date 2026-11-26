use crate::viewport::{Camera, PRIMARY, stroke_cells};
use agge_core::{
    Kind, Project, Resource,
    pixel::{Art, Profile, Renderer, source},
};
use eframe::egui::{
    self, Color32, ColorImage, Pos2, Rect, RichText, TextureHandle, TextureOptions, Ui, Vec2,
};
use std::{
    collections::BTreeSet,
    fmt::Write,
    sync::Arc,
    time::{Duration, Instant},
};

#[derive(Default)]
pub(super) struct Panel {
    source: Option<String>,
    art: Option<Result<Arc<Art>, String>>,
    authored: BTreeSet<String>,
    texture: Option<(String, Option<String>, TextureHandle)>,
    frame: String,
    layer: String,
    brush: Option<u8>,
    isolate: bool,
    playing: Option<Instant>,
    camera: Camera,
    stroke: Vec<[usize; 2]>,
    error: Option<String>,
}

impl Panel {
    pub(super) fn invalidate(&mut self) {
        self.source = None;
        self.texture = None;
    }

    fn render(&mut self, project: &Project, id: &str, text: &str) {
        if self.source.as_deref() == Some(text) {
            return;
        }
        self.source = Some(text.into());
        self.texture = None;
        self.authored.clear();
        self.authored.insert("base".into());
        if let Ok(program) = source::parse(text) {
            for stage in program.stages {
                if let source::Stage::Frame(block) = stage {
                    self.authored.insert(block.name.into());
                }
            }
        }
        self.art = Some(
            if let Some(Resource::Pixel(saved)) = project.get(Kind::Pixel, id)
                && saved == text
            {
                Renderer::new(project).render(id)
            } else {
                let mut draft = project.clone();
                draft.put(id.into(), Resource::Pixel(text.into()));
                Renderer::new(&draft).render(id)
            }
            .map_err(|error| error.to_string()),
        );
    }
}

pub(super) fn canvas(
    ui: &mut Ui,
    project: &Project,
    id: &str,
    text: &mut String,
    panel: &mut Panel,
) {
    panel.render(project, id, text);
    let art = match panel.art.as_ref().expect("render initialized") {
        Ok(art) => Arc::clone(art),
        Err(error) => {
            ui.colored_label(Color32::LIGHT_RED, error);
            ui.label("Open Source to edit the artwork.");
            return;
        }
    };
    if !art.has_frame(&panel.frame) {
        panel.frame = art
            .frame_names()
            .next()
            .expect("at least the base pose")
            .into();
    }
    let frames = art.frame_names().collect::<Vec<_>>();
    if let Some(start) = panel.playing {
        panel.frame =
            frames[((start.elapsed().as_secs_f64() * 10.) as usize) % frames.len()].into();
        ui.ctx().request_repaint_after(Duration::from_millis(50));
    }
    ui.horizontal_wrapped(|ui| {
        egui::ComboBox::from_id_salt("pixel_frame")
            .selected_text(&panel.frame)
            .show_ui(ui, |ui| {
                for name in std::iter::once("base")
                    .chain(frames.iter().copied().filter(|name| *name != "base"))
                {
                    if ui
                        .selectable_value(&mut panel.frame, name.into(), name)
                        .clicked()
                    {
                        panel.playing = None;
                    }
                }
            });
        if frames.len() > 1
            && ui
                .button(if panel.playing.is_some() {
                    "Pause poses"
                } else {
                    "Play poses"
                })
                .clicked()
        {
            panel.playing = panel.playing.is_none().then(Instant::now);
        }
        ui.label(format!("{} × {} · {}", art.size[0], art.size[1], art.style));
        panel.camera.toolbar(ui);
    });
    let layers = art
        .layers(Some(&panel.frame))
        .expect("selected frame")
        .collect::<Vec<_>>();
    if !layers.contains(&panel.layer.as_str()) {
        panel.layer = layers.last().copied().unwrap_or("paint").into();
    }
    ui.horizontal_wrapped(|ui| {
        egui::ComboBox::from_id_salt("pixel_layer")
            .selected_text(&panel.layer)
            .show_ui(ui, |ui| {
                for &name in &layers {
                    ui.selectable_value(&mut panel.layer, name.into(), name);
                }
            });
        ui.add_enabled(
            !layers.is_empty(),
            egui::Checkbox::new(&mut panel.isolate, "Isolate layer"),
        );
    });
    let can_paint = panel.authored.contains(&panel.frame) && panel.playing.is_none();
    ui.add_enabled_ui(can_paint, |ui| {
        ui.horizontal_wrapped(|ui| {
            if ui
                .selectable_label(panel.brush == Some(0), "Erase")
                .clicked()
            {
                panel.brush = Some(0);
            }
            for (key, rgba) in art.palette.swatches() {
                let color = super::color(rgba);
                if ui
                    .add(swatch(char::from(key), color).selected(panel.brush == Some(key)))
                    .clicked()
                {
                    panel.brush = Some(key);
                }
            }
        });
    });
    if !panel.authored.contains(&panel.frame) {
        ui.label("Generated pose · edit its source");
    }
    if let Some(error) = &panel.error {
        ui.colored_label(Color32::LIGHT_RED, error);
    }
    let isolate = (panel.isolate && !layers.is_empty()).then_some(panel.layer.as_str());
    let [w, h] = art.size;
    if !panel
        .texture
        .as_ref()
        .is_some_and(|(frame, layer, _)| frame == &panel.frame && layer.as_deref() == isolate)
    {
        match agge_runtime::pixel_sheet(&art, &[&panel.frame], isolate, None, 1, 1, 256) {
            Ok(image) => {
                let texture = ui.ctx().load_texture(
                    "pixel_pose",
                    ColorImage::from_rgba_unmultiplied(art.size, image.as_raw()),
                    TextureOptions::NEAREST,
                );
                panel.texture = Some((panel.frame.clone(), isolate.map(str::to_owned), texture));
            }
            Err(error) => {
                ui.colored_label(Color32::LIGHT_RED, error.to_string());
                return;
            }
        }
    }
    let view = panel.camera.show(ui, art.size.map(|n| n as u32));
    let checker = 4;
    if view.scale >= 1.0 {
        for row in (0..h).step_by(checker) {
            for col in (0..w).step_by(checker) {
                let at = view.rect.min + Vec2::new(col as f32, row as f32) * view.scale;
                let dims = Vec2::new(checker.min(w - col) as f32, checker.min(h - row) as f32)
                    * view.scale;
                let shade = if (row / checker + col / checker) % 2 == 0 {
                    42
                } else {
                    54
                };
                view.painter.rect_filled(
                    Rect::from_min_size(at, dims),
                    0.,
                    Color32::from_gray(shade),
                );
            }
        }
    }
    if let Some((_, _, texture)) = &panel.texture {
        view.painter.image(
            texture.id(),
            view.rect,
            Rect::from_min_max(Pos2::ZERO, Pos2::new(1., 1.)),
            Color32::WHITE,
        );
    }
    view.grid(1., 28);
    let pointer = ui.input(|input| input.pointer.interact_pos());
    let point = pointer.and_then(|position| view.cell(position, w, h));
    if view.response.clicked_by(egui::PointerButton::Secondary)
        && let Some([x, y]) = point
        && let Ok(pixel) = art.pixels(Some(&panel.frame), isolate, Some([x, y, 1, 1]))
    {
        panel.brush = Some(pixel[0]);
    }
    if !can_paint {
        panel.stroke.clear();
        return;
    }
    let brush = panel
        .brush
        .filter(|&key| key == 0 || art.palette.color(key)[3] != 0)
        .unwrap_or_else(|| {
            art.palette
                .ink
                .or_else(|| art.palette.swatches().next().map(|(key, _)| key))
                .unwrap_or(0)
        });
    panel.brush = Some(brush);
    let (pressed, down, released) = ui.input(|input| {
        (
            input.pointer.button_pressed(PRIMARY),
            input.pointer.button_down(PRIMARY),
            input.pointer.button_released(PRIMARY),
        )
    });
    if pressed && view.response.hovered() {
        panel.stroke.clear();
        panel.stroke.extend(point);
    } else if (down || released)
        && !panel.stroke.is_empty()
        && let Some(point) = point
        && panel.stroke.last() != Some(&point)
        && panel.stroke.len() < 256
    {
        panel.stroke.push(point);
    }
    let preview = if brush == 0 {
        Color32::from_rgba_unmultiplied(255, 96, 96, 110)
    } else {
        super::color(art.palette.color(brush))
    };
    let paint = |x, y| {
        let at = view.rect.min + Vec2::new(x as f32, y as f32) * view.scale;
        view.painter.rect_filled(
            Rect::from_min_size(at, Vec2::splat(view.scale)),
            0.,
            preview,
        );
    };
    if let Some(&[x, y]) = panel.stroke.first() {
        paint(x, y);
        for pair in panel.stroke.windows(2) {
            stroke_cells(pair[0], pair[1], paint);
        }
    }
    if released && !panel.stroke.is_empty() {
        let draw = stroke_commands(&panel.stroke, brush);
        panel.stroke.clear();
        match source::patch(text, Some(&panel.frame), &panel.layer, &draw, false) {
            Ok(updated) => {
                *text = updated;
                panel.error = None;
                ui.ctx().request_repaint();
            }
            Err(error) => panel.error = Some(error.to_string()),
        }
    } else if !down {
        panel.stroke.clear();
    }
}

fn stroke_commands(points: &[[usize; 2]], brush: u8) -> String {
    let key = if brush == 0 { '-' } else { char::from(brush) };
    let [x, y] = points[0];
    if points.len() == 1 {
        return format!("px {key} {x} {y}");
    }
    let mut draw = String::new();
    for pair in points.windows(2) {
        let [[x, y], [u, v]] = [pair[0], pair[1]];
        writeln!(draw, "line {key} {x} {y} {u} {v}").expect("string write");
    }
    draw
}

fn swatch(key: char, color: Color32) -> egui::Button<'static> {
    let text = if u16::from(color.r()) + u16::from(color.g()) + u16::from(color.b()) > 405 {
        Color32::BLACK
    } else {
        Color32::WHITE
    };
    egui::Button::new(RichText::new(key.to_string()).monospace().color(text))
        .fill(color)
        .min_size(Vec2::splat(26.))
}

pub(super) fn profile_properties(ui: &mut Ui, profile: &mut Profile) {
    let mut remove = None;
    for (&key, hex) in &mut profile.swatches {
        ui.horizontal(|ui| {
            ui.monospace(key.to_string());
            ui.add(egui::TextEdit::singleline(hex).desired_width(90.));
            if ui.small_button("Remove").clicked() {
                remove = Some(key);
            }
        });
    }
    if let Some(key) = remove {
        profile.swatches.remove(&key);
        for ramp in &mut profile.ramps {
            ramp.retain(|ch| ch != key);
        }
        profile.ramps.retain(|ramp| ramp.len() > 1);
        if profile.ink == Some(key) {
            profile.ink = None;
        }
    }
    if let Some(key) = ('a'..='z').find(|key| !profile.swatches.contains_key(key))
        && ui.button("Add swatch").clicked()
    {
        profile.swatches.insert(key, "ffffff".into());
    }
    egui::ComboBox::from_id_salt("palette_ink")
        .selected_text(
            profile
                .ink
                .map_or("Outline ink".into(), |key| key.to_string()),
        )
        .show_ui(ui, |ui| {
            ui.selectable_value(&mut profile.ink, None, "None");
            for &key in profile.swatches.keys() {
                ui.selectable_value(&mut profile.ink, Some(key), key.to_string());
            }
        });
    ui.horizontal(|ui| {
        ui.label("Light direction");
        for axis in &mut profile.light {
            ui.add(egui::DragValue::new(axis).range(-1..=1));
        }
    });
    ui.label("Ramps · dark to light");
    let mut remove_ramp = None;
    for (index, ramp) in profile.ramps.iter_mut().enumerate() {
        ui.horizontal(|ui| {
            ui.text_edit_singleline(ramp);
            if ui.small_button("×").clicked() {
                remove_ramp = Some(index);
            }
        });
    }
    if let Some(index) = remove_ramp {
        profile.ramps.remove(index);
    }
    if ui.button("Add ramp").clicked() {
        profile.ramps.push(String::new());
    }
}

pub(super) fn profile_canvas(ui: &mut Ui, profile: &Profile) {
    match profile.compile() {
        Ok(palette) => {
            ui.horizontal_wrapped(|ui| {
                for (key, rgba) in palette.swatches() {
                    let color = super::color(rgba);
                    ui.add(swatch(char::from(key), color).sense(egui::Sense::hover()));
                }
            });
        }
        Err(error) => {
            ui.colored_label(Color32::LIGHT_RED, error.to_string());
        }
    }
}
