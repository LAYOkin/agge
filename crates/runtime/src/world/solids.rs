use super::Bounds;
use std::collections::{BTreeMap, BTreeSet, HashMap};

const CELL: f32 = 64.;
const MAX_CELLS: i32 = 64;

type Cell = (i32, i32);

#[derive(Clone, Copy, PartialEq, Eq)]
struct Cells {
    min: Cell,
    max: Cell,
}

impl Cells {
    fn new(bounds: Bounds) -> Self {
        let index = |value: f32| (value / CELL).floor() as i32;
        Self {
            min: (index(bounds.left), index(bounds.top)),
            max: (index(bounds.right), index(bounds.bottom)),
        }
    }

    fn small(self) -> bool {
        let width = self.max.0 - self.min.0 + 1;
        let height = self.max.1 - self.min.1 + 1;
        width.saturating_mul(height) <= MAX_CELLS
    }

    fn iter(self) -> impl Iterator<Item = Cell> {
        (self.min.1..=self.max.1).flat_map(move |y| (self.min.0..=self.max.0).map(move |x| (x, y)))
    }
}

#[derive(Default)]
pub(super) struct SolidIndex {
    locations: BTreeMap<u64, Cells>,
    buckets: HashMap<Cell, Vec<u64>>,
    large: BTreeSet<u64>,
}

impl SolidIndex {
    pub(super) fn clear(&mut self) {
        self.locations.clear();
        self.buckets.clear();
        self.large.clear();
    }

    pub(super) fn update(&mut self, id: u64, bounds: Bounds) {
        let cells = Cells::new(bounds);
        if self.locations.get(&id) == Some(&cells) {
            return;
        }
        self.remove(id);
        self.locations.insert(id, cells);
        if cells.small() {
            for cell in cells.iter() {
                self.buckets.entry(cell).or_default().push(id);
            }
        } else {
            self.large.insert(id);
        }
    }

    pub(super) fn remove(&mut self, id: u64) {
        if let Some(cells) = self.locations.remove(&id) {
            if cells.small() {
                for cell in cells.iter() {
                    if let std::collections::hash_map::Entry::Occupied(mut entry) =
                        self.buckets.entry(cell)
                    {
                        let ids = entry.get_mut();
                        if let Some(index) = ids.iter().position(|&candidate| candidate == id) {
                            ids.swap_remove(index);
                        }
                        if ids.is_empty() {
                            entry.remove();
                        }
                    }
                }
            } else {
                self.large.remove(&id);
            }
        }
    }

    // A reducer may visit a collider more than once. Axis clamping is monotone and idempotent.
    pub(super) fn visit(&self, bounds: Bounds, mut consider: impl FnMut(u64)) {
        let cells = (self.locations.len() > 8)
            .then(|| {
                // Leave room for rounding at the supported world-coordinate limits.
                Cells::new(Bounds {
                    left: bounds.left - 0.25,
                    top: bounds.top - 0.25,
                    right: bounds.right + 0.25,
                    bottom: bounds.bottom + 0.25,
                })
            })
            .filter(|cells| cells.small());
        if let Some(cells) = cells {
            for cell in cells.iter() {
                if let Some(ids) = self.buckets.get(&cell) {
                    for &id in ids {
                        consider(id);
                    }
                }
            }
            for &id in &self.large {
                consider(id);
            }
        } else {
            for &id in self.locations.keys() {
                consider(id);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Bounds, SolidIndex};
    use std::collections::BTreeSet;

    #[test]
    fn index_tracks_movement_large_colliders_and_removal() {
        let mut solids = SolidIndex::default();
        for id in 1..=12 {
            solids.update(id, Bounds::new(id as f32 * 128., 0., 8., 8.));
        }
        solids.update(20, Bounds::new(-500., -500., 1100., 1100.));
        solids.update(21, Bounds::new(-100., -100., 12., 12.));
        let near = Bounds::new(-110., -110., 24., 24.);
        let candidates = |index: &SolidIndex, area| {
            let mut result = BTreeSet::new();
            index.visit(area, |id| {
                result.insert(id);
            });
            result
        };
        assert_eq!(candidates(&solids, near), BTreeSet::from([20, 21]));
        solids.update(21, Bounds::new(132., 3., 12., 12.));
        assert_eq!(candidates(&solids, near), BTreeSet::from([20]));
        let new_position = Bounds::new(128., 0., 16., 16.);
        assert_eq!(
            candidates(&solids, new_position),
            BTreeSet::from([1, 20, 21])
        );
        assert_eq!(
            candidates(&solids, Bounds::new(-1000., -1000., 5000., 5000.)).len(),
            14
        );
        solids.remove(20);
        solids.remove(21);
        assert_eq!(candidates(&solids, new_position), BTreeSet::from([1]));
    }

    #[test]
    fn indexed_sweeps_match_an_exhaustive_reduction() {
        let mut index = SolidIndex::default();
        let mut rng = 991_357_u32;
        let mut sample = |n: u32| {
            rng = rng.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            ((rng >> 8) % n) as f32
        };
        let obstacles = (0..220)
            .map(|id| {
                let bounds = Bounds::new(
                    sample(8000) / 4. - 1000.,
                    sample(8000) / 4. - 1000.,
                    1. + sample(256) / 4.,
                    1. + sample(256) / 4.,
                );
                index.update(id, bounds);
                bounds
            })
            .collect::<Vec<_>>();
        for i in 0..2000 {
            let mover = Bounds::new(
                sample(8000) / 4. - 1000.,
                sample(8000) / 4. - 1000.,
                1. + sample(240) / 4.,
                1. + sample(240) / 4.,
            );
            let delta = sample(2200) / 4. - 275.;
            let axis = i % 2;
            let expected = obstacles.iter().fold(delta, |step, &candidate| {
                mover.clamp_motion(candidate, axis, step)
            });
            let mut actual = delta;
            index.visit(mover.swept(axis, delta), |id| {
                actual = mover.clamp_motion(obstacles[id as usize], axis, actual);
            });
            assert_eq!(actual, expected);
        }
    }
}
