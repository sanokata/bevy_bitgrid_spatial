use bevy::ecs::entity::EntityHashMap;
use bevy::prelude::*;
use lexaos_bitboard::BitBoard;
use smallvec::SmallVec;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct EntityEntry {
    center: (i32, i32),
    radius: i32,
    kind_idx: usize,
}

/// タイル座標ベースのエンティティ位置を管理する空間ハッシュ (汎用版)
/// const E: エンティティ種別の数 (Dynamic layers)
/// const S: 静的レイヤーの数 (Static layers like Terrain)
#[derive(Resource)]
pub struct SpatialHash<const W: usize, const H: usize, const E: usize, const S: usize> {
    /// セル管理（y * W + x でアクセス）
    cells: Box<[SmallVec<[Entity; 4]>]>,
    /// エンティティの管理情報
    entity_info: EntityHashMap<EntityEntry>,
    /// 存在判定用のビットマップ
    presence: BitBoard<W, H>,
    /// 種別ごとの高速存在判定ビットマップ (Eレイヤー)
    kind_boards: [BitBoard<W, H>; E],
    /// 地形などの静的レイヤーのコピー (Sレイヤー)
    static_layers: [BitBoard<W, H>; S],
    /// 静的レイヤーの同期用リビジョン
    static_revision: u32,
}

impl<const W: usize, const H: usize, const E: usize, const S: usize> Default
    for SpatialHash<W, H, E, S>
{
    fn default() -> Self {
        let cells = vec![SmallVec::new(); W * H].into_boxed_slice();
        Self {
            cells,
            entity_info: EntityHashMap::default(),
            presence: BitBoard::default(),
            kind_boards: std::array::from_fn(|_| BitBoard::default()),
            static_layers: std::array::from_fn(|_| BitBoard::default()),
            static_revision: 0,
        }
    }
}

impl<const W: usize, const H: usize, const E: usize, const S: usize> SpatialHash<W, H, E, S> {
    /// 指定インデックスのエンティティレイヤーを取得
    #[inline(always)]
    pub fn layer(&self, kind_idx: usize) -> &BitBoard<W, H> {
        &self.kind_boards[kind_idx]
    }

    /// 指定インデックスの静的レイヤーを取得
    #[inline(always)]
    pub fn static_layer(&self, layer_idx: usize) -> &BitBoard<W, H> {
        &self.static_layers[layer_idx]
    }

