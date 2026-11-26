use super::Bounds;
use agge_core::Tilemap;

pub(super) struct CollisionTiles {
    at: [f32; 2],
    cell: f32,
    width: usize,
    height: usize,
    occupied: Vec<u64>,
}

impl CollisionTiles {
    pub(super) fn compile(map: &Tilemap) -> Option<Self> {
        if !map.tiles.values().any(agge_core::Tile::solid) {
            return None;
        }
        let mut palette = [false; 128];
        for (&symbol, tile) in &map.tiles {
            palette[symbol as usize] = tile.solid();
        }
        let width = map.rows[0].len();
        let height = map.rows.len();
        let mut occupied = vec![0; (width * height).div_ceil(64)];
        for (index, symbol) in map.rows.iter().flat_map(|row| row.bytes()).enumerate() {
            if palette[symbol as usize] {
                occupied[index / 64] |= 1 << (index % 64);
            }
        }
        if occupied.iter().all(|&word| word == 0) {
            return None;
        }
        Some(Self {
            at: map.at.map(|v| v as f32),
            cell: map.cell as f32,
            width,
            height,
            occupied,
        })
    }

    pub(super) fn clamp(&self, bounds: Bounds, swept: Bounds, axis: usize, mut delta: f32) -> f32 {
        let [left, top] = self.at;
        let cell = self.cell;
        let column = |x: f32| ((x - left) / cell).floor() as usize;
        let row = |y: f32| ((y - top) / cell).floor() as usize;
        if swept.right < left || swept.bottom < top {
            return delta;
        }
        let (first, last) = (column(swept.left), column(swept.right).min(self.width - 1));
        for y in row(swept.top)..=row(swept.bottom).min(self.height - 1) {
            let start = y * self.width;
            for x in first..=last {
                let i = start + x;
                if self.occupied[i / 64] & (1 << (i % 64)) != 0 {
                    let tile =
                        Bounds::new(left + x as f32 * cell, top + y as f32 * cell, cell, cell);
                    delta = bounds.clamp_motion(tile, axis, delta);
                }
            }
        }
        delta
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agge_core::{FlatTile, Tile};

    #[test]
    fn packed_collision_matches_full_tile_scan_across_word_and_map_edges() {
        let rows = (0..10)
            .map(|y| {
                (0..70)
                    .map(|x| if (x + 2 * y) % 7 == 0 { '#' } else { '.' })
                    .collect::<String>()
            })
            .collect();
        let map = Tilemap {
            at: [-23, -10],
            cell: 8,
            rows,
            tiles: [('#', Tile::Flat(FlatTile([255; 4], true)))].into(),
            atlas: None,
        };
        let tiles = CollisionTiles::compile(&map).unwrap();
        let blockers = map
            .rows
            .iter()
            .enumerate()
            .flat_map(|(y, row)| {
                row.bytes()
                    .enumerate()
                    .filter(|&(_, ch)| ch == b'#')
                    .map(move |(x, _)| {
                        Bounds::new(-23. + x as f32 * 8., -10. + y as f32 * 8., 8., 8.)
                    })
            })
            .collect::<Vec<_>>();
        for x in (-40..580).step_by(3) {
            for y in (-25..85).step_by(5) {
                let actor = Bounds::new(x as f32 + 0.25, y as f32, 7., 5.);
                for (axis, delta) in [(0, 17.), (0, -13.), (1, 9.), (1, -12.)] {
                    let expected = blockers.iter().fold(delta, |motion, &blocker| {
                        actor.clamp_motion(blocker, axis, motion)
                    });
                    assert_eq!(
                        tiles.clamp(actor, actor.swept(axis, delta), axis, delta),
                        expected
                    );
                }
            }
        }
    }
}
