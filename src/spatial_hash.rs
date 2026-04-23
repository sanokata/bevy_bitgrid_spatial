use hashbrown::HashMap;
use ahash::RandomState;
use core::hash::Hash;
use smallvec::SmallVec;
use lexaos_bitboard::{BitBoard, BitLayout, RowMajorLayout};
use std::marker::PhantomData;

#[cfg(feature = "bevy")]
use bevy::prelude::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct EntityEntry {
    center: (i32, i32),
    radius: i32,
    kind_idx: usize,
}

/// タイル座標ベースのエンティティ位置を管理する空間ハッシュ (汎用版)
/// ID: エンティティを識別する型 (Entity, u32, etc)
/// const E: エンティティ種別の数 (Dynamic layers)
/// const S: 静的レイヤーの数 (Static layers like Terrain)
/// L: メモリレイアウト
#[cfg_attr(feature = "bevy", derive(Resource))]
pub struct SpatialHash<ID, const W: usize, const H: usize, const E: usize, const S: usize, L = RowMajorLayout> 
where ID: Copy + Eq + Hash, L: BitLayout<W, H>
{
    /// セル管理（y * W + x でアクセス）。(ID, KindIdx) のペアで保持。
    cells: Box<[SmallVec<[(ID, u8); 4]>]>,
    /// エンティティの管理情報（履歴保持・削除用）
    entity_info: HashMap<ID, EntityEntry, RandomState>,
    /// 存在判定用のビットマップ
    presence: BitBoard<W, H, L>,
    /// 種別ごとの高速存在判定ビットマップ (Eレイヤー)
    kind_boards: [BitBoard<W, H, L>; E],
    /// 地形などの静的レイヤーのコピー (Sレイヤー)
    static_layers: [BitBoard<W, H, L>; S],
    /// 静的レイヤーの同期用リビジョン
    static_revision: u32,
    _layout: PhantomData<L>,
}

impl<ID, const W: usize, const H: usize, const E: usize, const S: usize, L: BitLayout<W, H>> Default
    for SpatialHash<ID, W, H, E, S, L>
where ID: Copy + Eq + Hash
{
    fn default() -> Self {
        let total_cells = L::total_words() * 64;
        let cells = vec![SmallVec::new(); total_cells].into_boxed_slice();
        Self {
            cells,
            entity_info: HashMap::with_hasher(RandomState::default()),
            presence: BitBoard::default(),
            kind_boards: std::array::from_fn(|_| BitBoard::default()),
            static_layers: std::array::from_fn(|_| BitBoard::default()),
            static_revision: 0,
            _layout: PhantomData,
        }
    }
}