    /// 静的レイヤー全体を一括更新し、リビジョンを上げる
    pub fn full_sync_static_layer(
        &mut self,
        layer_idx: usize,
        board: &BitBoard<W, H>,
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
    fn layer_mut(&mut self, kind_idx: usize) -> &mut BitBoard<W, H> {
        &mut self.kind_boards[kind_idx]
    }

    /// 座標から配列インデックスを取得
    #[inline(always)]
    fn get_index(x: i32, y: i32) -> Option<usize> {
        BitBoard::<W, H>::tile_to_index(x, y)
    }

    /// エンティティの各セルへの登録内容を差分更新
    pub fn update_diff(
        &mut self,
        entity: Entity,
        new_center: (i32, i32),
        new_radius: i32,
        new_kind_idx: usize,
    ) {
        let old_info = if let Some(info) = self.entity_info.get(&entity) {
            if info.center == new_center
                && info.radius == new_radius
                && info.kind_idx == new_kind_idx
            {
                return;
            }
            if info.radius != new_radius || info.kind_idx != new_kind_idx {
                let kind = info.kind_idx; // 固定用の取得
                self.remove(entity);
                self.insert(entity, new_center, new_radius, new_kind_idx);
                return;
            }
            *info
        } else {
            self.insert(entity, new_center, new_radius, new_kind_idx);
            return;
        };

        let old_center = old_info.center;
        let radius = new_radius;
        let kind_idx = new_kind_idx;

        let old_min = (old_center.0 - radius, old_center.1 - radius);
        let old_max = (old_center.0 + radius, old_center.1 + radius);
        let new_min = (new_center.0 - radius, new_center.1 - radius);
        let new_max = (new_center.0 + radius, new_center.1 + radius);

        for x in old_min.0..=old_max.0 {
            for y in old_min.1..=old_max.1 {
                if x < new_min.0 || x > new_max.0 || y < new_min.1 || y > new_max.1 {
                    self.cell_remove(x, y, entity, kind_idx);
                }
            }
        }

        for x in new_min.0..=new_max.0 {
            for y in new_min.1..=new_max.1 {
                if x < old_min.0 || x > old_max.0 || y < old_min.1 || y > old_max.1 {
                    self.cell_insert(x, y, entity, kind_idx);
                }
            }
        }

        if let Some(info) = self.entity_info.get_mut(&entity) {
            info.center = new_center;
        }
    }

    fn cell_insert(&mut self, x: i32, y: i32, entity: Entity, kind_idx: usize) {
        if let Some(idx) = BitBoard::<W, H>::tile_to_index(x, y) {
            self.cells[idx].push(entity);
            self.presence.set(x, y, true);
            self.layer_mut(kind_idx).set(x, y, true);
        }
    }

    fn cell_remove(&mut self, x: i32, y: i32, entity: Entity, kind_idx: usize) {
        if let Some(idx) = BitBoard::<W, H>::tile_to_index(x, y) {
            let list = &mut self.cells[idx];
            if let Some(pos) = list.iter().position(|&e| e == entity) {
                list.swap_remove(pos);
            }
            if list.is_empty() {
                self.presence.set(x, y, false);
            }
            let has_same_kind = list.iter().any(|&e| {
                self.entity_info
                    .get(&e)
                    .map_or(false, |info| info.kind_idx == kind_idx)
            });
            if !has_same_kind {
                self.layer_mut(kind_idx).set(x, y, false);
            }
        }
    }

    pub fn insert(&mut self, entity: Entity, tile_pos: (i32, i32), radius: i32, kind_idx: usize) {
        for dx in -radius..=radius {
            for dy in -radius..=radius {
                self.cell_insert(tile_pos.0 + dx, tile_pos.1 + dy, entity, kind_idx);
            }
        }
        self.entity_info.insert(
            entity,
            EntityEntry {
                center: tile_pos,
                radius,
                kind_idx,
            },
        );
    }

    pub fn remove(&mut self, entity: Entity) {
        if let Some(entry) = self.entity_info.remove(&entity) {
            for dx in -entry.radius..=entry.radius {
                for dy in -entry.radius..=entry.radius {
                    self.cell_remove(
                        entry.center.0 + dx,
                        entry.center.1 + dy,
                        entity,
                        entry.kind_idx,
                    );
                }
            }
        }
    }

    pub fn update(
        &mut self,
        entity: Entity,
        new_tile_pos: (i32, i32),
        radius: i32,
        kind_idx: usize,
    ) {
        self.update_diff(entity, new_tile_pos, radius, kind_idx);
    }

    pub fn compute_visibility_mask(
        &self,
        cx: i32,
        cy: i32,
        radius: f32,
        opaque_layer_idx: usize,
    ) -> BitBoard<W, H> {
        let opaque_board = self.static_layer(opaque_layer_idx);
        opaque_board.compute_visibility_mask(cx, cy, radius, opaque_board)
    }

    pub fn query_filtered_radius_callback<F>(
        &self,
        center: (i32, i32),
        radius: i32,
        exclude: Entity,
        kind_idx: Option<usize>,
        mut callback: F,
    ) where
        F: FnMut(Entity),
    {
        let mask = match kind_idx {
            Some(k) => self.layer(k),
            None => &self.presence,
        };

        for dy in -radius..=radius {
            let y = center.1 + dy;
            if y < 0 || y >= (H as i32) {
                continue;
            }

            let min_x = (center.0 - radius).max(0);
            let max_x = (center.0 + radius).min((W as i32) - 1);
            if min_x > max_x {
                continue;
            }

            if !mask.any_in_row(y, min_x, max_x) {
                continue;
            }

            let row_base_idx = (y as usize) * W;

            for x in min_x..=max_x {
                if !mask.get(x, y) {
                    continue;
                }

                let idx = row_base_idx + (x as usize);
                for &e in &self.cells[idx] {
                    if e != exclude {
                        if kind_idx.is_none()
                            || self
                                .entity_info
                                .get(&e)
                                .map_or(false, |info| Some(info.kind_idx) == kind_idx)
                        {
                            callback(e);
                        }
                    }
                }
            }
        }
    }

    pub fn query_kind_mask_callback<F>(
        &self,
        mask: &BitBoard<W, H>,
        kind_idx: usize,
        exclude: Entity,
        mut callback: F,
    ) where
        F: FnMut(Entity),
    {
        let kind_board = self.layer(kind_idx);

        mask.for_each_intersection(kind_board, |_x, _y, idx| {
            for &e in &self.cells[idx] {
                if e != exclude {
                    callback(e);
                }
            }
        });
    }

    /// 範囲制限付きのマクスクエリ
    pub fn query_kind_mask_bounded_callback<F>(
        &self,
        mask: &BitBoard<W, H>,
        kind_idx: usize,
        exclude: Entity,
        min_tile: (i32, i32),
        max_tile: (i32, i32),
        mut callback: F,
    ) where
        F: FnMut(Entity),
    {
        let kind_board = self.layer(kind_idx);

        mask.for_each_intersection_in_range(kind_board, min_tile, max_tile, |_x, _y, idx| {
            for &e in &self.cells[idx] {
                if e != exclude {
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
