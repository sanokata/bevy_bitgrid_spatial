use super::SpatialHash;
use bitgrid::{BitBoard, BitLayout};
use core::hash::Hash;

/// Radii for which `is_static_area_all_set` uses a pre-eroded layer cache.
/// Element `r` corresponds to `eroded_layers[layer][r - 1]`.
/// Radius 0 is handled separately via a direct layer lookup and is not in this table.
const CACHED_EROSION_RADII: [i32; 2] = [1, 2];

impl<ID, const W: usize, const H: usize, const E: usize, const S: usize, L: BitLayout<W, H>>
    SpatialHash<ID, W, H, E, S, L>
where
    ID: Copy + Eq + Hash,
{
    /// Returns the static layer at the given index.
    #[inline(always)]
    pub fn static_layer(&self, layer_idx: usize) -> &BitBoard<W, H, L> {
        &self.static_layers[layer_idx]
    }

    /// Computes a visibility mask from `(cx, cy)` within `radius`, treating the
    /// specified static layer as the set of opaque tiles.
    pub fn mask_visibility(
        &self,
        cx: i32,
        cy: i32,
        radius: f32,
        opaque_layer_idx: usize,
    ) -> BitBoard<W, H, L> {
        let opaque_board = self.static_layer(opaque_layer_idx);
        BitBoard::<W, H, L>::mask_visibility(cx, cy, radius, opaque_board)
    }

    /// Computes a visibility mask into an existing buffer (allocation-free).
    pub fn mask_visibility_into(
        &self,
        cx: i32,
        cy: i32,
        radius: f32,
        opaque_layer_idx: usize,
        out: &mut BitBoard<W, H, L>,
    ) {
        let opaque_board = self.static_layer(opaque_layer_idx);
        out.mask_visibility_into(cx, cy, radius, opaque_board);
    }

    /// Replaces an entire static layer and rebuilds its erosion caches.
    ///
    /// `revision` is a monotonically increasing token propagated from the authoritative
    /// tile-map (e.g. `TileMap::revision`). It is stored as-is and compared with `!=`
    /// by callers to detect whether a re-sync is needed; its absolute value has no meaning.
    ///
    /// **Note**: this function rebuilds both `static_layers` and `eroded_layers` atomically.
    /// [`update_static_tile`](Self::update_static_tile) only updates `static_layers` and
    /// leaves `eroded_layers` stale, so callers that rely on cached radius-1/2 queries
    /// must call this function after a batch of tile updates.
    pub fn full_sync_static_layer(
        &mut self,
        layer_idx: usize,
        board: &BitBoard<W, H, L>,
        revision: u32,
    ) {
        if layer_idx >= S {
            return;
        }

        // Copy into the existing buffer (no reallocation).
        self.static_layers[layer_idx].clone_from(board);

        // Rebuild erosion caches (radius=1, radius=2) in-place, reusing a scratch buffer.
        let mut scratch = BitBoard::<W, H, L>::new();
        self.eroded_layers[layer_idx][0].clone_from(board);
        self.eroded_layers[layer_idx][0].erode_with_buffer(1, &mut scratch);
        self.eroded_layers[layer_idx][1].clone_from(board);
        self.eroded_layers[layer_idx][1].erode_with_buffer(2, &mut scratch);

        self.static_revision = revision;
    }

    /// Updates a single tile in a static layer and advances the revision token.
    ///
    /// **Note**: this does not update `eroded_layers`. After calling this, cached
    /// radius-1/2 results from [`is_static_area_all_set`](Self::is_static_area_all_set)
    /// will be stale until [`full_sync_static_layer`](Self::full_sync_static_layer) is called.
    pub fn update_static_tile(
        &mut self,
        layer_idx: usize,
        x: i32,
        y: i32,
        val: bool,
        revision: u32,
    ) {
        if layer_idx < S {
            self.static_layers[layer_idx].set(x, y, val);
            self.static_revision = revision;
        }
    }

    /// Returns the current static-layer revision token.
    pub fn static_revision(&self) -> u32 {
        self.static_revision
    }

    /// Returns `true` if every tile in the square `[x ± radius, y ± radius]` is set
    /// in the given static layer (e.g. passability check).
    ///
    /// - `radius == 0`: direct single-tile lookup.
    /// - Radii in `CACHED_EROSION_RADII` (currently 1 and 2): O(1) lookup via the
    ///   pre-eroded cache built by [`full_sync_static_layer`](Self::full_sync_static_layer).
    /// - Any other radius: falls back to the generic `BitBoard::is_area_all_set`.
    pub fn is_static_area_all_set(&self, layer_idx: usize, x: i32, y: i32, radius: i32) -> bool {
        if layer_idx >= S {
            return false;
        }
        if radius == 0 {
            return self.static_layers[layer_idx].get(x, y);
        }
        if let Some(cache_idx) = CACHED_EROSION_RADII.iter().position(|&r| r == radius) {
            return self.eroded_layers[layer_idx][cache_idx].get(x, y);
        }
        self.static_layers[layer_idx].is_area_all_set(x, y, radius)
    }

    /// Returns `true` if at least one tile in the square `[x ± radius, y ± radius]` is
    /// set in the given static layer (e.g. collision check).
    pub fn is_static_area_any_set(&self, layer_idx: usize, x: i32, y: i32, radius: i32) -> bool {
        if layer_idx >= S {
            return false;
        }
        self.static_layers[layer_idx].is_area_any_set(x, y, radius)
    }
}
