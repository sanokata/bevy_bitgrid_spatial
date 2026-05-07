use super::SpatialHash;
use bitgrid::{BitBoard, BitLayout};
use core::hash::Hash;

/// Start angle and sweep angle for a sector query.
#[derive(Debug, Clone, Copy)]
pub struct SectorArgs {
    pub start_angle: f32,
    pub sweep_angle: f32,
}

impl<ID, const W: usize, const H: usize, const E: usize, const S: usize, L: BitLayout<W, H>>
    SpatialHash<ID, W, H, E, S, L>
where
    ID: Copy + Eq + Hash,
{
    /// Returns a query builder for this spatial hash.
    pub fn query(&self) -> crate::query_builder::SpatialQuery<'_, ID, W, H, E, S, L> {
        crate::query_builder::SpatialQuery::new(self)
    }

    /// Returns the occupancy [`BitBoard`] for the given entity kind index.
    #[inline(always)]
    pub fn layer(&self, kind_idx: usize) -> &BitBoard<W, H, L> {
        &self.kind_boards[kind_idx]
    }

    /// Selects the narrowest [`BitBoard`] to iterate based on `kind_mask`.
    ///
    /// - `None` or multi-bit mask → returns `presence` (all entities); per-cell kind
    ///   filtering is then applied inside `query_with_mask`.
    /// - Single-bit mask `Some(1 << k)` → returns `kind_boards[k]` directly, skipping
    ///   tiles that contain no entity of that kind.
    /// - `k >= E` falls back to `presence`; treated as a programming error and caught
    ///   by `debug_assert!` in debug builds.
    #[inline]
    fn select_target_board(&self, kind_mask: Option<u64>) -> &BitBoard<W, H, L> {
        match kind_mask {
            Some(mask) if mask.count_ones() == 1 => {
                let k = mask.trailing_zeros() as usize;
                debug_assert!(
                    k < E,
                    "kind_mask points to layer {} but only {} layers exist",
                    k,
                    E
                );
                if k < E { self.layer(k) } else { &self.presence }
            }
            _ => &self.presence,
        }
    }

    /// Core query kernel: iterates all occupied tiles in `search_mask`, then
    /// applies `kind_mask` and `exclude` filters before invoking `callback`.
    fn query_with_mask<F>(
        &self,
        search_mask: &BitBoard<W, H, L>,
        kind_mask: Option<u64>,
        exclude: Option<ID>,
        mut callback: F,
    ) where
        F: FnMut(ID),
    {
        let target_board = self.select_target_board(kind_mask);
        search_mask.for_each_overlap(target_board, |_x, _y, idx| {
            for &(e, k) in &self.cells[idx] {
                let pass_exclude = exclude.is_none_or(|ex| e != ex);
                let pass_kind = kind_mask.is_none_or(|mask| (mask >> k) & 1 == 1);
                if pass_exclude && pass_kind {
                    callback(e);
                }
            }
        });
    }

    /// Finds entities within a circular area.
    pub(crate) fn query_circle_callback<F>(
        &self,
        center: (i32, i32),
        radius: f32,
        kind_mask: Option<u64>,
        exclude: Option<ID>,
        callback: F,
    ) where
        F: FnMut(ID),
    {
        let circle_mask = BitBoard::<W, H, L>::mask_sector(center.0, center.1, radius, 0.0, 360.0);
        self.query_with_mask(&circle_mask, kind_mask, exclude, callback);
    }

    /// Finds entities within a sector (cone).
    pub(crate) fn query_sector_callback<F>(
        &self,
        center: (i32, i32),
        radius: f32,
        args: SectorArgs,
        kind_mask: Option<u64>,
        exclude: Option<ID>,
        callback: F,
    ) where
        F: FnMut(ID),
    {
        let sector_mask = BitBoard::<W, H, L>::mask_sector(
            center.0,
            center.1,
            radius,
            args.start_angle,
            args.sweep_angle,
        );
        self.query_with_mask(&sector_mask, kind_mask, exclude, callback);
    }

    /// Finds entities within an arbitrary [`BitBoard`] mask, with optional kind filtering.
    pub(crate) fn query_mask_callback<F>(
        &self,
        mask: &BitBoard<W, H, L>,
        kind_mask: Option<u64>,
        exclude: Option<ID>,
        callback: F,
    ) where
        F: FnMut(ID),
    {
        self.query_with_mask(mask, kind_mask, exclude, callback);
    }

    /// Returns `true` if any entity occupies the given tile.
    ///
    /// Returns `false` for out-of-bounds coordinates.
    pub fn is_tile_occupied(&self, tile_x: i32, tile_y: i32) -> bool {
        if tile_x < 0 || tile_y < 0 || tile_x >= (W as i32) || tile_y >= (H as i32) {
            return false;
        }
        self.presence.get(tile_x, tile_y)
    }
}
