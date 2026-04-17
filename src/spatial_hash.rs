use bevy::prelude::*;
use bevy::ecs::entity::EntityHashMap;
use lexaos_bitboard::BitBoard;
use smallvec::SmallVec;

/// 空間ハッシュで管理するエンティティ種別
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpatialEntityKind {
    Actor,
    Item,
}

struct EntityEntry {
    keys: SmallVec<[(i32, i32); 9]>,
    kind: SpatialEntityKind,
}

/// タイル座標ベースのエンティティ位置を管理する空間ハッシュ
/// フラット配列、SmallVec、BitBoard を活用した高速実装
#[derive(Resource)]
pub struct SpatialHash<const W: usize, const H: usize> {
    /// セル管理（y * W + x でアクセス）。SmallVec により少数エンティティならヒープ確保なし
    cells: Box<[SmallVec<[Entity; 4]>]>,
    /// エンティティの管理情報（所属セルと種別）
    entity_info: EntityHashMap<EntityEntry>,
    /// 存在判定用のビットマップ
    presence: BitBoard<W, H>,
    /// 種別ごとの高速存在判定ビットマップ
    pub items: BitBoard<W, H>,
    pub actors: BitBoard<W, H>,
}

impl<const W: usize, const H: usize> Default for SpatialHash<W, H> {
    fn default() -> Self {
        let cells = vec![SmallVec::new(); W * H].into_boxed_slice();
        Self {
            cells,
            entity_info: EntityHashMap::default(),
            presence: BitBoard::default(),
            items: BitBoard::default(),
            actors: BitBoard::default(),
        }
    }
}

impl<const W: usize, const H: usize> SpatialHash<W, H> {
    /// 座標から配列インデックスを取得。範囲外は None
    #[inline(always)]
    fn get_index(x: i32, y: i32) -> Option<usize> {
        if x < 0 || y < 0 || x >= (W as i32) || y >= (H as i32) {
            None
        } else {
            Some((y as usize) * W + (x as usize))
        }
    }

    /// 指定座標を中心に、半径 radius の範囲でエンティティを登録
    pub fn insert(&mut self, entity: Entity, tile_pos: (i32, i32), radius: i32, kind: SpatialEntityKind) {
        let mut keys = SmallVec::new();

        for dx in -radius..=radius {
            for dy in -radius..=radius {
                let x = tile_pos.0 + dx;
                let y = tile_pos.1 + dy;
                
                if let Some(idx) = Self::get_index(x, y) {
                    self.cells[idx].push(entity);
                    
                    self.presence.set(x, y, true);
                    match kind {
                        SpatialEntityKind::Actor => self.actors.set(x, y, true),
                        SpatialEntityKind::Item => self.items.set(x, y, true),
                    }
                    
                    keys.push((x, y));
                }
            }
        }
        self.entity_info.insert(entity, EntityEntry { keys, kind });
    }

    /// エンティティを全セルから削除
    pub fn remove(&mut self, entity: Entity) {
        if let Some(entry) = self.entity_info.remove(&entity) {
            for key in entry.keys {
                if let Some(idx) = Self::get_index(key.0, key.1) {
                    let list = &mut self.cells[idx];
                    list.retain(|e: &mut Entity| *e != entity);
                    
                    if list.is_empty() {
                        self.presence.set(key.0, key.1, false);
                    }

                    // 同一種別の他エンティティが存在するか確認し、ビットマップを更新
                    let has_same_kind = list.iter().any(|&e| {
                        self.entity_info.get(&e).map_or(false, |info| info.kind == entry.kind)
                    });

                    if !has_same_kind {
                        match entry.kind {
                            SpatialEntityKind::Actor => self.actors.set(key.0, key.1, false),
                            SpatialEntityKind::Item => self.items.set(key.0, key.1, false),
                        }
                    }
                }
            }
        }
    }

    /// エンティティの座標情報を更新
    pub fn update(&mut self, entity: Entity, new_tile_pos: (i32, i32), radius: i32, kind: SpatialEntityKind) {
        if let Some(info) = self.entity_info.get(&entity) {
            if let Some(&(first_x, first_y)) = info.keys.first() {
                if first_x == new_tile_pos.0 - radius && first_y == new_tile_pos.1 - radius {
                    return;
                }
            }
            self.remove(entity);
        }
        self.insert(entity, new_tile_pos, radius, kind);
    }

    /// コールバックを用いた半径範囲内のエンティティ走査
    pub fn query_radius_callback<F>(&self, center: (i32, i32), radius: i32, exclude: Entity, mut callback: F)
    where
        F: FnMut(Entity),
    {
        for dy in -radius..=radius {
            let y = center.1 + dy;
            if y < 0 || y >= (H as i32) {
                continue;
            }
            
            let min_x = (center.0 - radius).max(0);
            let max_x = (center.0 + radius).min((W as i32) - 1);
            if min_x > max_x { continue; }
            
            // 行ごとの一括判定による早期スキップ
            if !self.presence.any_in_row(y, min_x, max_x) {
                continue; 
            }
            
            for x in min_x..=max_x {
                if !self.presence.get(x, y) {
                    continue; 
                }

                if let Some(idx) = Self::get_index(x, y) {
                    for &e in &self.cells[idx] {
                        if e != exclude {
                            callback(e);
                        }
                    }
                }
            }
        }
    }

