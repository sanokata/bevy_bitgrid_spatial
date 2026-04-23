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


    /// エンティティの各セルへの登録内容を差分更新
    pub fn update_diff(
        &mut self,
        id: ID,
        new_center: (i32, i32),
        new_radius: i32,
        new_kind_idx: usize,
    ) {
        let old_info = if let Some(info) = self.entity_info.get(&id) {
            if info.center == new_center
                && info.radius == new_radius
                && info.kind_idx == new_kind_idx
            {
                return;
            }
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

        let old_min = (old_center.0 - radius, old_center.1 - radius);
        let old_max = (old_center.0 + radius, old_center.1 + radius);
        let new_min = (new_center.0 - radius, new_center.1 - radius);
        let new_max = (new_center.0 + radius, new_center.1 + radius);

        // 離れたセルから除去
        for x in old_min.0..=old_max.0 {
            for y in old_min.1..=old_max.1 {
                if x < new_min.0 || x > new_max.0 || y < new_min.1 || y > new_max.1 {
                    self.cell_remove(x, y, id, kind_idx);
                }
            }
        }

        // 新しいセルへ挿入
        for x in new_min.0..=new_max.0 {
            for y in new_min.1..=new_max.1 {
                if x < old_min.0 || x > old_max.0 || y < old_min.1 || y > old_max.1 {
                    self.cell_insert(x, y, id, kind_idx);
                }
            }
        }

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
            for dx in -entry.radius..=entry.radius {
                for dy in -entry.radius..=entry.radius {
                    self.cell_remove(
                        entry.center.0 + dx,
                        entry.center.1 + dy,
                        id,
                        entry.kind_idx,
                    );
                }
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
        opaque_board.mask_visibility(cx, cy, radius, opaque_board)
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
        wall_map.set(6, 6, true);
        
        assert_eq!(hash.static_revision(), 0);
        
        // 静的レイヤー (インデックス 0 とする) を同期
        hash.full_sync_static_layer(0, &wall_map, 1);
        
        assert_eq!(hash.static_revision(), 1);
        
        // 静的レイヤー自体は self.cells や presence には入らないが、
        // self.static_layers[0] に記録される
        // (現状の public API では is_tile_occupied は presence のみをみる)
        // もし将来的に static も含めた判定が必要な場合は、レイヤー直接アクセサが使われる
        assert!(!hash.is_tile_occupied(5, 5)); 
        
        // query_mask_callback で静的レイヤーを指定して引けるか
        // (現状は query は動的セルのみ。静的レイヤーとの AND 演算は利用者側で行う想定)
        // ここではクラッシュせず Revision が上がることを確認すれば十分。
    }
}