impl<ID, const W: usize, const H: usize, const E: usize, const S: usize, L: BitLayout<W, H>> SpatialHash<ID, W, H, E, S, L> 
where ID: Copy + Eq + Hash
{
    /// 指定インデックスのエンティティレイヤーを取得
    #[inline(always)]
    pub fn layer(&self, kind_idx: usize) -> &BitBoard<W, H, L> {
        &self.kind_boards[kind_idx]
    }

    /// 指定インデックスの静的レイヤーを取得
    #[inline(always)]
    pub fn static_layer(&self, layer_idx: usize) -> &BitBoard<W, H, L> {
        &self.static_layers[layer_idx]
    }

    /// 静的レイヤー全体を一括更新し、リビジョンを上げる
    pub fn full_sync_static_layer(
        &mut self,
        layer_idx: usize,
        board: &BitBoard<W, H, L>,
        revision: u32,
    ) {
        if layer_idx < S {
            self.static_layers[layer_idx] = board.clone();
            self.static_revision = revision;
        }
    }

    pub fn static_revision(&self) -> u32 {
        self.static_revision
    }

    #[inline(always)]
    fn layer_mut(&mut self, kind_idx: usize) -> &mut BitBoard<W, H, L> {
        &mut self.kind_boards[kind_idx]
    }


    /// エンティティの各セルへの登録内容を差分更新 (最適化版)
    pub fn update_diff(
        &mut self,
        id: ID,
        new_center: (i32, i32),
        new_radius: i32,
        new_kind_idx: usize,
    ) {
        let old_info = if let Some(info) = self.entity_info.get(&id) {
            // 位置も半径も種別も変わっていなければ何もしない
            if info.center == new_center
                && info.radius == new_radius
                && info.kind_idx == new_kind_idx
            {
                return;
            }
            // 半径または種別が変わった場合は、マスク形状が変わるため単純な差分更新ではなく再登録
            if info.radius != new_radius || info.kind_idx != new_kind_idx {
                self.remove(id);
                self.insert(id, new_center, new_radius, new_kind_idx);
                return;
            }
            *info
        } else {
            self.insert(id, new_center, new_radius, new_kind_idx);
            return;
        };

        let old_center = old_info.center;
        let radius = new_radius;
        let kind_idx = new_kind_idx;

        // 1. BitBoard を用いて新旧のマスクを作成し、差分ビット（追加・削除すべき座標）を抽出
        // 矩形範囲を扱うため mask_rect を使用
        let old_mask = BitBoard::<W, H, L>::mask_rect(
            old_center.0 - radius,
            old_center.1 - radius,
            radius * 2 + 1,
            radius * 2 + 1,
        );
        let new_mask = BitBoard::<W, H, L>::mask_rect(
            new_center.0 - radius,
            new_center.1 - radius,
            radius * 2 + 1,
            radius * 2 + 1,
        );

        // 消去すべき座標: 旧にあって新にないビット
        let remove_mask = &old_mask & &!&new_mask;
        for (x, y) in remove_mask.iter_set_bits() {
            self.cell_remove(x, y, id, kind_idx);
        }

        // 追加すべき座標: 新にあって旧にないビット
        let insert_mask = &new_mask & &!&old_mask;
        for (x, y) in insert_mask.iter_set_bits() {
            self.cell_insert(x, y, id, kind_idx);
        }

        // 管理情報の位置を更新
        if let Some(info) = self.entity_info.get_mut(&id) {
            info.center = new_center;
        }
    }

    /// しきい値ベースのスロットリング更新。
    /// 前回の更新位置から threshold (タイル単位) 未満の移動であれば、セル跨ぎがない限り更新をスキップ。
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
            
            // 半径や種別が変わっておらず、かつ移動距離がしきい値未満ならスキップ
            if dx < threshold && dy < threshold 
               && info.radius == new_radius 
               && info.kind_idx == new_kind_idx 
            {
                // セル境界を跨いでいないかチェック（半径が1以上の場合はより厳密な判定が必要だが、
                // ここでは単純な位置ベースのスロットリングとする）
                return;
            }
        }
        
        self.update_diff(id, new_center, new_radius, new_kind_idx);
    }

    fn cell_insert(&mut self, x: i32, y: i32, id: ID, kind_idx: usize) {
        if let Some(idx) = BitBoard::<W, H, L>::tile_to_index(x, y) {
            self.cells[idx].push((id, kind_idx as u8));
            self.presence.set(x, y, true);
            self.layer_mut(kind_idx).set(x, y, true);
        }
    }

    fn cell_remove(&mut self, x: i32, y: i32, id: ID, kind_idx: usize) {
        if let Some(idx) = BitBoard::<W, H, L>::tile_to_index(x, y) {
            let list = &mut self.cells[idx];
            if let Some(pos) = list.iter().position(|&(e, _)| e == id) {
                list.swap_remove(pos);
            }
            if list.is_empty() {
                self.presence.set(x, y, false);
            }
            
            // list 内の種別情報だけで BitBoard 更新判定が可能
            let has_same_kind = list.iter().any(|&(_, k)| k == kind_idx as u8);
            if !has_same_kind {
                self.layer_mut(kind_idx).set(x, y, false);
            }
        }
    }

    pub fn insert(&mut self, id: ID, tile_pos: (i32, i32), radius: i32, kind_idx: usize) {
        // 1. 中心点から dilate を用いて一括でマスクを生成 (O(log radius))
        let mut mask = BitBoard::<W, H, L>::default();
        mask.set(tile_pos.0, tile_pos.1, true);
        let mask = if radius > 0 {
            mask.dilate(radius as u32)
        } else {
            mask
        };

        // 2. BitBoard の一括 OR 更新 (SIMD最適化の恩恵)
        self.presence |= &mask;
        *self.layer_mut(kind_idx) |= &mask;

        // 3. セル情報への登録 (iter_set_bits により、セットされたビットのみを効率的に走査)
        for (x, y) in mask.iter_set_bits() {
            if let Some(idx) = BitBoard::<W, H, L>::tile_to_index(x, y) {
                self.cells[idx].push((id, kind_idx as u8));
            }
        }

        // 4. 管理情報の保存
        self.entity_info.insert(
            id,
            EntityEntry {
                center: tile_pos,
                radius,
                kind_idx,
            },
        );
    }

    pub fn remove(&mut self, id: ID) {
        if let Some(entry) = self.entity_info.remove(&id) {
            let mask = BitBoard::<W, H, L>::mask_rect(
                entry.center.0 - entry.radius,
                entry.center.1 - entry.radius,
                entry.radius * 2 + 1,
                entry.radius * 2 + 1,
            );
            for (x, y) in mask.iter_set_bits() {
                self.cell_remove(x, y, id, entry.kind_idx);
            }
        }
    }

    pub fn update(
        &mut self,
        id: ID,
        new_tile_pos: (i32, i32),
        radius: i32,
        kind_idx: usize,
    ) {
        self.update_diff(id, new_tile_pos, radius, kind_idx);
    }

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

    /// 既存のバッファを使用して視界マスクを計算（アロケーションフリー）
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

    /// 円形範囲内のエンティティを検索 (正確な半径判定)
    pub fn query_circle_callback<F>(
        &self,
        center: (i32, i32),
        radius: f32,
        kind_idx: Option<usize>,
        exclude: ID,
        mut callback: F,
    ) where
        F: FnMut(ID),
    {
        // 1. 円形マスクの作成
        let circle_mask = BitBoard::<W, H, L>::mask_sector(center.0, center.1, radius, 0.0, 360.0);
        
        // 2. 対象となるレイヤーボードを選択
        let target_board = match kind_idx {
            Some(k) => self.layer(k),
            None => &self.presence,
        };
        let kind_u8 = kind_idx.map(|k| k as u8);

        // 3. マスクとの積集合を高速走査
        circle_mask.for_each_overlap(target_board, |_x, _y, idx| {
            for &(e, k) in &self.cells[idx] {
                if e != exclude {
                    if kind_u8.map_or(true, |target_k| k == target_k) {
                        callback(e);
                    }
                }
            }
        });
    }

    /// 扇形範囲内のエンティティを検索 (視界コーン等に使用)
    pub fn query_sector_callback<F>(
        &self,
        center: (i32, i32),
        radius: f32,
        start_angle_deg: f32,
        sweep_angle_deg: f32,
        kind_idx: Option<usize>,
        exclude: ID,
        mut callback: F,
    ) where
        F: FnMut(ID),
    {
        let sector_mask = BitBoard::<W, H, L>::mask_sector(center.0, center.1, radius, start_angle_deg, sweep_angle_deg);
        
        let target_board = match kind_idx {
            Some(k) => self.layer(k),
            None => &self.presence,
        };
        let kind_u8 = kind_idx.map(|k| k as u8);

        sector_mask.for_each_overlap(target_board, |_x, _y, idx| {
            for &(e, k) in &self.cells[idx] {
                if e != exclude {
                    if kind_u8.map_or(true, |target_k| k == target_k) {
                        callback(e);
                    }
                }
            }
        });
    }

    /// 矩形（正方形）範囲内のエンティティを検索 (レガシー/互換用)
    pub fn query_filtered_radius_callback<F>(
        &self,
        center: (i32, i32),
        radius: i32,
        exclude: ID,
        kind_idx: Option<usize>,
        mut callback: F,
    ) where
        F: FnMut(ID),
    {
        let target_board = match kind_idx {
            Some(k) => self.layer(k),
            None => &self.presence,
        };
        let kind_u8 = kind_idx.map(|k| k as u8);

        let min_tile = (center.0 - radius, center.1 - radius);
        let max_tile = (center.0 + radius, center.1 + radius);

        // BitBoard::for_each_overlap_in を活用
        // 自分自身との積集合を指定範囲で取ることで、指定範囲のセットビットを走査
        target_board.for_each_overlap_in(target_board, min_tile, max_tile, |_x, _y, idx| {
            for &(e, k) in &self.cells[idx] {
                if e != exclude {
                    if kind_u8.map_or(true, |target_k| k == target_k) {
                        callback(e);
                    }
                }
            }
        });
    }

    /// 任意のマスクと種別（任意）でエンティティを検索
    pub fn query_mask_callback<F>(
        &self,
        mask: &BitBoard<W, H, L>,
        kind_idx: Option<usize>,
        exclude: ID,
        mut callback: F,
    ) where
        F: FnMut(ID),
    {
        let target_board = match kind_idx {
            Some(k) => self.layer(k),
            None => &self.presence,
        };
        let kind_u8 = kind_idx.map(|k| k as u8);

        mask.for_each_overlap(target_board, |_x, _y, idx| {
            for &(e, k) in &self.cells[idx] {
                if e != exclude && kind_u8.map_or(true, |tk| k == tk) {
                    callback(e);
                }
            }
        });
    }

    /// 任意のマスク、種別（任意）、および範囲制限でエンティティを検索
    pub fn query_mask_bounded_callback<F>(
        &self,
        mask: &BitBoard<W, H, L>,
        kind_idx: Option<usize>,
        exclude: ID,
        min_tile: (i32, i32),
        max_tile: (i32, i32),
        mut callback: F,
    ) where
        F: FnMut(ID),
    {
        let target_board = match kind_idx {
            Some(k) => self.layer(k),
            None => &self.presence,
        };
        let kind_u8 = kind_idx.map(|k| k as u8);

        mask.for_each_overlap_in(target_board, min_tile, max_tile, |_x, _y, idx| {
            for &(e, k) in &self.cells[idx] {
                if e != exclude && kind_u8.map_or(true, |tk| k == tk) {
                    callback(e);
                }
            }
        });
    }

    pub fn is_tile_occupied(&self, tile_x: i32, tile_y: i32) -> bool {
        if tile_x < 0 || tile_y < 0 || tile_x >= (W as i32) || tile_y >= (H as i32) {
            return false;
        }
        self.presence.get(tile_x, tile_y)
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
        hash.query_filtered_radius_callback((10, 10), 5, 99u32, None, |e| {
            found.push(e);
        });
        
        assert_eq!(found.len(), 2);
        assert!(found.contains(&e1));
        assert!(found.contains(&e2)); // Exact distance 5 should be included
        
        let mut found2 = Vec::new();
        hash.query_filtered_radius_callback((10, 10), 4, 99u32, None, |e| {
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
        hash.query_circle_callback((10, 10), 5.0, None, 99u32, |e| {
            found.push(e);
        });
        
        assert!(found.contains(&e1));
        assert!(!found.contains(&e2), "Corner of the square should be excluded in circular query");
    }

    #[test]
    fn test_spatial_query_sector() {
        let mut hash = TestSpatialHash::default();
        let e1 = 1u32;
        let e2 = 2u32;
        
        // (10, 10) から右方向に e1, 左方向に e2
        hash.insert(e1, (15, 10), 0, 0); // 右 (0度)
        hash.insert(e2, (5, 10), 0, 0);  // 左 (180度)
        
        let mut found = Vec::new();
        // 右向き 90度の視界 ( -45度 〜 45度 )
        hash.query_sector_callback((10, 10), 10.0, -45.0, 90.0, None, 99u32, |e| {
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
        hash.query_mask_callback(&proximity_mask, Some(1), 99u32, |e| {
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
        // 範囲を (0,0) ~ (15,15) に限定して検索
        hash.query_mask_bounded_callback(
            &full_mask, 
            None, 
            99u32, 
            (0, 0), 
            (15, 15), 
            |e| { found.push(e); }
        );
        
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
        assert!(!vis.get(14, 10), "Shadowcasting should work through SpatialHash wrapper");
    }

    #[test]
    fn test_spatial_multiple_entities_overlap() {
        let mut hash = TestSpatialHash::default();
        let e1 = 1u32;
        let e2 = 2u32;
        
        hash.insert(e1, (10, 10), 0, 0);
        hash.insert(e2, (10, 10), 0, 0);
        
        assert!(hash.is_tile_occupied(10, 10));
        assert_eq!(hash.cells[TestBoard::tile_to_index(10, 10).unwrap()].len(), 2);
        
        hash.remove(e1);
        assert!(hash.is_tile_occupied(10, 10), "Still occupied by e2");
        assert_eq!(hash.cells[TestBoard::tile_to_index(10, 10).unwrap()].len(), 1);
        
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
        hash.query_circle_callback((10, 10), 2.0, None, e1, |e| {
            found.push(e);
        });
        
        assert_eq!(found.len(), 1);
        assert!(found.contains(&e2));
        assert!(!found.contains(&e1));
    }

    #[test]
    fn test_spatial_consistency_audit() {
        let mut hash = TestSpatialHash::default();
        let e1 = 1u32;
        hash.insert(e1, (50, 50), 2, 0);
        
        // Audit: presence board counts should match entity_info area? 
        // Not exactly because of overlap, but for a single entity it should.
        let mask = TestBoard::mask_rect(50-2, 50-2, 5, 5);
        assert_eq!(hash.presence.count_ones(), mask.count_ones());
        
        // Audit: cells should have e1 where presence is true
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
        assert!(hash.layer(0).get(10, 10), "Bit should remain since e2 is still Kind 0 at this tile");
        
        hash.remove(e2);
        assert!(!hash.layer(0).get(10, 10), "Bit should be cleared now");
    }

    #[test]
    fn test_spatial_query_empty_mask() {
        let mut hash = TestSpatialHash::default();
        hash.insert(1, (10, 10), 0, 0);
        
        let empty_mask = BitBoard::<256, 256>::new();
        let mut found = Vec::new();
        hash.query_mask_callback(&empty_mask, None, 99, |e| found.push(e));
        
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
}