    /// 半径範囲内のエンティティ一覧を返す
    pub fn query_radius(&self, center: (i32, i32), radius: i32, exclude: Entity) -> Vec<Entity> {
        let mut result = Vec::new();
        self.query_radius_callback(center, radius, exclude, |e| {
            if !result.contains(&e) {
                result.push(e);
            }
        });
        result
    }

    /// 指定タイルに候補となるエンティティが存在するか
    pub fn any_in_tile(&self, tile_x: i32, tile_y: i32, exclude: Entity) -> bool {
        if !self.presence.get(tile_x, tile_y) {
            return false;
        }
        if let Some(idx) = Self::get_index(tile_x, tile_y) {
            self.cells[idx].iter().any(|&e| e != exclude)
        } else {
            false
        }
    }

    /// 指定タイルが他エンティティに占有されているか判定
    pub fn is_tile_occupied(&self, tile_x: i32, tile_y: i32) -> bool {
        if tile_x < 0 || tile_y < 0 || tile_x >= (W as i32) || tile_y >= (H as i32) {
            return false;
        }
        self.presence.get(tile_x, tile_y)
    }

    /// 全登録情報を削除
    pub fn clear(&mut self) {
        for list in self.cells.iter_mut() {
            list.clear();
        }
        self.entity_info.clear();
        self.presence.clear();
        self.items.clear();
        self.actors.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::ecs::world::World;

    /// テスト用に `World` からエンティティを `n` 個生成して返す
    fn spawn_n(n: usize) -> (World, Vec<Entity>) {
        let mut world = World::default();
        let entities = (0..n).map(|_| world.spawn_empty().id()).collect();
        (world, entities)
    }

    #[test]
    fn insert_and_query_radius() {
        let (_w, es) = spawn_n(2);
        let (e0, dummy) = (es[0], es[1]);
        let mut sh = SpatialHash::<256, 256>::default();
        sh.insert(e0, (0, 0), 0, SpatialEntityKind::Actor);
        let hits = sh.query_radius((0, 0), 1, dummy);
        assert!(hits.contains(&e0));
    }

    #[test]
    fn remove_clears_entity() {
        let (_w, es) = spawn_n(2);
        let (e0, dummy) = (es[0], es[1]);
        let mut sh = SpatialHash::<256, 256>::default();
        sh.insert(e0, (0, 0), 0, SpatialEntityKind::Actor);
        sh.remove(e0);
        assert!(!sh.is_tile_occupied(0, 0));
        assert!(sh.query_radius((0, 0), 1, dummy).is_empty());
    }

    #[test]
    fn update_moves_to_new_cell() {
        let (_w, es) = spawn_n(2);
        let (e0, dummy) = (es[0], es[1]);
        let mut sh = SpatialHash::<256, 256>::default();
        sh.insert(e0, (0, 0), 0, SpatialEntityKind::Actor);
        sh.update(e0, (5, 0), 0, SpatialEntityKind::Actor);
        assert!(!sh.any_in_tile(0, 0, dummy));
        assert!(sh.any_in_tile(5, 0, dummy));
    }

    #[test]
    fn any_in_tile_excludes_self() {
        let (_w, es) = spawn_n(2);
        let (e0, dummy) = (es[0], es[1]);
        let mut sh = SpatialHash::<256, 256>::default();
        sh.insert(e0, (0, 0), 0, SpatialEntityKind::Actor);
        // 自身を exclude すると false
        assert!(!sh.any_in_tile(0, 0, e0));
        // 別エンティティから見ると true
        assert!(sh.any_in_tile(0, 0, dummy));
    }

    #[test]
    fn update_same_cell_is_noop() {
        let (_w, es) = spawn_n(1);
        let e0 = es[0];
        let mut sh = SpatialHash::<256, 256>::default();
        sh.insert(e0, (0, 0), 0, SpatialEntityKind::Actor);
        sh.update(e0, (0, 0), 0, SpatialEntityKind::Actor); // 同じセルへの更新
        assert!(sh.is_tile_occupied(0, 0));
    }

    #[test]
    fn bitmap_kinds_work() {
        let (_w, es) = spawn_n(2);
        let (actor, item) = (es[0], es[1]);
        let mut sh = SpatialHash::<256, 256>::default();
        sh.insert(actor, (0, 0), 0, SpatialEntityKind::Actor);
        sh.insert(item, (1, 0), 0, SpatialEntityKind::Item);

        assert!(sh.actors.get(0, 0));
        assert!(!sh.actors.get(1, 0));
        assert!(sh.items.get(1, 0));
        assert!(!sh.items.get(0, 0));
    }

    #[test]
    fn query_radius_deduplication() {
        let (_w, es) = spawn_n(2);
        let (e0, dummy) = (es[0], es[1]);
        let mut sh = SpatialHash::<256, 256>::default();
        sh.insert(e0, (0, 0), 1, SpatialEntityKind::Actor);
        
        let hits = sh.query_radius((0, 0), 2, dummy);
        assert_eq!(hits.len(), 1, "Duplicate entities returned in query_radius!");
        assert_eq!(hits[0], e0);
    }
}
