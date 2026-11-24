use super::{Tools, asset_preview, choose, color, coordinates, field, fit, text};
use crate::editor::{MINT, MUTED, Workbench};
use agge_core::{Kind, Project, Resource, Sprite, Tile, Tilemap};
use agge_runtime::Media;
use eframe::egui::{self, Color32, Painter, Pos2, Rect, RichText, Stroke, StrokeKind, Ui, Vec2};

pub(super) fn sprite_properties(ui: &mut Ui, sprite: &mut Sprite, tools: &mut Tools) {
    text(ui, "PNG file", &mut sprite.file);
    ui.collapsing("Slice grid", |ui| {
        coordinates(ui, "Origin", &mut tools.frame_grid.at);
        coordinates(ui, "Frame size", &mut tools.frame_grid.cell);
        coordinates(ui, "Columns / rows", &mut tools.frame_grid.grid);
        coordinates(ui, "Spacing", &mut tools.frame_grid.gap);
        if ui.button("Generate frames").clicked() {
            match tools.frame_grid.frames() {
                Ok(frames) => {
                    if tools.media_key.as_deref() == Some(sprite.file.as_str())
                        && let Some(image) = tools.media.sprites.values().next()
                        && frames
                            .iter()
                            .any(|&[x, y, w, h]| x + w > image.width() || y + h > image.height())
                    {
                        tools.palette_error = Some("The grid extends outside the PNG.".into());
                        return;
                    }
                    sprite.frames = frames;
                    tools.frame = 0;
                    tools.sheet = true;
                    tools.palette_error = None;
                }
                Err(error) => tools.palette_error = Some(error.to_string()),
            }
        }
        if let Some(error) = &tools.palette_error {
            ui.colored_label(Color32::LIGHT_RED, error);
        }
    });

    if !sprite.frames.is_empty() {
        tools.frame = tools.frame.min(sprite.frames.len() - 1);
        ui.horizontal(|ui| {
            ui.label("Frame");
            ui.add(egui::DragValue::new(&mut tools.frame).range(0..=sprite.frames.len() - 1));
        });
        for (label, value) in ["X", "Y", "Width", "Height"]
            .into_iter()
            .zip(&mut sprite.frames[tools.frame])
        {
            field(ui, label, value);
        }
    }
    ui.horizontal(|ui| {
        if ui.button("Add frame").clicked() {
            tools.frame = sprite.frames.len();
            sprite.frames.push([0, 0, 16, 16]);
        }
        if ui
            .add_enabled(
                tools.frame < sprite.frames.len(),
                egui::Button::new("Remove"),
            )
            .clicked()
        {
            sprite.frames.remove(tools.frame);
            tools.frame = tools.frame.saturating_sub(1);
        }
    });
}

pub(super) fn sprite_canvas(ui: &mut Ui, work: &mut Workbench, id: &str, sprite: &Sprite) {
    if work.tool.media_key.as_deref() != Some(sprite.file.as_str()) {
        work.tool.media_key = Some(sprite.file.clone());
        work.tool.media = Media::default();
        work.tool.media_error = work
            .tool
            .media
            .load_sprite(work.store.root(), id, &sprite.file)
            .err()
            .map(|error| error.to_string());
        work.canvas = agge_studio::Canvas::default();
    }
    if let Some(error) = &work.tool.media_error {
        ui.colored_label(Color32::LIGHT_RED, error);
        return;
    }
    let Some(image) = work.tool.media.sprites.get(id) else {
        return;
    };
    let dimensions = [image.width(), image.height()];
    ui.horizontal(|ui| {
        ui.selectable_value(&mut work.tool.sheet, true, "Sheet");
        ui.selectable_value(&mut work.tool.sheet, false, "Frame preview");
        ui.label(
            RichText::new(format!(
                "{} × {} · {} frames",
                dimensions[0],
                dimensions[1],
                sprite.frame_count()
            ))
            .color(MUTED),
        );
    });
    if sprite.frames.iter().any(|&[x, y, w, h]| {
        w == 0
            || h == 0
            || x.saturating_add(w) > dimensions[0]
            || y.saturating_add(h) > dimensions[1]
    }) {
        ui.colored_label(
            Color32::LIGHT_RED,
            "One or more frames are empty or outside the sheet.",
        );
    }
    let available = ui.available_rect_before_wrap().shrink(18.);
    if !work.tool.sheet {
        let mut project = Project::default();
        project.put(id.into(), Resource::Sprite(sprite.clone()));
        let frame = work.tool.frame.min(sprite.frame_count() - 1);
        if work.tool.media.frame(&project, id, frame).is_none() {
            ui.colored_label(
                Color32::LIGHT_RED,
                "The frame is outside the sheet or empty.",
            );
        } else {
            asset_preview(
                ui,
                &mut work.canvas,
                &work.tool.media,
                &project,
                available,
                id,
                frame,
            );
        }
        return;
    }
    let screen = fit(available, dimensions);
    let response = ui.allocate_rect(screen, egui::Sense::click());
    let painter = ui.painter_at(screen);
    painter.rect_filled(screen, 0., Color32::from_rgb(13, 17, 25));
    work.canvas.sheet(&painter, screen, &work.tool.media, id);
    let scale = screen.width() / dimensions[0] as f32;
    let rect = |[x, y, w, h]: [u32; 4]| {
        Rect::from_min_size(
            screen.min + Vec2::new(x as f32, y as f32) * scale,
            Vec2::new(w as f32, h as f32) * scale,
        )
    };
    if response.clicked()
        && let Some(pointer) = response.interact_pointer_pos()
        && let Some(i) = sprite
            .frames
            .iter()
            .position(|&frame| rect(frame).contains(pointer))
    {
        work.tool.frame = i;
    }
    for (i, &frame) in sprite.frames.iter().enumerate() {
        let selected = i == work.tool.frame;
        painter.rect_stroke(
            rect(frame),
            0.,
            Stroke::new(
                if selected { 2. } else { 1. },
                if selected {
                    MINT
                } else {
                    Color32::from_white_alpha(102)
                },
            ),
            StrokeKind::Inside,
        );
    }
}

