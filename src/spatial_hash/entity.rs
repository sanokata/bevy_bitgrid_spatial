use super::SpatialHash;
use bitgrid::{BitBoard, BitLayout};
use core::hash::Hash;

/// Per-entity spatial registration record stored in [`SpatialHash::entity_info`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct EntityEntry {
    /// Tile-space center of the entity's occupied region.
    pub(crate) center: (i32, i32),
    /// Half-size of the occupied square: covers `[center - radius, center + radius]` on each axis.
    pub(crate) radius: i32,
    /// Index into `kind_boards`; identifies what kind of entity this is.
    pub(crate) kind_idx: usize,
}

/// Axis-aligned bounding rectangle in tile coordinates (inclusive on all sides).
#[derive(Debug, Clone, Copy)]
struct Rect {
    x1: i32,
    y1: i32,
    x2: i32,
    y2: i32,
}

impl Rect {
    /// Constructs the square `[center - radius, center + radius]` on each axis.
    fn centered(center: (i32, i32), radius: i32) -> Self {
        Self {
            x1: center.0 - radius,
            y1: center.1 - radius,
            x2: center.0 + radius,
            y2: center.1 + radius,
        }
    }
}

/// Visits every tile in `src ∖ other` (set difference) exactly once,
/// decomposing the difference into at most four axis-aligned bands.
///
/// # Band decomposition
///
/// The difference of two axis-aligned rectangles `src` and `other` is covered
/// without overlap by splitting `src` around the intersection `I = src ∩ other`:
///
/// ```text
///         src.x1                   src.x2
///   ┌──────┬───────────────────┬────────┐ src.y1
///   │      │                   │        │
///   │      │     top band      │        │  ← y in [src.y1, iy1)
///   │      ├───────────────────┤        │ iy1
///   │      │                   │        │
///   │ left │   I (excluded)    │ right  │  ← y in [iy1, iy2]
///   │ band │                   │  band  │
///   │      ├───────────────────┤        │ iy2
///   │      │                   │        │
///   │      │    bottom band    │        │  ← y in (iy2, src.y2]
///   └──────┴───────────────────┴────────┘ src.y2
///        ix1                   ix2
/// ```
///
/// Top and bottom bands span the full width of `src`; left and right bands cover
/// only the intersection rows `[iy1, iy2]`. When there is no intersection
/// (`ix1 > ix2 || iy1 > iy2`), all of `src` is returned as a single region.
fn for_each_rect_diff<F: FnMut(i32, i32)>(src: Rect, other: Rect, mut f: F) {
    let ix1 = src.x1.max(other.x1);
    let ix2 = src.x2.min(other.x2);
    let iy1 = src.y1.max(other.y1);
    let iy2 = src.y2.min(other.y2);

    if ix1 > ix2 || iy1 > iy2 {
        // No overlap: the entire src is the difference.
        for y in src.y1..=src.y2 {
            for x in src.x1..=src.x2 {
                f(x, y);
            }
        }
        return;
    }

    // Top band: full width, rows above the intersection.
    if src.y1 < iy1 {
        for y in src.y1..iy1 {
            for x in src.x1..=src.x2 {
                f(x, y);
            }
        }
    }
    // Bottom band: full width, rows below the intersection.
    if iy2 < src.y2 {
        for y in (iy2 + 1)..=src.y2 {
            for x in src.x1..=src.x2 {
                f(x, y);
            }
        }
    }
    // Left band: columns left of the intersection, intersection rows only.
    if src.x1 < ix1 {
        for y in iy1..=iy2 {
            for x in src.x1..ix1 {
                f(x, y);
            }
        }
    }
    // Right band: columns right of the intersection, intersection rows only.
    if ix2 < src.x2 {
        for y in iy1..=iy2 {
            for x in (ix2 + 1)..=src.x2 {
                f(x, y);
            }
        }
    }
}

impl<ID, const W: usize, const H: usize, const E: usize, const S: usize, L: BitLayout<W, H>>
    SpatialHash<ID, W, H, E, S, L>
