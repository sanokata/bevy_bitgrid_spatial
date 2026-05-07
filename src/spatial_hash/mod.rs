use ahash::RandomState;
use core::hash::Hash;
use hashbrown::HashMap;
use bitgrid::{BitBoard, BitLayout, RowMajorLayout};
use smallvec::SmallVec;
use std::marker::PhantomData;

#[cfg(feature = "bevy")]
use bevy::prelude::*;

mod entity;
pub mod query;
mod static_layer;

pub(crate) use entity::EntityEntry;
pub use query::SectorArgs;

type CellStorage<ID> = Box<[SmallVec<[(ID, u8); 4]>]>;

/// タイル座標ベースのエンティティ位置を管理する空間ハッシュ (汎用版)
/// ID: エンティティを識別する型 (Entity, u32, etc)
/// const E: エンティティ種別の数 (Dynamic layers)
/// const S: 静的レイヤーの数 (Static layers like Terrain)
/// L: メモリレイアウト
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
    /// セル管理（y * W + x でアクセス）。(ID, KindIdx) のペアで保持。
    pub(crate) cells: CellStorage<ID>,
    /// エンティティの管理情報（履歴保持・削除用）
    pub(crate) entity_info: HashMap<ID, EntityEntry, RandomState>,
    /// 存在判定用のビットマップ
    pub(crate) presence: BitBoard<W, H, L>,
    /// 種別ごとの高速存在判定ビットマップ (Eレイヤー)
    pub(crate) kind_boards: [BitBoard<W, H, L>; E],
    /// 静的なレイヤー（地形等）のコピー (Sレイヤー)
    pub(crate) static_layers: [BitBoard<W, H, L>; S],
    /// 静的なレイヤーの収縮済みキャッシュ。インデックスは
    /// `[layer][CACHED_EROSION_RADII の位置]`（static_layer.rs 参照）。
    pub(crate) eroded_layers: [[BitBoard<W, H, L>; 2]; S],
    /// 静的レイヤーの変更検知トークン。値そのものに意味はなく、外部（TileMap 等）の
    /// 単調増加カウンタを `full_sync_static_layer` 経由で受け取り、`!=` 比較で
    /// 再同期要否を判定する。`u32::wrapping_add` で wrap しても呼び出し側で
    /// `wrapping_sub > 1000` のような差分判定をすれば大幅な乖離を検出可能。
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

    // テスト用にダミーのIDとして u32 を使用
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
        // 矩形クエリで代用 (radius 5 = width 11)
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

        // (10, 10) から距離 5 の位置に配置
        hash.insert(e1, (10, 15), 0, 0); // 距離 5.0 (ちょうど)
        hash.insert(e2, (14, 14), 0, 0); // 距離 sqrt(4^2 + 4^2) = 5.65 (円の外だが正方形の内)

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

        // (10, 10) から右方向に e1, 左方向に e2
        hash.insert(e1, (15, 10), 0, 0); // 右 (0度)
        hash.insert(e2, (5, 10), 0, 0); // 左 (180度)

        let mut found = Vec::new();
        // 右向き 90度の視界 ( -45度 〜 45度 )
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

        // 味方を (10, 10) に、敵を (12, 12) に配置
        hash.insert(ally, (10, 10), 0, 0); // Kind 0: Ally
        hash.insert(enemy, (12, 12), 0, 1); // Kind 1: Enemy

        // 味方レイヤーを 3マス膨張させて「味方の周囲」マスクを作成
        let proximity_mask = hash.layer(0).dilate(3);

        let mut found = Vec::new();
        // マスク内かつ Kind 1 (Enemy) のエンティティを検索
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

        // 初期配置
        hash.insert(e1, (10, 10), 1, 0);
        assert!(hash.is_tile_occupied(10, 10));
        assert!(hash.is_tile_occupied(11, 11));

        // 移動と半径変更
        hash.update_diff(e1, (20, 20), 0, 0);

        // 古い場所は消えているはず
        assert!(!hash.is_tile_occupied(10, 10));
        assert!(!hash.is_tile_occupied(11, 11));
        // 新しい場所が占有されている
        assert!(hash.is_tile_occupied(20, 20));
        assert!(!hash.is_tile_occupied(21, 21)); // 半径0なので
    }

    #[test]
    fn test_spatial_query_mask_bounded() {
        let mut hash = TestSpatialHash::default();
        let e1 = 1u32;
        let e2 = 2u32;

        hash.insert(e1, (10, 10), 0, 0);
        hash.insert(e2, (20, 20), 0, 0);

        // 画面全体を覆うマスクを作成
        let mut full_mask = BitBoard::<256, 256, RowMajorLayout>::default();
        full_mask = !full_mask;

        let mut found = Vec::new();
        // 範囲を (0,0) ~ (15,15) に限定して検索 (現在は bounded クエリがビルダーにないが、マスクとの積で代用可能)
        let bounds_mask = BitBoard::<256, 256, RowMajorLayout>::mask_rect(0, 0, 16, 16);
        let combined_mask = &full_mask & &bounds_mask;
        
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

        // しきい値 5。距離 2 の移動ならスキップされるはず
        hash.update_with_threshold(e1, (12, 10), 0, 0, 5);

        let info = hash.entity_info.get(&e1).unwrap();
        assert_eq!(info.center, (10, 10), "Update should be throttled");

        // 距離 6 の移動なら更新されるはず
        hash.update_with_threshold(e1, (16, 10), 0, 0, 5);
        let info2 = hash.entity_info.get(&e1).unwrap();
        assert_eq!(info2.center, (16, 10), "Update should be applied");
    }

    #[test]
    fn test_spatial_mask_visibility() {
        let mut hash = TestSpatialHash::default();
        // 5x5 の位置に壁を設置 (静的レイヤー 0)
        let mut wall_map = BitBoard::<256, 256, RowMajorLayout>::default();
        wall_map.set(12, 10, true);
        hash.full_sync_static_layer(0, &wall_map, 1);

        // (10, 10) から半径 5 で視界を計算
        // (12, 10) の壁の向こう側 (14, 10) は見えないはず
        let vis = hash.mask_visibility(10, 10, 5.0, 0);

        assert!(vis.get(11, 10));
        assert!(vis.get(12, 10)); // 壁自体は見えている
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

        // 移動 + 半径変更 + 種別変更
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
        // e1 を除外して検索
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
        
        // (20, 20) を中心に 5x5 (radius=2) をセット
        for y in 18..=22 {
            for x in 18..=22 {
                board.set(x, y, true);
            }
        }
        
        // 静的レイヤー 0 に同期
        sh.full_sync_static_layer(0, &board, 1);
        
        // 判定
        assert!(sh.is_static_area_all_set(0, 20, 20, 2));
        assert!(sh.is_static_area_all_set(0, 20, 20, 1));
        assert!(sh.is_static_area_any_set(0, 20, 20, 3));
        
        // 一部削除して再判定
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

    // ─── エッジケース: update_diff の各分岐 ─────────────────────────

    #[test]
    fn update_diff_is_no_op_when_nothing_changed() {
        let mut h = TestSpatialHash::default();
        h.insert(1u32, (10, 10), 1, 0);
        let presence_before = h.presence.count_ones();
        let cells_before = h.cells[TestBoard::tile_to_index(10, 10).unwrap()].len();

        // center / radius / kind が完全一致 → 何もしない
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
        // center 同じ、radius のみ 1 → 2
        h.update_diff(1u32, (10, 10), 2, 0);
        // 半径 2 の範囲（5x5）が新たに占有されている
        assert!(h.is_tile_occupied(8, 8));
        assert!(h.is_tile_occupied(12, 12));
        // 古い 3x3 領域内の角は新範囲にも含まれるので true のまま
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

        h.update_diff(1u32, (10, 10), 0, 1); // Kind 1 へ変更

        assert!(!h.layer(0).get(10, 10), "旧 kind の bit はクリアされる");
        assert!(h.layer(1).get(10, 10), "新 kind の bit が立つ");
        assert_eq!(h.entity_info.get(&1u32).unwrap().kind_idx, 1);
    }

    #[test]
    fn update_diff_handles_disjoint_movement() {
        // 旧範囲と新範囲が完全に離れている場合、for_each_rect_diff の
        // 「交差なし」分岐を通って old 全削除 + new 全挿入になる
        let mut h = TestSpatialHash::default();
        h.insert(1u32, (10, 10), 1, 0);
        assert!(h.is_tile_occupied(10, 10));
        assert!(h.is_tile_occupied(11, 11));

        // center のみ動かす（radius/kind 同じ）→ 経路 4 を通る
        h.update_diff(1u32, (100, 100), 1, 0);

        // 旧範囲は完全に空
        assert!(!h.is_tile_occupied(10, 10));
        assert!(!h.is_tile_occupied(11, 11));
        // 新範囲が占有されている
        assert!(h.is_tile_occupied(100, 100));
        assert!(h.is_tile_occupied(101, 101));
    }

    // ─── エッジケース: insert / remove の境界 ───────────────────────

    #[test]
    fn insert_overrides_existing_entity_info_but_leaves_stale_cells() {
        // 同じ ID を 2 回 insert すると entity_info は上書きだが、
        // cells には旧登録が残る（呼び出し側で remove してから insert すべき）
        let mut h = TestSpatialHash::default();
        h.insert(1u32, (10, 10), 0, 0);
        h.insert(1u32, (20, 20), 0, 0);

        // entity_info は新位置を指す
        assert_eq!(h.entity_info.get(&1u32).unwrap().center, (20, 20));
        // 旧 cell には古い登録が残っている（既知の制限）
        let old_idx = TestBoard::tile_to_index(10, 10).unwrap();
        assert!(h.cells[old_idx].iter().any(|&(id, _)| id == 1u32));
    }

    #[test]
    fn remove_unknown_entity_is_idempotent() {
        let mut h = TestSpatialHash::default();
        // 未登録 ID の remove はパニックせず何もしない
        h.remove(42u32);
        h.remove(42u32);
        assert!(h.presence.is_empty());
    }

    #[test]
    fn insert_with_negative_radius_is_silent_no_op_for_cells() {
        // 負の radius を渡すと mask_rect が空マスクを返し、cells は空のまま
        let mut h = TestSpatialHash::default();
        h.insert(1u32, (10, 10), -1, 0);
        // entity_info には登録されるが cells は空
        assert!(h.entity_info.contains_key(&1u32));
        assert!(h.presence.is_empty());
    }

    #[test]
    fn insert_with_huge_radius_does_not_panic() {
        // ボード幅以上の半径でも mask_rect 側でクリップされるため動作する
        let mut h = TestSpatialHash::default();
        h.insert(1u32, (128, 128), 500, 0);
        assert!(h.entity_info.contains_key(&1u32));
        // ボード全域が占有されている（少なくとも中央は確実）
        assert!(h.is_tile_occupied(128, 128));
        // remove も問題なく動く
        h.remove(1u32);
        assert!(h.presence.is_empty());
    }

    // ─── エッジケース: クエリビルダー ──────────────────────────────

    #[test]
    fn query_with_kind_mask_zero_yields_no_results() {
        let mut h = TestSpatialHash::default();
        h.insert(1u32, (10, 10), 0, 0);
        h.insert(2u32, (10, 10), 0, 1);

        // mask=0 はどの kind にも一致しない → 結果は空
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

        // with_kind を 2 回チェーン → 最後の (1) が有効
        let mut found = Vec::new();
        h.query()
            .with_kind(0)
            .with_kind(1)
            .circle((10, 10), 5.0, |id| found.push(id));
        assert_eq!(found, vec![2u32]);
    }

    // ─── エッジケース: 静的レイヤーのキャッシュ整合性 ──────────────

    #[test]
    fn update_static_tile_does_not_refresh_eroded_cache() {
        // 既知の不変条件: update_static_tile は static_layers のみ更新し、
        // eroded_layers は古いままになる。半径 1〜2 のキャッシュ済みクエリは
        // この事実を踏まえて呼び出し側で full_sync を使うべき。
        let mut h = TestSpatialHash::default();
        let mut board = BitBoard::<256, 256, RowMajorLayout>::default();
        // 5x5 の通行可能領域を初期構築
        for y in 18..=22 {
            for x in 18..=22 {
                board.set(x, y, true);
            }
        }
        h.full_sync_static_layer(0, &board, 1);
        // 半径 1 で all_set のはず
        assert!(h.is_static_area_all_set(0, 20, 20, 1));

        // 1 タイルだけ穴を空ける（部分更新）
        h.update_static_tile(0, 20, 20, false, 2);
        // 直接 layer に問い合わせると false を返すが…
        assert!(!h.static_layer(0).get(20, 20));
        // …半径 1 のキャッシュは古い「all set」を返す（既知の制限）
        assert!(
            h.is_static_area_all_set(0, 20, 20, 1),
            "eroded キャッシュは update_static_tile では更新されない"
        );

        // full_sync 後はキャッシュが追従
        h.full_sync_static_layer(0, &h.static_layer(0).clone(), 3);
        assert!(!h.is_static_area_all_set(0, 20, 20, 1));
    }

    #[test]
    fn full_sync_with_invalid_layer_idx_is_silent_no_op() {
        let mut h = TestSpatialHash::default();
        let board = BitBoard::<256, 256, RowMajorLayout>::default();
        // S=5 なので layer_idx=10 は無効
        h.full_sync_static_layer(10, &board, 1);
        assert_eq!(
            h.static_revision(),
            0,
            "無効 layer_idx ではリビジョンも更新されない"
        );
    }

    // ─── エッジケース: ジェネリック ID 型 ──────────────────────────

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
