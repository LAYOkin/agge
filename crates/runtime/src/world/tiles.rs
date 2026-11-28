use super::{Bounds, World};
use agge_core::{Kind, Resource, Result, Tile, Tilemap, fail};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

const MAX_CHANGES: usize = 65_536;
pub(super) type TileChanges = BTreeMap<String, BTreeMap<String, TileDelta>>;

#[derive(Default, Debug)]
pub(crate) struct TileDelta {
    cells: BTreeMap<usize, u8>,
    blockers: BTreeMap<usize, u64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(super) struct SavedTile(pub u16, pub u16, pub char);

pub(super) type SavedTiles = BTreeMap<String, BTreeMap<String, Vec<SavedTile>>>;

/// A layer shared by scene rendering, collider overlays, and runtime queries.
#[derive(Clone, Copy, Debug)]
pub struct TileLayer<'a> {
    pub map: &'a Tilemap,
    delta: Option<&'a TileDelta>,
}

impl<'a> TileLayer<'a> {
    pub fn authored(map: &'a Tilemap) -> Self {
        Self { map, delta: None }
    }

    pub fn symbol(self, [x, y]: [usize; 2]) -> Option<char> {
        let row = self.map.rows.get(y)?.as_bytes();
        let base = *row.get(x)?;
        let index = y * row.len() + x;
        Some(
            self.delta
                .and_then(|delta| delta.cells.get(&index))
                .copied()
                .unwrap_or(base) as char,
        )
    }

    pub(crate) fn row(self, y: usize, x: usize, end: usize) -> impl Iterator<Item = u8> + 'a {
        let row = self.map.rows[y].as_bytes();
        let base = y * row.len();
        let mut edits = self
            .delta
            .map(|delta| delta.cells.range(base + x..base + end).peekable());
        row[x..end]
            .iter()
            .copied()
            .enumerate()
            .map(move |(offset, symbol)| {
                let index = base + x + offset;
                edits
                    .as_mut()
                    .and_then(|edits| edits.next_if(|&(key, _)| *key == index))
                    .map_or(symbol, |(_, &replacement)| replacement)
            })
    }
}

pub(super) struct CollisionTiles {
    at: [f32; 2],
    cell: f32,
    width: usize,
    height: usize,
    occupied: Vec<u64>,
    palette: [bool; 128],
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
        Some(Self {
            at: map.at.map(|v| v as f32),
            cell: map.cell as f32,
            width,
            height,
            occupied,
            palette,
        })
    }

    fn update(&self, patch: &mut TileDelta, index: usize, symbol: u8) {
        let i = index / 64;
        let mask = 1 << (index % 64);
        let original = self.occupied[i];
        let before = patch.blockers.get(&i).copied().unwrap_or(original);
        let after = if self.palette[symbol as usize] {
            before | mask
        } else {
            before & !mask
        };
        if after == original {
            patch.blockers.remove(&i);
        } else {
            patch.blockers.insert(i, after);
        }
    }

    pub(super) fn clamp(
        &self,
        patch: Option<&TileDelta>,
        bounds: Bounds,
        swept: Bounds,
        axis: usize,
        mut delta: f32,
    ) -> f32 {
        let [left, top] = self.at;
        let cell = self.cell;
        let column = |x: f32| ((x - left) / cell).floor() as usize;
        let row = |y: f32| ((y - top) / cell).floor() as usize;
        if swept.right < left || swept.bottom < top {
            return delta;
        }
        let (first, last) = (column(swept.left), column(swept.right).min(self.width - 1));
        if first > last {
            return delta;
        }
        for y in row(swept.top)..=row(swept.bottom).min(self.height - 1) {
            let start = y * self.width;
            let (lo, hi) = (start + first, start + last);
            let (first_word, last_word) = (lo / 64, hi / 64);
            for (offset, &base) in self.occupied[first_word..=last_word].iter().enumerate() {
                let i = first_word + offset;
                let mut word = patch
                    .and_then(|patch| patch.blockers.get(&i))
                    .copied()
                    .unwrap_or(base);
                if i == first_word {
                    word &= u64::MAX << (lo % 64);
                }
                if i == last_word {
                    word &= u64::MAX >> (63 - hi % 64);
                }
                while word != 0 {
                    let x = i * 64 + word.trailing_zeros() as usize - start;
                    word &= word - 1;
                    let tile =
                        Bounds::new(left + x as f32 * cell, top + y as f32 * cell, cell, cell);
                    delta = bounds.clamp_motion(tile, axis, delta);
                    if delta == 0. {
                        return 0.;
                    }
                }
            }
        }
        delta
    }
}

impl World {
    pub fn changed_tile_count(&self) -> usize {
        self.tile_change_count
    }

