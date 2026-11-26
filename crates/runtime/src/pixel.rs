use agge_core::{Result, fail, pixel::Art};
use image::{Rgba, RgbaImage};

/// Render an exact RGBA crop of one or more named poses at an integer scale.
pub fn pixel_sheet(
    art: &Art,
    frames: &[&str],
    layer: Option<&str>,
    area: Option<[usize; 4]>,
    scale: usize,
    columns: usize,
    edge_limit: usize,
) -> Result<RgbaImage> {
    let area @ [_, _, width, height] = art.area(area)?;
    if frames.is_empty()
        || frames.len() > agge_core::pixel::MAX_FRAMES
        || !(1..=16).contains(&scale)
        || !(1..=frames.len()).contains(&columns)
    {
        return Err(fail("invalid pose count, scale or column count"));
    }
    let cell_w = width * scale;
    let cell_h = height * scale;
    let w = cell_w * columns;
    let h = cell_h * frames.len().div_ceil(columns);
    if w > edge_limit || h > edge_limit {
        return Err(fail(format!(
            "pixel image exceeds {edge_limit} pixels per edge; narrow the area, frames or scale"
        )));
    }
    let mut image = RgbaImage::new(w as u32, h as u32);
    for (i, frame) in frames.iter().enumerate() {
        let glyphs = art.pixels(Some(frame), layer, Some(area))?;
        let left = (i % columns) * cell_w;
        let top = (i / columns) * cell_h;
        for (p, key) in glyphs.into_iter().enumerate() {
            if key == 0 {
                continue;
            }
            let rgba = Rgba(art.palette.color(key));
            let x = left + (p % width) * scale;
            let y = top + (p / width) * scale;
            for row in y..y + scale {
                for col in x..x + scale {
                    image.put_pixel(col as u32, row as u32, rgba);
                }
            }
        }
    }
    Ok(image)
}
