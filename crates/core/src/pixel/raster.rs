use super::{
    Glyph, MAX_EDGE, Palette, Renderer, Transform, glyph,
    source::{Words, art_key, pair},
};
use crate::{Result, fail};

const BAYER: [[i32; 4]; 4] = [[0, 8, 2, 10], [12, 4, 14, 6], [3, 11, 1, 9], [15, 7, 13, 5]];

pub struct Raster<'a> {
    pub pixels: &'a mut [Glyph],
    pub size: [usize; 2],
    pub palette: &'a Palette,
}

fn index([w, h]: [usize; 2], [x, y]: [i32; 2]) -> Option<usize> {
    (x >= 0 && y >= 0 && x < w as i32 && y < h as i32).then(|| y as usize * w + x as usize)
}

fn get(pixels: &[Glyph], size: [usize; 2], at: [i32; 2]) -> Glyph {
    index(size, at).map_or(0, |i| pixels[i])
}

pub fn blit(
    dest: &mut [Glyph],
    size: [usize; 2],
    src: &[Glyph],
    source_size: [usize; 2],
    at: [i32; 2],
    transform: Transform,
    skip_empty: bool,
) {
    let width = source_size[0];
    for (i, &key) in src.iter().enumerate() {
        if key == 0 && skip_empty {
            continue;
        }
        let [x, y] = transform.apply([i % width, i / width], source_size);
        if let Some(index) = index(size, [at[0] + x as i32, at[1] + y as i32]) {
            dest[index] = key;
        }
    }
}

#[derive(Clone, Copy)]
struct Region {
    at: [i32; 2],
    size: [usize; 2],
}

impl Region {
    fn read(words: &mut Words<'_>) -> Result<Self> {
        let at = words.point()?;
        let size = [words.number()?, words.number()?];
        if size.iter().any(|n| !(1..=MAX_EDGE).contains(n)) {
            return Err(fail("region dimensions must be 1–256"));
        }
        Ok(Self { at, size })
    }

    fn cost(self) -> usize {
        self.size[0] * self.size[1]
    }

    fn inside(self, [w, h]: [usize; 2]) -> bool {
        self.at[0] >= 0
            && self.at[1] >= 0
            && self.at[0] as usize + self.size[0] <= w
            && self.at[1] as usize + self.size[1] <= h
    }
}