where
    ID: Copy + Eq + Hash,
{
    /// Returns the tile-space `(x, y, radius)` of the entity, or `None` if not registered.
    pub fn get_entity_info(&self, id: ID) -> Option<(i32, i32, i32)> {
        self.entity_info
            .get(&id)
            .map(|info| (info.center.0, info.center.1, info.radius))
    }

    /// Updates the entity's cell registrations using a minimal diff.
    ///
    /// Takes one of four paths depending on what changed:
    ///
    /// 1. **Not registered** — delegates to [`insert`](Self::insert).
    /// 2. **Exact match** (center, radius, and kind_idx all unchanged) — no-op.
    /// 3. **radius or kind_idx changed** — calls [`remove`](Self::remove) then
    ///    [`insert`](Self::insert) to fully rebuild the registration.
    /// 4. **center only changed** — uses [`for_each_rect_diff`] to visit only the
    ///    cells that entered or left the bounding square, issuing targeted
    ///    `cell_remove`/`cell_insert` calls.
    pub fn update_diff(
        &mut self,
        id: ID,
        new_center: (i32, i32),
        new_radius: i32,
        new_kind_idx: usize,
    ) {
        let old_info = if let Some(info) = self.entity_info.get(&id) {
            // Path 2: exact match — nothing to do.
            if info.center == new_center
                && info.radius == new_radius
                && info.kind_idx == new_kind_idx
            {
                return;
            }
            // Path 3: radius or kind changed — rebuild via remove + insert.
            if info.radius != new_radius || info.kind_idx != new_kind_idx {
                self.remove(id);
                self.insert(id, new_center, new_radius, new_kind_idx);
                return;
            }
            *info
        } else {
            // Path 1: not yet registered — insert fresh.
            self.insert(id, new_center, new_radius, new_kind_idx);
            return;
        };

        // Path 4: center only changed — scan the minimal band diff.

        let old_center = old_info.center;
        let radius = new_radius;
        let kind_idx = new_kind_idx;

        let old_rect = Rect::centered(old_center, radius);
        let new_rect = Rect::centered(new_center, radius);

        // Remove cells that were in the old region but are not in the new one.
        for_each_rect_diff(old_rect, new_rect, |x, y| {
            self.cell_remove(x, y, id, kind_idx);
        });

        // Insert cells that are in the new region but were not in the old one.
        for_each_rect_diff(new_rect, old_rect, |x, y| {
            self.cell_insert(x, y, id, kind_idx);
        });

        if let Some(info) = self.entity_info.get_mut(&id) {
            info.center = new_center;
        }
    }

    /// Throttled update that suppresses [`update_diff`](Self::update_diff) for small movements.
    ///
    /// Suppression criterion: strict Chebyshev distance `|dx| < threshold && |dy| < threshold`
    /// **and** no change to `radius` or `kind_idx`. With `threshold = 1`, the entity must
    /// move at least one full tile on either axis before the hash is updated.
    pub fn update_with_threshold(
        &mut self,
        id: ID,
        new_center: (i32, i32),
        new_radius: i32,
        new_kind_idx: usize,
        threshold: i32,
    ) {
        if let Some(info) = self.entity_info.get(&id) {
            let dx = (new_center.0 - info.center.0).abs();
            let dy = (new_center.1 - info.center.1).abs();

            if dx < threshold
                && dy < threshold
                && info.radius == new_radius
                && info.kind_idx == new_kind_idx
            {
                return;
            }
        }

        self.update_diff(id, new_center, new_radius, new_kind_idx);
    }

    /// Registers `id` at tile `(x, y)` in the presence and kind bitmaps.
    /// Out-of-bounds tiles are silently ignored by `tile_to_index`.
    pub(super) fn cell_insert(&mut self, x: i32, y: i32, id: ID, kind_idx: usize) {
        if let Some(idx) = BitBoard::<W, H, L>::tile_to_index(x, y) {
            self.cells[idx].push((id, kind_idx as u8));
            self.presence.set(x, y, true);
            self.layer_mut(kind_idx).set(x, y, true);
        }
    }

    /// Removes `id` from tile `(x, y)` and clears presence/kind bits if the cell is now empty.
    pub(super) fn cell_remove(&mut self, x: i32, y: i32, id: ID, kind_idx: usize) {
        if let Some(idx) = BitBoard::<W, H, L>::tile_to_index(x, y) {
            let list = &mut self.cells[idx];
            if let Some(pos) = list.iter().position(|&(e, _)| e == id) {
                list.swap_remove(pos);
            }
            if list.is_empty() {
                self.presence.set(x, y, false);
            }

            let has_same_kind = list.iter().any(|&(_, k)| k == kind_idx as u8);
            if !has_same_kind {
                self.layer_mut(kind_idx).set(x, y, false);
            }
        }
    }

    /// Inserts an entity at the given tile position with the specified radius and kind.
    ///
    /// Registers the entity in every tile within `[tile_pos ± radius]` on both axes.
    /// Calling `insert` on an already-registered ID overwrites `entity_info` but leaves
    /// stale cell entries; call [`remove`](Self::remove) first when re-registering.
    pub fn insert(&mut self, id: ID, tile_pos: (i32, i32), radius: i32, kind_idx: usize) {
        let mask = BitBoard::<W, H, L>::mask_rect(
            tile_pos.0 - radius,
            tile_pos.1 - radius,
            radius * 2 + 1,
            radius * 2 + 1,
        );

        self.presence |= &mask;
        *self.layer_mut(kind_idx) |= &mask;

        for (x, y) in mask.iter_set_bits() {
            if let Some(idx) = BitBoard::<W, H, L>::tile_to_index(x, y) {
                self.cells[idx].push((id, kind_idx as u8));
            }
        }

        self.entity_info.insert(
            id,
            EntityEntry {
                center: tile_pos,
                radius,
                kind_idx,
            },
        );
    }

    /// Removes an entity from all cells it currently occupies.
    ///
    /// If the entity is not registered, this is a no-op.
    pub fn remove(&mut self, id: ID) {
        if let Some(entry) = self.entity_info.remove(&id) {
            let radius = entry.radius;
            let center = entry.center;
            for y in (center.1 - radius)..=(center.1 + radius) {
                for x in (center.0 - radius)..=(center.0 + radius) {
                    self.cell_remove(x, y, id, entry.kind_idx);
                }
            }
        }
    }

    /// Alias for [`update_diff`](Self::update_diff).
    pub fn update(&mut self, id: ID, new_tile_pos: (i32, i32), radius: i32, kind_idx: usize) {
        self.update_diff(id, new_tile_pos, radius, kind_idx);
    }

    #[inline(always)]
    fn layer_mut(&mut self, kind_idx: usize) -> &mut BitBoard<W, H, L> {
        &mut self.kind_boards[kind_idx]
    }
}
