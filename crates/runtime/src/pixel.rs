use agge_core::{Result, fail, pixel::Art};
use image::{Rgba, RgbaImage};

pub struct PixelComparison {
    pub changed: usize,
    pub area: Option<[usize; 4]>,
    pub image: Option<RgbaImage>,
}

/// Compare one pose by visible RGBA, with optional layer isolation.
/// The returned sheet contains before, after, and changed-mask panels.
pub fn pixel_compare(
    before: &Art,
    after: &Art,
    frame: &str,
    layer: Option<&str>,
    area: Option<[usize; 4]>,
    scale: usize,
    edge_limit: usize,
) -> Result<PixelComparison> {
    if before.size != after.size || !(1..=16).contains(&scale) {
        return Err(fail(
            "comparison requires matching canvases and a scale from 1 to 16",
        ));
    }
    if let Some(layer) = layer
        && !after.layers(Some(frame))?.any(|id| id == layer)
    {
        return Err(fail(format!("absent pixel layer {layer:?}")));
    }
    if !after.has_frame(frame) {
        return Err(fail(format!("absent pixel frame {frame:?}")));
    }
    let scan @ [x, y, w, h] = after.area(area)?;
    let before = comparison_pixels(before, frame, layer, scan)?;
    let after = comparison_pixels(after, frame, layer, scan)?;
    let mut count = 0;
    let mut min = [w, h];
    let mut max = [0, 0];
    for (i, (old, new)) in before.iter().zip(&after).enumerate() {
        if old != new {
            count += 1;
            let here = [i % w, i / w];
            for axis in 0..2 {
                min[axis] = min[axis].min(here[axis]);
                max[axis] = max[axis].max(here[axis] + 1);
            }
        }
    }
    if count == 0 {
        return Ok(PixelComparison {
            changed: 0,
            area: None,
            image: None,
        });
    }
    let [left, top, width, height] = if area.is_some() {
        [0, 0, w, h]
    } else {
        [min[0], min[1], max[0] - min[0], max[1] - min[1]]
    };
    let (out_w, out_h) = (width * scale * 3, height * scale);
    if out_w > edge_limit || out_h > edge_limit {
        return Err(fail(format!(
            "comparison exceeds {edge_limit} pixels per edge; narrow the area or scale"
        )));
    }
    let image = RgbaImage::from_fn(out_w as u32, out_h as u32, |px, py| {
        let (px, py) = (px as usize / scale, py as usize / scale);
        let i = (top + py) * w + left + px % width;
        match px / width {
            0 => before[i],
            1 => after[i],
            _ if before[i] != after[i] => Rgba([255, 65, 174, 255]),
            _ => Rgba([0; 4]),
        }
    });
    Ok(PixelComparison {
        changed: count,
        area: Some([x + left, y + top, width, height]),
        image: Some(image),
    })
}

fn comparison_pixels(
    art: &Art,
    frame: &str,
    layer: Option<&str>,
    area @ [_, _, w, h]: [usize; 4],
) -> Result<Vec<Rgba<u8>>> {
    if !art.has_frame(frame) {
        return Ok(vec![Rgba([0; 4]); w * h]);
    }
    if let Some(layer) = layer
        && !art.layers(Some(frame))?.any(|id| id == layer)
    {
        return Ok(vec![Rgba([0; 4]); w * h]);
    }
    Ok(art
        .pixels(Some(frame), layer, Some(area))?
        .into_iter()
        .map(|key| {
            let rgba = art.palette.color(key);
            Rgba(if key == 0 || rgba[3] == 0 {
                [0; 4]
            } else {
                rgba
            })
        })
        .collect())
}

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