impl Raster<'_> {
    fn ink(&self, words: &mut Words<'_>) -> Result<Glyph> {
        let key = words.glyph()?;
        self.palette.require(key)?;
        Ok(key)
    }

    fn put(&mut self, at: [i32; 2], ink: Glyph) {
        if let Some(i) = index(self.size, at) {
            self.pixels[i] = ink;
        }
    }

    fn region(&mut self, region: Region, mut draw: impl FnMut([i32; 2], Glyph) -> Glyph) {
        let [x, y] = region.at;
        let [w, h] = region.size;
        for row in y.max(0)..(y + h as i32).min(self.size[1] as i32) {
            for col in x.max(0)..(x + w as i32).min(self.size[0] as i32) {
                let pixel = &mut self.pixels[row as usize * self.size[0] + col as usize];
                *pixel = draw([col, row], *pixel);
            }
        }
    }

    pub fn draw(&mut self, renderer: &mut Renderer<'_>, text: &str) -> Result<()> {
        let mut words = Words::new(text);
        let op = words.next()?;
        match op {
            "px" => {
                let ink = self.ink(&mut words)?;
                let at = words.point()?;
                renderer.spend(1)?;
                self.put(at, ink);
            }
            "line" => {
                let ink = self.ink(&mut words)?;
                let from = words.point()?;
                let to = words.point()?;
                renderer
                    .spend((from[0].abs_diff(to[0]).max(from[1].abs_diff(to[1])) + 1) as usize)?;
                self.line(from, to, ink);
            }
            "box" | "rect" | "oval" | "dither" | "clear" | "recolor" => {
                let ink = if op == "clear" {
                    0
                } else {
                    self.ink(&mut words)?
                };
                let other = if matches!(op, "dither" | "recolor") {
                    self.ink(&mut words)?
                } else {
                    0
                };
                let region = Region::read(&mut words)?;
                renderer.spend(region.cost())?;
                let [w, h] = region.size.map(|n| n as i64);
                self.region(region, |[x, y], previous| {
                    let dx = i64::from(x - region.at[0]);
                    let dy = i64::from(y - region.at[1]);
                    match op {
                        "rect" if dx != 0 && dy != 0 && dx + 1 != w && dy + 1 != h => previous,
                        "oval"
                            if ((2 * dx + 1 - w) * h).pow(2) + ((2 * dy + 1 - h) * w).pow(2)
                                > (w * h).pow(2) =>
                        {
                            previous
                        }
                        "dither" if (dx + dy) % 2 != 0 => other,
                        "recolor" if previous == ink => other,
                        "recolor" => previous,
                        _ => ink,
                    }
                });
            }
            "ramp" => {
                let colors = words.next()?.as_bytes();
                if !(2..=8).contains(&colors.len()) || !colors.iter().all(|&key| glyph(key)) {
                    return Err(fail("ramp needs 2–8 visible palette glyphs"));
                }
                for &key in colors {
                    self.palette.require(key)?;
                }
                let region = Region::read(&mut words)?;
                let direction = words.point()?;
                if direction.iter().any(|axis| !(-8..=8).contains(axis)) {
                    return Err(fail("ramp direction components must be from −8 to 8"));
                }
                let source = words
                    .optional()
                    .map(|word| self.ink(&mut Words::new(word)))
                    .transpose()?;
                if source == Some(0) {
                    return Err(fail("ramp source glyph must be visible"));
                }
                let [w, h] = region.size.map(|n| n as i32 - 1);
                let [dx, dy] = direction;
                let reach = dx.abs() * w + dy.abs() * h;
                if reach == 0 {
                    return Err(fail("ramp direction must cross at least two cells"));
                }
                renderer.spend(region.cost())?;
                self.region(region, |[x, y], pixel| {
                    if pixel == 0 || source.is_some_and(|key| key != pixel) {
                        return pixel;
                    }
                    let local_x = x - region.at[0];
                    let local_y = y - region.at[1];
                    let u = if dx < 0 { w - local_x } else { local_x };
                    let v = if dy < 0 { h - local_y } else { local_y };
                    let level = (u * dx.abs() + v * dy.abs()) * (colors.len() as i32 - 1);
                    let threshold = 2 * BAYER[(y & 3) as usize][(x & 3) as usize] + 1;
                    let round_up = (level % reach) * 32 > threshold * reach;
                    colors[(level / reach) as usize + usize::from(round_up)]
                });
            }
            "poly" => {
                let ink = self.ink(&mut words)?;
                let mut points = Vec::new();
                while let Some(word) = words.optional() {
                    points.push(pair(word)?);
                    if points.len() > 32 {
                        return Err(fail("polygon accepts up to 32 points"));
                    }
                }
                if points.len() < 3 {
                    return Err(fail("polygon needs at least 3 points"));
                }
                self.polygon(renderer, &points, ink)?;
            }
            "rows" => {
                let at = words.point()?;
                self.rows(renderer, at, words.next()?)?;
            }
            "flood" => {
                let ink = self.ink(&mut words)?;
                let at = words.point()?;
                renderer.spend(self.pixels.len())?;
                self.flood(at, ink)?;
            }
            "copy" | "mirror" => {
                let axis = if op == "mirror" {
                    Some(words.next()?)
                } else {
                    None
                };
                let region = Region::read(&mut words)?;
                let [x, y] = region.at;
                let [w, h] = region.size;
                let (at, transform) = match axis {
                    Some("x") => ([x + w as i32, y], Transform::MirrorX),
                    Some("y") => ([x, y + h as i32], Transform::MirrorY),
                    Some(_) => return Err(fail("mirror axis must be x or y")),
                    None => (words.point()?, words.transform()?),
                };
                if !region.inside(self.size) {
                    return Err(fail("copy source region exceeds canvas"));
                }
                renderer.spend(region.cost())?;
                let mut src = Vec::with_capacity(region.cost());
                for y in y as usize..y as usize + h {
                    let start = y * self.size[0] + x as usize;
                    src.extend_from_slice(&self.pixels[start..start + w]);
                }
                blit(
                    self.pixels,
                    self.size,
                    &src,
                    region.size,
                    at,
                    transform,
                    true,
                );
            }
            "use" => {
                let (id, frame) = art_key(words.next()?)?;
                let at = words.point()?;
                let transform = words.transform()?;
                let art = renderer
                    .render(id)
                    .map_err(|e| fail(format!("use {id}: {e}")))?;
                let area = art.size[0] * art.size[1];
                renderer.spend(area * art.layers(frame)?.len().max(1))?;
                let src = art.pixels(frame, None, None)?;
                for &key in &src {
                    self.palette.require(key)?;
                }
                blit(self.pixels, self.size, &src, art.size, at, transform, true);
            }
            "shift" => {
                let at = words.point()?;
                renderer.spend(self.pixels.len())?;
                let src = self.pixels.to_vec();
                self.pixels.fill(0);
                blit(
                    self.pixels,
                    self.size,
                    &src,
                    self.size,
                    at,
                    Transform::Identity,
                    true,
                );
            }
            "outline" | "outline8" | "shade" => {
                let region = Region::read(&mut words)?;
                renderer.spend(self.pixels.len() + region.cost())?;
                let before = self.pixels.to_vec();
                let size = self.size;
                let palette = self.palette;
                if op == "shade" {
                    let [lx, ly] = palette.light;
                    self.region(region, |[x, y], pixel| {
                        if pixel == 0 {
                            return 0;
                        }
                        let lit = get(&before, size, [x + lx, y + ly]) == 0;
                        let dark = get(&before, size, [x - lx, y - ly]) == 0;
                        match (lit, dark) {
                            (true, false) => palette.shades[pixel as usize][1],
                            (false, true) => palette.shades[pixel as usize][0],
                            _ => pixel,
                        }
                    });
                } else {
                    let mut optional = words.optional().map(Words::new);
                    let ink = match &mut optional {
                        Some(words) => self.ink(words)?,
                        None => palette
                            .ink
                            .ok_or_else(|| fail("outline needs ink or a style ink"))?,
                    };
                    if ink == 0 {
                        return Err(fail("outline ink must be visible"));
                    }
                    let offsets = [
                        (1, 0),
                        (-1, 0),
                        (0, 1),
                        (0, -1),
                        (1, 1),
                        (-1, 1),
                        (1, -1),
                        (-1, -1),
                    ];
                    let offsets = if op == "outline8" {
                        &offsets[..]
                    } else {
                        &offsets[..4]
                    };
                    self.region(region, |[x, y], pixel| {
                        if pixel == 0
                            && offsets
                                .iter()
                                .any(|&(dx, dy)| get(&before, size, [x + dx, y + dy]) != 0)
                        {
                            ink
                        } else {
                            pixel
                        }
                    });
                }
            }
            _ => return Err(fail(format!("unknown drawing command {op:?}"))),
        }
        words.done()
    }

    fn line(&mut self, [mut x, mut y]: [i32; 2], [end_x, end_y]: [i32; 2], ink: Glyph) {
        let dx = (end_x - x).abs();
        let dy = -(end_y - y).abs();
        let sx = (end_x - x).signum();
        let sy = (end_y - y).signum();
        let mut error = dx + dy;
        loop {
            self.put([x, y], ink);
            if x == end_x && y == end_y {
                break;
            }
            let twice = error * 2;
            if twice >= dy {
                error += dy;
                x += sx;
            }
            if twice <= dx {
                error += dx;
                y += sy;
            }
        }
    }

    fn polygon(
        &mut self,
        renderer: &mut Renderer<'_>,
        points: &[[i32; 2]],
        ink: Glyph,
    ) -> Result<()> {
        let [w, h] = self.size;
        let xmin = points
            .iter()
            .map(|p| p[0])
            .min()
            .unwrap()
            .clamp(0, w as i32);
        let xmax = points
            .iter()
            .map(|p| p[0])
            .max()
            .unwrap()
            .clamp(0, w as i32);
        let ymin = points
            .iter()
            .map(|p| p[1])
            .min()
            .unwrap()
            .clamp(0, h as i32);
        let ymax = points
            .iter()
            .map(|p| p[1])
            .max()
            .unwrap()
            .clamp(0, h as i32);
        renderer.spend((xmax - xmin) as usize * (ymax - ymin) as usize * points.len())?;
        for y in ymin..ymax {
            for x in xmin..xmax {
                // Test pixel centers with the even-odd rule and integer cross products.
                let mut inside = false;
                for (&a, &b) in points.iter().zip(points.iter().cycle().skip(1)) {
                    let [ax, ay] = a.map(|n| n * 2);
                    let [bx, by] = b.map(|n| n * 2);
                    let [px, py] = [2 * x + 1, 2 * y + 1];
                    if (ay > py) != (by > py) {
                        let lhs = i64::from(px - ax) * i64::from(by - ay);
                        let rhs = i64::from(bx - ax) * i64::from(py - ay);
                        inside ^= if by > ay { lhs < rhs } else { lhs > rhs };
                    }
                }
                if inside {
                    self.put([x, y], ink);
                }
            }
        }
        Ok(())
    }

    fn rows(&mut self, renderer: &mut Renderer<'_>, [x, y]: [i32; 2], text: &str) -> Result<()> {
        for (row, stamps) in text.split('/').enumerate() {
            if row == MAX_EDGE || stamps.is_empty() {
                return Err(fail("rows need 1–256 nonempty rows separated by /"));
            }
            let mut count = None::<usize>;
            let mut col = 0;
            for key in stamps.bytes() {
                if key.is_ascii_digit() {
                    let next = count.unwrap_or(0) * 10 + usize::from(key - b'0');
                    if !(1..=MAX_EDGE).contains(&next) {
                        return Err(fail("row runs must be 1–256"));
                    }
                    count = Some(next);
                    continue;
                }
                let run = count.take().unwrap_or(1);
                if col + run > MAX_EDGE || !(matches!(key, b'.' | b'-') || glyph(key)) {
                    return Err(fail("invalid row ink or row width exceeds 256"));
                }
                renderer.spend(run)?;
                if key != b'.' {
                    let key = if key == b'-' { 0 } else { key };
                    self.palette.require(key)?;
                    for dx in col..col + run {
                        self.put([x + dx as i32, y + row as i32], key);
                    }
                }
                col += run;
            }
            if count.is_some() {
                return Err(fail("row run is missing its ink"));
            }
        }
        Ok(())
    }

    fn flood(&mut self, at: [i32; 2], ink: Glyph) -> Result<()> {
        let start =
            index(self.size, at).ok_or_else(|| fail("flood origin is outside the canvas"))?;
        let old = self.pixels[start];
        if old == ink {
            return Ok(());
        }
        self.pixels[start] = ink;
        let mut pending = vec![at];
        while let Some([x, y]) = pending.pop() {
            for next in [[x + 1, y], [x - 1, y], [x, y + 1], [x, y - 1]] {
                if let Some(i) = index(self.size, next)
                    && self.pixels[i] == old
                {
                    self.pixels[i] = ink;
                    pending.push(next);
                }
            }
        }
        Ok(())
    }
}
