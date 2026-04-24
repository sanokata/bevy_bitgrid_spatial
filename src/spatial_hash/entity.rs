use core::hash::Hash;
use lexaos_bitboard::{BitBoard, BitLayout};
use super::SpatialHash;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct EntityEntry {
    pub(crate) center: (i32, i32),
    pub(crate) radius: i32,
    pub(crate) kind_idx: usize,
}

impl<ID, const W: usize, const H: usize, const E: usize, const S: usize, L: BitLayout<W, H>>
    SpatialHash<ID, W, H, E, S, L>
where
    ID: Copy + Eq + Hash,
{
    /// 指定したエンティティの現在の位置情報を取得
    pub fn get_entity_info(&self, id: ID) -> Option<(i32, i32, i32)> {
        self.entity_info
            .get(&id)
            .map(|info| (info.center.0, info.center.1, info.radius))
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

        let remove_mask = &old_mask & &!&new_mask;
        for (x, y) in remove_mask.iter_set_bits() {
            self.cell_remove(x, y, id, kind_idx);
        }

        let insert_mask = &new_mask & &!&old_mask;
        for (x, y) in insert_mask.iter_set_bits() {
            self.cell_insert(x, y, id, kind_idx);
        }

        if let Some(info) = self.entity_info.get_mut(&id) {
            info.center = new_center;
        }
    }

    /// しきい値ベースのスロットリング更新
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

    pub(super) fn cell_insert(&mut self, x: i32, y: i32, id: ID, kind_idx: usize) {
        if let Some(idx) = BitBoard::<W, H, L>::tile_to_index(x, y) {
            self.cells[idx].push((id, kind_idx as u8));
            self.presence.set(x, y, true);
            self.layer_mut(kind_idx).set(x, y, true);
        }
    }

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

    pub fn insert(&mut self, id: ID, tile_pos: (i32, i32), radius: i32, kind_idx: usize) {
        let mut mask = BitBoard::<W, H, L>::default();
        mask.set(tile_pos.0, tile_pos.1, true);
        let mask = if radius > 0 {
            mask.dilate(radius as u32)
        } else {
            mask
        };

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

    pub fn update(&mut self, id: ID, new_tile_pos: (i32, i32), radius: i32, kind_idx: usize) {
        self.update_diff(id, new_tile_pos, radius, kind_idx);
    }

    #[inline(always)]
    fn layer_mut(&mut self, kind_idx: usize) -> &mut BitBoard<W, H, L> {
        &mut self.kind_boards[kind_idx]
    }
}
