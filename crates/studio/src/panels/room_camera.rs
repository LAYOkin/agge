use super::{MINT, coordinates};
use agge_core::{
    Kind, MAX_ROOM_DIMENSION, Project, Resource, Room, clamp_view_origin, follow_view_origin,
};
use eframe::egui::{self, Color32, Painter, Pos2, Rect, Stroke, StrokeKind, Ui, Vec2};

pub(super) fn properties(ui: &mut Ui, room: &mut Room, viewport: [u32; 2]) {
    let viewport = viewport.map(|n| n.clamp(1, 4096));
    ui.collapsing("Camera and room size", |ui| {
        let mut custom = room.size.is_some();
        if ui.checkbox(&mut custom, "Custom room size").changed() {
            room.size = custom.then_some(viewport);
        }
        if let Some(size) = &mut room.size {
            ui.horizontal(|ui| {
                ui.label("Dimensions");
                for (dimension, min) in size.iter_mut().zip(viewport) {
                    ui.add(
                        egui::DragValue::new(dimension)
                            .range(min..=MAX_ROOM_DIMENSION)
                            .speed(2.),
                    );
                }
            });
        }
        ui.horizontal(|ui| {
            ui.label("Follow");
            let selected = room.camera.follow.as_deref().unwrap_or("None");
            egui::ComboBox::from_id_salt("room_camera_follow")
                .selected_text(selected)
                .show_ui(ui, |ui| {
                    if ui
                        .selectable_label(room.camera.follow.is_none(), "None")
                        .clicked()
                    {
                        room.camera.follow = None;
                    }
                    for placement in &room.instances {
                        if ui
                            .selectable_label(
                                room.camera.follow.as_ref() == Some(&placement.0),
                                &placement.0,
                            )
                            .clicked()
                        {
                            room.camera.follow = Some(placement.0.clone());
                        }
                    }
                });
        });
        coordinates(ui, "Initial origin", &mut room.camera.at);
        if room.camera.follow.is_some() {
            ui.horizontal(|ui| {
                ui.label("Dead zone");
                for (axis, bound) in room.camera.dead.iter_mut().zip(viewport) {
                    ui.add(
                        egui::DragValue::new(axis)
                            .range(0. ..=bound as f32)
                            .speed(1.),
                    );
                }
            });
            ui.horizontal(|ui| {
                ui.label("Smoothing");
                ui.add(
                    egui::DragValue::new(&mut room.camera.smooth)
                        .range(0. ..=240.)
                        .speed(0.15),
                )
                .on_hover_text("0 snaps to the target. Higher positive values catch up faster.");
            });
        }
    });
}

pub(super) fn guide(
    painter: &Painter,
    project: &Project,
    room: &Room,
    origin: Pos2,
    scale: f32,
    viewport: [u32; 2],
    size: [u32; 2],
) -> Rect {
    let mut at = clamp_view_origin(room.camera.at, size, viewport);
    if let Some(target) = &room.camera.follow
        && let Some(placement) = room.instances.iter().find(|p| &p.0 == target)
        && let Some(Resource::Object(object)) = project.get(Kind::Object, &placement.1)
    {
        let center = [
            placement.2 + object.size[0] / 2.,
            placement.3 + object.size[1] / 2.,
        ];
        at = follow_view_origin(at, center, size, viewport, room.camera.dead);
    }
    let frame = Rect::from_min_size(
        origin + Vec2::new(at[0], at[1]) * scale,
        Vec2::new(viewport[0] as f32, viewport[1] as f32) * scale,
    );
    if size != viewport {
        let stroke = Stroke::new(1.5, MINT.gamma_multiply(0.6));
        painter.rect_stroke(frame, 0., stroke, StrokeKind::Inside);
    }
    if room.camera.follow.is_some() && room.camera.dead.iter().all(|&d| d > 0.) {
        let dead = Vec2::new(room.camera.dead[0], room.camera.dead[1]) * scale;
        let rect = Rect::from_center_size(frame.center(), dead);
        painter.rect_stroke(
            rect,
            0.,
            Stroke::new(1., Color32::from_white_alpha(96)),
            StrokeKind::Inside,
        );
    }
    frame
}