    pub(crate) fn active_tile_layers(&self) -> impl Iterator<Item = TileLayer<'_>> {
        self.room()
            .maps
            .iter()
            .filter_map(|id| self.view_tile_layer(id))
    }

    pub fn tile_layer(&self, id: &str) -> Option<TileLayer<'_>> {
        if !self.room().maps.iter().any(|map| map == id) {
            return None;
        }
        self.view_tile_layer(id)
    }

    fn view_tile_layer(&self, id: &str) -> Option<TileLayer<'_>> {
        let Resource::Tilemap(map) = self.project.get(Kind::Tilemap, id)? else {
            return None;
        };
        let delta = self
            .tile_changes
            .get(&self.room)
            .and_then(|room| room.get(id));
        Some(TileLayer { map, delta })
    }

    pub fn tile_region(&self, id: &str, [x, y, w, h]: [usize; 4]) -> Result<Vec<String>> {
        let layer = self
            .tile_layer(id)
            .ok_or_else(|| fail(format!("tilemap/{id} is absent from the current room")))?;
        let end = x.saturating_add(w);
        let bottom = y.saturating_add(h);
        if w == 0
            || h == 0
            || w.saturating_mul(h) > 4096
            || end > layer.map.rows[0].len()
            || bottom > layer.map.rows.len()
        {
            return Err(fail("tile region must cover 1–4096 cells inside the map"));
        }
        Ok((y..bottom)
            .map(|row| layer.row(row, x, end).map(char::from).collect())
            .collect())
    }

    pub(super) fn set_tile(&mut self, id: &str, cell: [usize; 2], symbol: char) -> Result<()> {
        let room = self.room.clone();
        self.set_room_tile(&room, id, cell, symbol)
    }

    fn set_room_tile(
        &mut self,
        room: &str,
        id: &str,
        [x, y]: [usize; 2],
        symbol: char,
    ) -> Result<()> {
        let Some(Resource::Room(layout)) = self.project.get(Kind::Room, room) else {
            return Err(fail(format!("absent room/{room}")));
        };
        let Some(Resource::Tilemap(map)) = self
            .project
            .get(Kind::Tilemap, id)
            .filter(|_| layout.maps.iter().any(|map| map == id))
        else {
            return Err(fail(format!("tilemap/{id} is absent from room/{room}")));
        };
        let Some(&base) = map.rows.get(y).and_then(|row| row.as_bytes().get(x)) else {
            return Err(fail(format!(
                "tilemap/{id} cell {x},{y} is outside its bounds"
            )));
        };
        if symbol != '.' && !map.tiles.contains_key(&symbol) {
            return Err(fail(format!("tilemap/{id} has no palette key {symbol:?}")));
        }
        let symbol = symbol as u8;
        let index = y * map.rows[0].len() + x;
        let previous = self
            .tile_changes
            .get(room)
            .and_then(|maps| maps.get(id))
            .and_then(|patch| patch.cells.get(&index))
            .copied()
            .unwrap_or(base);
        if previous == symbol {
            return Ok(());
        }
        if previous == base {
            if self.tile_change_count == MAX_CHANGES {
                return Err(fail("world has reached 65,536 changed tile cells"));
            }
            self.tile_change_count += 1;
        } else if symbol == base {
            self.tile_change_count -= 1;
        }
        let maps = self.tile_changes.entry(room.into()).or_default();
        let patch = maps.entry(id.into()).or_default();
        if symbol == base {
            patch.cells.remove(&index);
        } else {
            patch.cells.insert(index, symbol);
        }
        if let Some(tiles) = self.collision_tiles.get(id) {
            tiles.update(patch, index, symbol);
        }
        if patch.cells.is_empty() {
            maps.remove(id);
        }
        if maps.is_empty() {
            self.tile_changes.remove(room);
        }
        Ok(())
    }

    pub(super) fn save_tiles(&self) -> SavedTiles {
        self.tile_changes
            .iter()
            .map(|(room, maps)| {
                let tiles = maps
                    .iter()
                    .map(|(id, patch)| {
                        let Some(Resource::Tilemap(map)) = self.project.get(Kind::Tilemap, id)
                        else {
                            unreachable!()
                        };
                        let width = map.rows[0].len();
                        let cells = patch
                            .cells
                            .iter()
                            .map(|(&index, &symbol)| {
                                SavedTile(
                                    (index % width) as u16,
                                    (index / width) as u16,
                                    symbol as char,
                                )
                            })
                            .collect();
                        (id.clone(), cells)
                    })
                    .collect();
                (room.clone(), tiles)
            })
            .collect()
    }

    pub(super) fn restore_tiles(&mut self, tiles: SavedTiles) -> Result<()> {
        if tiles
            .values()
            .flat_map(BTreeMap::values)
            .map(Vec::len)
            .sum::<usize>()
            > MAX_CHANGES
        {
            return Err(fail("checkpoint has too many changed tile cells"));
        }
        for (room, maps) in tiles {
            for (id, cells) in maps {
                if cells
                    .windows(2)
                    .any(|pair| (pair[0].1, pair[0].0) >= (pair[1].1, pair[1].0))
                {
                    return Err(fail(
                        "checkpoint tile cells must be unique and ordered by row and column",
                    ));
                }
                for SavedTile(x, y, symbol) in cells {
                    self.set_room_tile(&room, &id, [x as usize, y as usize], symbol)?;
                }
            }
        }
        Ok(())
    }

    pub(super) fn meets_tile(&self, id: &str, cell: [usize; 2], key: Option<char>) -> bool {
        self.tile_layer(id).is_some_and(|layer| {
            layer.symbol(cell).is_some_and(|symbol| match key {
                Some(key) => symbol == key,
                None => layer.map.tiles.get(&symbol).is_some_and(Tile::solid),
            })
        })
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
                        tiles.clamp(None, actor, actor.swept(axis, delta), axis, delta),
                        expected
                    );
                }
            }
        }
    }
}