pub(super) fn tile_properties(
    ui: &mut Ui,
    map: &mut Tilemap,
    project: &Project,
    tools: &mut Tools,
) {
    let mut atlas = map.atlas.clone().unwrap_or_default();
    ui.horizontal(|ui| {
        ui.label("Atlas");
        choose(
            ui,
            "tilemap atlas",
            Kind::Sprite,
            project,
            &mut atlas,
            "None",
        );
    });
    let cleared = map.atlas.is_some() && atlas.is_empty();
    map.atlas = (!atlas.is_empty()).then_some(atlas.clone());
    let frame_count = project
        .get(Kind::Sprite, &atlas)
        .and_then(|resource| match resource {
            Resource::Sprite(sprite) => Some(sprite.frame_count()),
            _ => None,
        });
    if let Some(frames) = frame_count {
        ui.collapsing("Map sprite frames", |ui| {
            text(ui, "Tile keys in frame order", &mut tools.atlas_keys);
            field(ui, "First frame", &mut tools.atlas_first);
            text(ui, "Solid keys", &mut tools.atlas_solid);
            if ui.button("Map frames").clicked() {
                match map.set_atlas_palette(
                    &atlas,
                    frames,
                    &tools.atlas_keys,
                    tools.atlas_first,
                    &tools.atlas_solid,
                ) {
                    Ok(()) => {
                        if let Some(key) = tools.atlas_keys.chars().next() {
                            tools.brush = key;
                        }
                        tools.palette_error = None;
                    }
                    Err(error) => tools.palette_error = Some(error.to_string()),
                }
            }
            if let Some(error) = &tools.palette_error {
                ui.colored_label(Color32::LIGHT_RED, error);
            }
        });
    }
    ui.label("Palette");
    ui.selectable_value(&mut tools.brush, '.', "Eraser");
    for (&key, tile) in &mut map.tiles {
        ui.push_id(key, |ui| {
            let mut tint = tile.color();
            let mut solid = tile.solid();
            let mut frame = tile.frame();
            ui.horizontal(|ui| {
                ui.selectable_value(&mut tools.brush, key, key.to_string());
                ui.color_edit_button_srgba_unmultiplied(&mut tint);
                ui.checkbox(&mut solid, "Solid");
            });
            if key == tools.brush
                && let Some(frames) = frame_count
            {
                ui.horizontal(|ui| {
                    let mut textured = frame.is_some();
                    ui.checkbox(&mut textured, "Frame");
                    let mut number = frame.unwrap_or(0);
                    if textured {
                        ui.add(egui::DragValue::new(&mut number).range(0..=frames - 1));
                    }
                    frame = textured.then_some(number);
                });
            }
            if cleared {
                frame = None;
            }
            if (frame, tint, solid) != (tile.frame(), tile.color(), tile.solid()) {
                tile.set_style(frame, tint, solid);
            }
        });
    }
    ui.horizontal(|ui| {
        ui.add(
            egui::TextEdit::singleline(&mut tools.palette_key)
                .desired_width(34.)
                .char_limit(1),
        );
        if ui.button("Add tile").clicked()
            && let Some(key) = tools.palette_key.chars().next()
            && key.is_ascii_graphic()
            && key != '.'
        {
            map.tiles
                .entry(key)
                .or_insert(Tile::flat([140, 182, 204, 255], false));
            tools.brush = key;
        }
    });
}

pub(super) fn palette_preview(
    painter: &Painter,
    rect: Rect,
    [width, height]: [usize; 2],
    map: &Tilemap,
    project: &Project,
    media: &Media,
    cached: &mut Option<(Tilemap, egui::TextureHandle)>,
) {
    if cached.as_ref().is_none_or(|(old, _)| old != map) {
        let mut swatches = [Color32::TRANSPARENT; 128];
        for (key, tile) in &map.tiles {
            let Some(swatch) = swatches.get_mut(*key as usize) else {
                continue;
            };
            let tint = tile.color();
            *swatch = color(match (map.atlas.as_deref(), tile.frame()) {
                (Some(atlas), Some(frame)) => {
                    media
                        .frame(project, atlas, frame)
                        .map_or(tint, |([x, y, w, h], image)| {
                            let pixel = image.get_pixel(x + w / 2, y + h / 2).0;
                            std::array::from_fn(|i| {
                                (u16::from(pixel[i]) * u16::from(tint[i]) / 255) as u8
                            })
                        })
                }
                _ => tint,
            });
        }
        let mut pixels = vec![Color32::TRANSPARENT; width * height];
        for (y, row) in map.rows.iter().take(height).enumerate() {
            for (x, &byte) in row.as_bytes().iter().take(width).enumerate() {
                if let Some(&swatch) = swatches.get(byte as usize) {
                    pixels[y * width + x] = swatch;
                }
            }
        }
        let image = egui::ColorImage::new([width, height], pixels);
        if let Some((old, texture)) = cached {
            old.clone_from(map);
            texture.set(image, egui::TextureOptions::NEAREST);
        } else {
            let texture =
                painter
                    .ctx()
                    .load_texture("tilemap canvas", image, egui::TextureOptions::NEAREST);
            *cached = Some((map.clone(), texture));
        }
    }
    if let Some((_, texture)) = cached {
        painter.image(
            texture.id(),
            rect,
            Rect::from_min_max(Pos2::ZERO, Pos2::new(1., 1.)),
            Color32::WHITE,
        );
    }
}
