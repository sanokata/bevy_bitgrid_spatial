use ahash::RandomState;
use bitgrid::{BitBoard, BitLayout, RowMajorLayout};
use core::hash::Hash;
use hashbrown::HashMap;
use smallvec::SmallVec;
use std::marker::PhantomData;

#[cfg(feature = "bevy")]
use bevy::prelude::*;

mod entity;
pub mod query;
mod static_layer;

pub(crate) use entity::EntityEntry;
pub use query::SectorArgs;

/// Per-cell entity list. Each entry stores the entity ID and its kind index.
type CellStorage<ID> = Box<[SmallVec<[(ID, u8); 4]>]>;

/// Tile-based spatial hash for tracking entity positions on a fixed-size grid.
///
/// - `ID`  — entity identifier type (e.g. `Entity`, `u32`)
/// - `W`, `H` — grid width and height in tiles
/// - `E`  — number of dynamic entity kind layers
/// - `S`  — number of static layers (e.g. terrain or collision maps)
/// - `L`  — memory layout for the underlying [`BitBoard`]s (default: [`RowMajorLayout`])
#[cfg_attr(feature = "bevy", derive(Resource))]
pub struct SpatialHash<
    ID,
    const W: usize,
    const H: usize,
    const E: usize,
    const S: usize,
    L = RowMajorLayout,
> where
    ID: Copy + Eq + Hash,
    L: BitLayout<W, H>,
{
    /// Per-tile entity lists, indexed by `tile_to_index(x, y)`.
    /// Each entry is `(ID, kind_idx as u8)`.
    pub(crate) cells: CellStorage<ID>,
    /// Per-entity metadata used for differential updates and removal.
    pub(crate) entity_info: HashMap<ID, EntityEntry, RandomState>,
    /// Aggregate occupancy bitmap: a bit is set if any entity occupies that tile.
    pub(crate) presence: BitBoard<W, H, L>,
    /// Per-kind occupancy bitmaps (`E` layers); a bit is set if any entity of that
    /// kind occupies the tile.
    pub(crate) kind_boards: [BitBoard<W, H, L>; E],
    /// Static layer bitmaps (`S` layers), e.g. terrain or collision maps.
    pub(crate) static_layers: [BitBoard<W, H, L>; S],
    /// Pre-eroded caches for fast `is_static_area_all_set` queries at small radii.
    /// Indexed as `[layer][cache_slot]`; see `CACHED_EROSION_RADII` in `static_layer.rs`.
    pub(crate) eroded_layers: [[BitBoard<W, H, L>; 2]; S],
    /// Change-detection token propagated from the authoritative tile-map revision counter.
    /// Only used for `!=` comparisons; the absolute value has no meaning.
    pub(crate) static_revision: u32,
    pub(crate) _layout: PhantomData<L>,
}

impl<ID, const W: usize, const H: usize, const E: usize, const S: usize, L: BitLayout<W, H>> Default
    for SpatialHash<ID, W, H, E, S, L>
