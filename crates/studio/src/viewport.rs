use agge_runtime::Bounds;
use eframe::egui::{self, Color32, Painter, Pos2, Rect, Response, Sense, Stroke, Ui, Vec2};

pub const PRIMARY: egui::PointerButton = egui::PointerButton::Primary;

pub struct Camera {
    zoom: f32,
    pan: Vec2,
}
impl Default for Camera {
    fn default() -> Self {
        Self {
            zoom: 1.,
            pan: Vec2::ZERO,
        }
    }
}

pub struct View {
    pub rect: Rect,
    pub visible: Rect,
    pub scale: f32,
    pub response: Response,
    pub painter: Painter,
}

impl Camera {
    pub fn toolbar(&mut self, ui: &mut Ui) {
        ui.separator();
        if ui
            .button("Fit")
            .on_hover_text("Wheel to zoom. Middle-drag to pan.")
            .clicked()
        {
            *self = Self::default();
        }
        ui.monospace(format!("{:.0}%", self.zoom * 100.));
    }

    pub fn show(&mut self, ui: &mut Ui, size: [u32; 2]) -> View {
        let region = ui.available_rect_before_wrap().shrink(10.);
        let response = ui.allocate_rect(region, Sense::click_and_drag());
        let painter = ui.painter_at(region);
        painter.rect_filled(region, 3., Color32::from_rgb(13, 17, 25));
        let fitting = agge_studio::fit(region.shrink(4.), size);
        let mut rect =
            Rect::from_center_size(fitting.center() + self.pan, fitting.size() * self.zoom);
        if response.hovered()
            && let Some(pointer) = response.hover_pos()
        {
            let scroll = ui.input(|i| i.raw_scroll_delta.y);
            if scroll != 0. {
                let old_zoom = self.zoom;
                self.zoom = (self.zoom * (scroll * 0.0035).exp()).clamp(0.25, 256.);
                let factor = self.zoom / old_zoom;
                let center = pointer + (rect.center() - pointer) * factor;
                self.pan = center - fitting.center();
                rect = Rect::from_center_size(center, fitting.size() * self.zoom);
            }
        }
        if response.dragged_by(egui::PointerButton::Middle) {
            let delta = response.drag_delta();
            self.pan += delta;
            rect = rect.translate(delta);
        }
        View {
            rect,
            visible: rect.intersect(region),
            scale: rect.width() / size[0] as f32,
            response,
            painter,
        }
    }
}

impl View {
    pub fn local(&self, position: Pos2) -> Vec2 {
        (position - self.rect.min) / self.scale
    }

    pub fn visible_bounds(&self) -> Bounds {
        let min = self.local(self.visible.min);
        let size = self.visible.size() / self.scale;
        Bounds::new(min.x, min.y, size.x, size.y)
    }

    pub fn cell(&self, position: Pos2, width: usize, height: usize) -> Option<[usize; 2]> {
        let local = self.local(position);
        if !self.visible.contains(position) || local.x < 0. || local.y < 0. {
            return None;
        }
        let [x, y] = [local.x as usize, local.y as usize];
        (x < width && y < height).then_some([x, y])
    }

    pub fn grid(&self, cell: f32, alpha: u8) {
        let spacing = cell * self.scale;
        if !self.visible.is_positive() || spacing < 5. {
            return;
        }
        let stroke = Stroke::new(1.0_f32, Color32::from_white_alpha(alpha));
        let first_x = ((self.visible.left() - self.rect.left()) / spacing)
            .ceil()
            .max(0.);
        let first_y = ((self.visible.top() - self.rect.top()) / spacing)
            .ceil()
            .max(0.);
        let mut x = self.rect.left() + first_x * spacing;
        while x <= self.visible.right() {
            self.painter
                .vline(x, self.visible.top()..=self.visible.bottom(), stroke);
            x += spacing;
        }
        let mut y = self.rect.top() + first_y * spacing;
        while y <= self.visible.bottom() {
            self.painter
                .hline(self.visible.left()..=self.visible.right(), y, stroke);
            y += spacing;
        }
    }
}

pub fn stroke_cells(from: [usize; 2], to: [usize; 2], mut paint: impl FnMut(usize, usize)) {
    let [mut x, mut y] = from.map(|n| n as isize);
    let [end_x, end_y] = to.map(|n| n as isize);
    let dx = (end_x - x).abs();
    let dy = -(end_y - y).abs();
    let step_x = (end_x - x).signum();
    let step_y = (end_y - y).signum();
    let mut error = dx + dy;
    loop {
        paint(x as usize, y as usize);
        if [x, y] == [end_x, end_y] {
            break;
        }
        let twice = 2 * error;
        if twice >= dy {
            error += dy;
            x += step_x;
        }
        if twice <= dx {
            error += dx;
            y += step_y;
        }
    }
}

pub fn flood_cells(rows: &mut [String], [w, h]: [usize; 2], start: [usize; 2], brush: char) {
    let [x, y] = start;
    let Some(&target) = rows.get(y).and_then(|row| row.as_bytes().get(x)) else {
        return;
    };
    if !brush.is_ascii() || !target.is_ascii() || char::from(target) == brush {
        return;
    }
    let mut buffer = [0; 4];
    let replacement = brush.encode_utf8(&mut buffer);
    let mut pending = vec![start];
    while let Some([x, y]) = pending.pop() {
        let Some(row) = rows.get_mut(y) else { continue };
        if row.as_bytes().get(x) != Some(&target) {
            continue;
        }
        row.replace_range(x..x + 1, replacement);
        if x > 0 {
            pending.push([x - 1, y]);
        }
        if x + 1 < w {
            pending.push([x + 1, y]);
        }
        if y > 0 {
            pending.push([x, y - 1]);
        }
        if y + 1 < h {
            pending.push([x, y + 1]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{flood_cells, stroke_cells};

    #[test]
    fn fill_stays_inside_the_connected_region() {
        let mut rows = ["..#.", ".###", "#..."].map(str::to_owned);
        flood_cells(&mut rows, [4, 3], [0, 0], '+');
        assert_eq!(rows, ["++#.", "+###", "#..."]);
        flood_cells(&mut rows, [4, 3], [0, 0], '+');
        assert_eq!(rows, ["++#.", "+###", "#..."]);
        flood_cells(&mut rows, [4, 3], [0, 0], '.');
        assert_eq!(rows, ["..#.", ".###", "#..."]);
    }

    #[test]
    fn brush_does_not_skip_cells_on_fast_drags() {
        for (from, to) in [
            ([0, 0], [8, 3]),
            ([8, 3], [0, 0]),
            ([2, 4], [2, 0]),
            ([4, 5], [1, 10]),
            ([1, 1], [1, 1]),
        ] {
            let mut cells = Vec::new();
            stroke_cells(from, to, |x, y| cells.push([x, y]));
            assert_eq!(cells.first(), Some(&from));
            assert_eq!(cells.last(), Some(&to));
            assert_eq!(
                cells.len(),
                from[0].abs_diff(to[0]).max(from[1].abs_diff(to[1])) + 1
            );
            assert!(
                cells
                    .windows(2)
                    .all(|w| w[0][0].abs_diff(w[1][0]) <= 1 && w[0][1].abs_diff(w[1][1]) <= 1)
            );
        }
    }
}
