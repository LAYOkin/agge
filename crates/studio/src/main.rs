mod editor;
mod panels;
mod viewport;

use agge_core::{Result, fail};
use eframe::egui;
use std::path::PathBuf;

fn main() -> std::process::ExitCode {
    let file = std::env::args_os().nth(1).map(PathBuf::from);
    if let Err(error) = run(file) {
        eprintln!("{error}");
        return std::process::ExitCode::FAILURE;
    }
    std::process::ExitCode::SUCCESS
}
fn run(file: Option<PathBuf>) -> Result<()> {
    let mut options = agge_studio::options([1360., 850.]);
    options.viewport = options.viewport.with_min_inner_size([840., 565.]);
    eframe::run_native(
        "AGGE Studio",
        options,
        Box::new(move |cc| {
            let mut style = egui::Style {
                visuals: egui::Visuals::dark(),
                ..Default::default()
            };
            style.visuals.panel_fill = egui::Color32::from_rgb(21, 25, 34);
            style.visuals.window_fill = egui::Color32::from_rgb(27, 31, 42);
            style.visuals.extreme_bg_color = egui::Color32::from_rgb(14, 17, 24);
            style.visuals.selection.bg_fill = egui::Color32::from_rgb(38, 91, 88);
            style.visuals.selection.stroke.color = egui::Color32::from_rgb(152, 239, 214);
            style.visuals.widgets.inactive.bg_fill = egui::Color32::from_rgb(37, 43, 55);
            style.visuals.widgets.hovered.bg_fill = egui::Color32::from_rgb(47, 61, 70);
            style.spacing.item_spacing = egui::vec2(9., 8.);
            style.spacing.button_padding = egui::vec2(9., 5.);
            cc.egui_ctx.set_style(style);
            Ok(Box::new(editor::Studio::new(file)))
        }),
    )
    .map_err(|error| fail(error.to_string()))
}