where
    ID: Copy + Eq + Hash,
{
    fn default() -> Self {
        let total_words = L::total_words();
        let cells = vec![SmallVec::new(); total_words * 64].into_boxed_slice();
        Self {
            cells,
            entity_info: HashMap::with_hasher(RandomState::default()),
            presence: BitBoard::default(),
            kind_boards: std::array::from_fn(|_| BitBoard::default()),
            static_layers: std::array::from_fn(|_| BitBoard::default()),
            eroded_layers: std::array::from_fn(|_| std::array::from_fn(|_| BitBoard::default())),
            static_revision: 0,
            _layout: PhantomData,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Use u32 as the entity ID type for tests.
    type TestSpatialHash = SpatialHash<u32, 256, 256, 2, 5>;
    type TestBoard = BitBoard<256, 256>;

    #[test]
    fn test_spatial_insert_remove() {
        let mut hash = TestSpatialHash::default();
        let entity = 1u32;

        hash.insert(entity, (10, 10), 1, 0);
        assert!(hash.is_tile_occupied(10, 10));
        assert!(hash.is_tile_occupied(9, 9));
        assert!(hash.is_tile_occupied(11, 11));
        assert!(!hash.is_tile_occupied(12, 12));

        hash.remove(entity);
        assert!(!hash.is_tile_occupied(10, 10));
    }

    #[test]
    fn test_spatial_query_radius() {
        let mut hash = TestSpatialHash::default();
        let e1 = 1u32;
        let e2 = 2u32;

        hash.insert(e1, (10, 10), 0, 0); // At (10, 10)
        hash.insert(e2, (15, 10), 0, 0); // At (15, 10)

        let mut found = Vec::new();
        // Use rect query as substitute; radius 5 corresponds to an 11-tile-wide square.
        hash.query().rect(10 - 5, 10 - 5, 11, 11, |e| {
            found.push(e);
        });

        assert_eq!(found.len(), 2);
        assert!(found.contains(&e1));
        assert!(found.contains(&e2));

        let mut found2 = Vec::new();
        hash.query().rect(10 - 4, 10 - 4, 9, 9, |e| {
            found2.push(e);
        });
        assert_eq!(found2.len(), 1);
        assert!(found2.contains(&e1));
    }

    #[test]
    fn test_spatial_boundary_conditions() {
        let mut hash = TestSpatialHash::default();
        let e1 = 1u32;

        // At the zero boundary
        hash.insert(e1, (0, 0), 1, 0); // Covers (-1,-1) to (1,1). Outside should be ignored by BitBoard logic.
        assert!(hash.is_tile_occupied(0, 0));
        assert!(hash.is_tile_occupied(1, 1));
        assert!(!hash.is_tile_occupied(2, 2));

        hash.remove(e1);

        // At the max boundary (255, 255)
        hash.insert(e1, (255, 255), 1, 0);
        assert!(hash.is_tile_occupied(255, 255));
        assert!(hash.is_tile_occupied(254, 254));
        assert!(!hash.is_tile_occupied(253, 253));
    }

    #[test]
    fn test_spatial_query_circle() {
        let mut hash = TestSpatialHash::default();
        let e1 = 1u32;
        let e2 = 2u32;

        // Placed at exactly 5 tiles and ~5.65 tiles from center (10, 10).
        hash.insert(e1, (10, 15), 0, 0); // distance 5.0 (on the boundary)
        hash.insert(e2, (14, 14), 0, 0); // sqrt(4^2 + 4^2) ≈ 5.65 — outside circle, inside bounding square

        let mut found = Vec::new();
        hash.query().circle((10, 10), 5.0, |e| {
            found.push(e);
        });

        assert!(found.contains(&e1));
        assert!(
            !found.contains(&e2),
            "Corner of the square should be excluded in circular query"
        );
    }

    #[test]
    fn test_spatial_query_sector() {
        let mut hash = TestSpatialHash::default();
        let e1 = 1u32;
        let e2 = 2u32;

        // e1 directly right (0°), e2 directly left (180°) from (10, 10).
        hash.insert(e1, (15, 10), 0, 0); // right (0°)
        hash.insert(e2, (5, 10), 0, 0); // left (180°)

        let mut found = Vec::new();
        // 90° forward cone facing right (-45° to +45°).
        hash.query().sector((10, 10), 10.0, -45.0, 90.0, |e| {
            found.push(e);
        });

        assert!(found.contains(&e1));
        assert!(!found.contains(&e2));
    }

    #[test]
    fn test_spatial_query_composite_proximity() {
        let mut hash = TestSpatialHash::default();
        let ally = 1u32;
        let enemy = 2u32;

        // Ally at (10, 10), enemy at (12, 12).
        hash.insert(ally, (10, 10), 0, 0); // Kind 0: Ally
        hash.insert(enemy, (12, 12), 0, 1); // Kind 1: Enemy

        // Dilate the ally layer by 3 tiles to build a "near ally" mask.
        let proximity_mask = hash.layer(0).dilate(3);

        let mut found = Vec::new();
        // Search for Kind 1 (Enemy) entities within that mask.
        hash.query().with_kind(1).mask(&proximity_mask, |e| {
            found.push(e);
        });

        assert!(found.contains(&enemy));
        assert!(!found.contains(&ally));
    }

    #[test]
    fn test_spatial_update_diff() {
        let mut hash = TestSpatialHash::default();
        let e1 = 1u32;

        // Initial placement.
        hash.insert(e1, (10, 10), 1, 0);
        assert!(hash.is_tile_occupied(10, 10));
        assert!(hash.is_tile_occupied(11, 11));

        // Move to (20, 20) and shrink radius to 0.
        hash.update_diff(e1, (20, 20), 0, 0);

        // Old position should be empty.
        assert!(!hash.is_tile_occupied(10, 10));
        assert!(!hash.is_tile_occupied(11, 11));
        // New position should be occupied.
        assert!(hash.is_tile_occupied(20, 20));
        assert!(!hash.is_tile_occupied(21, 21)); // radius 0: only center tile
    }

    #[test]
    fn test_spatial_query_mask_bounded() {
        let mut hash = TestSpatialHash::default();
        let e1 = 1u32;
        let e2 = 2u32;

        hash.insert(e1, (10, 10), 0, 0);
        hash.insert(e2, (20, 20), 0, 0);

        // Create a full-grid mask, then restrict to (0,0)–(15,15) via AND.
        let mut full_mask = BitBoard::<256, 256, RowMajorLayout>::default();
        full_mask = !full_mask;

        let bounds_mask = BitBoard::<256, 256, RowMajorLayout>::mask_rect(0, 0, 16, 16);
        let combined_mask = &full_mask & &bounds_mask;

        let mut found = Vec::new();
        hash.query().mask(&combined_mask, |e| {
            found.push(e);
        });

        assert_eq!(found.len(), 1);
        assert!(found.contains(&e1));
        assert!(!found.contains(&e2));
    }

    #[test]
    fn test_spatial_static_layers() {
        let mut hash = TestSpatialHash::default();
        let mut wall_map = BitBoard::<256, 256, RowMajorLayout>::default();
        wall_map.set(5, 5, true);

        assert_eq!(hash.static_revision(), 0);
        hash.full_sync_static_layer(0, &wall_map, 1);
        assert_eq!(hash.static_revision(), 1);
        assert!(!hash.is_tile_occupied(5, 5));
    }

    #[test]
    fn test_spatial_update_with_threshold() {
        let mut hash = TestSpatialHash::default();
        let e1 = 1u32;

        hash.insert(e1, (10, 10), 0, 0);

        // Threshold 5; a 2-tile move should be suppressed.
        hash.update_with_threshold(e1, (12, 10), 0, 0, 5);

        let info = hash.entity_info.get(&e1).unwrap();
        assert_eq!(info.center, (10, 10), "Update should be throttled");

        // A 6-tile move exceeds the threshold; update should proceed.
        hash.update_with_threshold(e1, (16, 10), 0, 0, 5);
        let info2 = hash.entity_info.get(&e1).unwrap();
        assert_eq!(info2.center, (16, 10), "Update should be applied");
    }

    #[test]
    fn test_spatial_mask_visibility() {
        let mut hash = TestSpatialHash::default();
        // Place a wall tile at (12, 10) in static layer 0.
        let mut wall_map = BitBoard::<256, 256, RowMajorLayout>::default();
        wall_map.set(12, 10, true);
        hash.full_sync_static_layer(0, &wall_map, 1);

        // Compute visibility from (10, 10) with radius 5.
        // (14, 10) lies behind the wall at (12, 10) and should be invisible.
        let vis = hash.mask_visibility(10, 10, 5.0, 0);

        assert!(vis.get(11, 10));
        assert!(vis.get(12, 10)); // the wall tile itself is visible
        assert!(
            !vis.get(14, 10),
            "Shadowcasting should work through SpatialHash wrapper"
        );
    }

    #[test]
    fn test_spatial_multiple_entities_overlap() {
        let mut hash = TestSpatialHash::default();
        let e1 = 1u32;
        let e2 = 2u32;

        hash.insert(e1, (10, 10), 0, 0);
        hash.insert(e2, (10, 10), 0, 0);

        assert!(hash.is_tile_occupied(10, 10));
        assert_eq!(
            hash.cells[TestBoard::tile_to_index(10, 10).unwrap()].len(),
            2
        );

        hash.remove(e1);
        assert!(hash.is_tile_occupied(10, 10), "Still occupied by e2");
        assert_eq!(
            hash.cells[TestBoard::tile_to_index(10, 10).unwrap()].len(),
            1
        );

        hash.remove(e2);
        assert!(!hash.is_tile_occupied(10, 10));
    }

    #[test]
    fn test_spatial_update_complex() {
        let mut hash = TestSpatialHash::default();
        let e1 = 1u32;

        hash.insert(e1, (10, 10), 1, 0);

        // Move + radius change + kind change in one update_diff call.
        hash.update_diff(e1, (20, 20), 0, 1);

        assert!(!hash.is_tile_occupied(10, 10));
        assert!(hash.is_tile_occupied(20, 20));
        assert!(hash.layer(1).get(20, 20));
        assert!(!hash.layer(0).get(20, 20));
    }

    #[test]
    fn test_spatial_query_exclude_logic() {
        let mut hash = TestSpatialHash::default();
        let e1 = 1u32;
        let e2 = 2u32;

        hash.insert(e1, (10, 10), 0, 0);
        hash.insert(e2, (11, 10), 0, 0);

        let mut found = Vec::new();
        // Search within radius 2, excluding e1.
        hash.query().exclude(e1).circle((10, 10), 2.0, |e| {
            found.push(e);
        });

        assert_eq!(found.len(), 1);
        assert!(found.contains(&e2));
        assert!(!found.contains(&e1));
    }

    #[test]
    fn test_static_area_queries() {
        let mut sh = TestSpatialHash::default();
        let mut board = BitBoard::<256, 256, RowMajorLayout>::default();

        // Set a 5×5 block centered at (20, 20) (radius=2).
        for y in 18..=22 {
            for x in 18..=22 {
                board.set(x, y, true);
            }
        }

        // Sync to static layer 0.
        sh.full_sync_static_layer(0, &board, 1);

        // All tiles within radius 1 and 2 should be set; radius 3 extends beyond the block.
        assert!(sh.is_static_area_all_set(0, 20, 20, 2));
        assert!(sh.is_static_area_all_set(0, 20, 20, 1));
        assert!(sh.is_static_area_any_set(0, 20, 20, 3));

        // Remove one tile and re-check.
        board.set(18, 18, false);
        sh.full_sync_static_layer(0, &board, 2);
        assert!(!sh.is_static_area_all_set(0, 20, 20, 2));
        assert!(sh.is_static_area_any_set(0, 20, 20, 2));
    }

    #[test]
    fn test_spatial_consistency_audit() {
        let mut hash = TestSpatialHash::default();
        let e1 = 1u32;
        hash.insert(e1, (50, 50), 2, 0);

        let mask = TestBoard::mask_rect(50 - 2, 50 - 2, 5, 5);
        assert_eq!(hash.presence.count_ones(), mask.count_ones());

        for (x, y) in hash.presence.iter_set_bits() {
            let idx = TestBoard::tile_to_index(x, y).unwrap();
            assert!(hash.cells[idx].iter().any(|&(id, _)| id == e1));
        }
    }

    #[test]
    fn test_spatial_out_of_bounds_movement() {
        let mut hash = TestSpatialHash::default();
        let e1 = 1u32;

        // Inside
        hash.insert(e1, (10, 10), 0, 0);
        assert!(hash.is_tile_occupied(10, 10));

        // Move completely outside (negative)
        hash.update_diff(e1, (-100, -100), 0, 0);
        assert!(!hash.is_tile_occupied(10, 10));
        assert!(hash.entity_info.get(&e1).unwrap().center == (-100, -100));
        assert!(hash.presence.is_empty());

        // Move partially inside (edge)
        hash.update_diff(e1, (0, 0), 2, 0); // Covers (-2, -2) to (2, 2)
        assert!(hash.is_tile_occupied(0, 0));
        assert!(hash.is_tile_occupied(2, 2));
    }

    #[test]
    fn test_spatial_update_kind_change_consistency() {
        let mut hash = TestSpatialHash::default();
        let e1 = 1u32;

        hash.insert(e1, (10, 10), 0, 0); // Kind 0
        assert!(hash.layer(0).get(10, 10));
        assert!(!hash.layer(1).get(10, 10));

        hash.update_diff(e1, (10, 10), 0, 1); // Kind 1
        assert!(!hash.layer(0).get(10, 10));
        assert!(hash.layer(1).get(10, 10));
    }

    #[test]
    fn test_spatial_multiple_entities_same_kind_removal() {
        let mut hash = TestSpatialHash::default();
        let e1 = 1u32;
        let e2 = 2u32;

        hash.insert(e1, (10, 10), 0, 0); // Kind 0
        hash.insert(e2, (10, 10), 0, 0); // Kind 0

        assert!(hash.layer(0).get(10, 10));

        hash.remove(e1);
        assert!(
            hash.layer(0).get(10, 10),
            "Bit should remain since e2 is still Kind 0 at this tile"
        );

        hash.remove(e2);
        assert!(!hash.layer(0).get(10, 10), "Bit should be cleared now");
    }

    #[test]
    fn test_spatial_query_empty_mask() {
        let mut hash = TestSpatialHash::default();
        hash.insert(1, (10, 10), 0, 0);

        let empty_mask = BitBoard::<256, 256>::new();
        let mut found = Vec::new();
        hash.query().mask(&empty_mask, |e| found.push(e));

        assert!(found.is_empty());
    }

    #[test]
    fn test_spatial_id_reuse() {
        let mut hash = TestSpatialHash::default();
        let e1 = 1u32;

        hash.insert(e1, (10, 10), 0, 0);
        hash.remove(e1);

        hash.insert(e1, (20, 20), 0, 0);
        assert!(!hash.is_tile_occupied(10, 10));
        assert!(hash.is_tile_occupied(20, 20));
    }

    // ─── Edge cases: update_diff branch coverage ──────────────────────────

    #[test]
    fn update_diff_is_no_op_when_nothing_changed() {
        let mut h = TestSpatialHash::default();
        h.insert(1u32, (10, 10), 1, 0);
        let presence_before = h.presence.count_ones();
        let cells_before = h.cells[TestBoard::tile_to_index(10, 10).unwrap()].len();

        // center, radius, and kind are all identical — must be a no-op.
        h.update_diff(1u32, (10, 10), 1, 0);

        assert_eq!(h.presence.count_ones(), presence_before);
        assert_eq!(
            h.cells[TestBoard::tile_to_index(10, 10).unwrap()].len(),
            cells_before
        );
    }

    #[test]
    fn update_diff_radius_only_change_goes_through_remove_insert() {
        let mut h = TestSpatialHash::default();
        h.insert(1u32, (10, 10), 1, 0);
        // Same center, radius changes from 1 to 2.
        h.update_diff(1u32, (10, 10), 2, 0);
        // The 5×5 area (radius 2) should now be occupied.
        assert!(h.is_tile_occupied(8, 8));
        assert!(h.is_tile_occupied(12, 12));
        // Old 3×3 corners are still inside the new 5×5 region.
        assert!(h.is_tile_occupied(11, 11));
        let info = h.entity_info.get(&1u32).unwrap();
        assert_eq!(info.radius, 2);
    }

    #[test]
    fn update_diff_kind_only_change_swaps_layer() {
        let mut h = TestSpatialHash::default();
        h.insert(1u32, (10, 10), 0, 0); // Kind 0
        assert!(h.layer(0).get(10, 10));
        assert!(!h.layer(1).get(10, 10));

        h.update_diff(1u32, (10, 10), 0, 1); // Change to Kind 1

        assert!(!h.layer(0).get(10, 10), "old kind bit should be cleared");
        assert!(h.layer(1).get(10, 10), "new kind bit should be set");
        assert_eq!(h.entity_info.get(&1u32).unwrap().kind_idx, 1);
    }

    #[test]
    fn update_diff_handles_disjoint_movement() {
        // Old and new regions are fully disjoint, exercising the "no overlap"
        // branch of for_each_rect_diff (full old-remove + full new-insert).
        let mut h = TestSpatialHash::default();
        h.insert(1u32, (10, 10), 1, 0);
        assert!(h.is_tile_occupied(10, 10));
        assert!(h.is_tile_occupied(11, 11));

        // Only center moves (radius/kind unchanged) — takes path 4.
        h.update_diff(1u32, (100, 100), 1, 0);

        // Old region should be fully vacated.
        assert!(!h.is_tile_occupied(10, 10));
        assert!(!h.is_tile_occupied(11, 11));
        // New region should be occupied.
        assert!(h.is_tile_occupied(100, 100));
        assert!(h.is_tile_occupied(101, 101));
    }

    // ─── Edge cases: insert / remove boundary behavior ───────────────────

    #[test]
    fn insert_overrides_existing_entity_info_but_leaves_stale_cells() {
        // Inserting the same ID twice overwrites entity_info but leaves stale cell
        // entries; callers should call remove before re-inserting.
        let mut h = TestSpatialHash::default();
        h.insert(1u32, (10, 10), 0, 0);
        h.insert(1u32, (20, 20), 0, 0);

        // entity_info reflects the new position.
        assert_eq!(h.entity_info.get(&1u32).unwrap().center, (20, 20));
        // Old cell still has the stale registration (known limitation).
        let old_idx = TestBoard::tile_to_index(10, 10).unwrap();
        assert!(h.cells[old_idx].iter().any(|&(id, _)| id == 1u32));
    }

    #[test]
    fn remove_unknown_entity_is_idempotent() {
        let mut h = TestSpatialHash::default();
        // Removing an unregistered ID must not panic and must be a no-op.
        h.remove(42u32);
        h.remove(42u32);
        assert!(h.presence.is_empty());
    }

    #[test]
    fn insert_with_negative_radius_is_silent_no_op_for_cells() {
        // Negative radius: mask_rect returns an empty mask, so cells are unaffected.
        let mut h = TestSpatialHash::default();
        h.insert(1u32, (10, 10), -1, 0);
        // entity_info is registered but the presence bitmap stays empty.
        assert!(h.entity_info.contains_key(&1u32));
        assert!(h.presence.is_empty());
    }

    #[test]
    fn insert_with_huge_radius_does_not_panic() {
        // A radius larger than the board is clipped by mask_rect and must not panic.
        let mut h = TestSpatialHash::default();
        h.insert(1u32, (128, 128), 500, 0);
        assert!(h.entity_info.contains_key(&1u32));
        // At minimum the center tile should be occupied.
        assert!(h.is_tile_occupied(128, 128));
        // remove should also work cleanly.
        h.remove(1u32);
        assert!(h.presence.is_empty());
    }

    // ─── Edge cases: query builder ───────────────────────────────────────

    #[test]
    fn query_with_kind_mask_zero_yields_no_results() {
        let mut h = TestSpatialHash::default();
        h.insert(1u32, (10, 10), 0, 0);
        h.insert(2u32, (10, 10), 0, 1);

        // mask=0 matches no kind — result must be empty.
        let mut found = Vec::new();
        h.query()
            .with_kind_mask(0)
            .circle((10, 10), 5.0, |id| found.push(id));
        assert!(found.is_empty());
    }

    #[test]
    fn query_with_kind_repeated_keeps_last_call() {
        let mut h = TestSpatialHash::default();
        h.insert(1u32, (10, 10), 0, 0);
        h.insert(2u32, (10, 10), 0, 1);

        // Chaining with_kind twice keeps only the last call (kind 1).
        let mut found = Vec::new();
        h.query()
            .with_kind(0)
            .with_kind(1)
            .circle((10, 10), 5.0, |id| found.push(id));
        assert_eq!(found, vec![2u32]);
    }

    // ─── Edge cases: static layer cache consistency ───────────────────────

    #[test]
    fn update_static_tile_does_not_refresh_eroded_cache() {
        // Known invariant: update_static_tile only updates static_layers; eroded_layers
        // remain stale until full_sync_static_layer is called.
        let mut h = TestSpatialHash::default();
        let mut board = BitBoard::<256, 256, RowMajorLayout>::default();
        // Build a 5×5 passable region.
        for y in 18..=22 {
            for x in 18..=22 {
                board.set(x, y, true);
            }
        }
        h.full_sync_static_layer(0, &board, 1);
        // All tiles within radius 1 of (20, 20) should be set.
        assert!(h.is_static_area_all_set(0, 20, 20, 1));

        // Punch a single hole via partial update.
        h.update_static_tile(0, 20, 20, false, 2);
        // The raw layer sees the change...
        assert!(!h.static_layer(0).get(20, 20));
        // ...but the eroded cache still reflects the old state (known limitation).
        assert!(
            h.is_static_area_all_set(0, 20, 20, 1),
            "eroded cache is not refreshed by update_static_tile"
        );

        // After full_sync, the cache is up to date.
        h.full_sync_static_layer(0, &h.static_layer(0).clone(), 3);
        assert!(!h.is_static_area_all_set(0, 20, 20, 1));
    }

    #[test]
    fn full_sync_with_invalid_layer_idx_is_silent_no_op() {
        let mut h = TestSpatialHash::default();
        let board = BitBoard::<256, 256, RowMajorLayout>::default();
        // S=5, so layer_idx=10 is out of range — must be silently ignored.
        h.full_sync_static_layer(10, &board, 1);
        assert_eq!(
            h.static_revision(),
            0,
            "invalid layer_idx must not update the revision"
        );
    }

    // ─── Edge cases: generic ID type ────────────────────────────────────

    #[test]
    fn spatial_hash_works_with_u64_id() {
        type U64Hash = SpatialHash<u64, 256, 256, 2, 1>;
        let mut h = U64Hash::default();
        let id = 0xDEAD_BEEF_CAFE_BABE_u64;
        h.insert(id, (10, 10), 0, 0);
        assert!(h.is_tile_occupied(10, 10));
        h.remove(id);
        assert!(!h.is_tile_occupied(10, 10));
    }
}
