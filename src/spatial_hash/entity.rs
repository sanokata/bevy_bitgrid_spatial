use super::SpatialHash;
use core::hash::Hash;
use lexaos_bitboard::{BitBoard, BitLayout};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct EntityEntry {
    pub(crate) center: (i32, i32),
    pub(crate) radius: i32,
    pub(crate) kind_idx: usize,
}

#[derive(Debug, Clone, Copy)]
struct Rect {
    x1: i32,
    y1: i32,
    x2: i32,
    y2: i32,
}

impl Rect {
    fn centered(center: (i32, i32), radius: i32) -> Self {
        Self {
            x1: center.0 - radius,
            y1: center.1 - radius,
            x2: center.0 + radius,
            y2: center.1 + radius,
        }
    }
}

/// `src` から `other` を引いた矩形差分（src ∖ other）を、最大 4 帯のみで走査して
/// callback を呼び出す。各セルは1度だけ訪問される。
fn for_each_rect_diff<F: FnMut(i32, i32)>(src: Rect, other: Rect, mut f: F) {
    // 交差矩形
    let ix1 = src.x1.max(other.x1);
    let ix2 = src.x2.min(other.x2);
    let iy1 = src.y1.max(other.y1);
    let iy2 = src.y2.min(other.y2);

    if ix1 > ix2 || iy1 > iy2 {
        // 重ならない場合は src 全体が差分
        for y in src.y1..=src.y2 {
            for x in src.x1..=src.x2 {
                f(x, y);
            }
        }
        return;
    }

    // 上帯: y in [src.y1, iy1-1], x in [src.x1, src.x2]
    if src.y1 < iy1 {
        for y in src.y1..iy1 {
            for x in src.x1..=src.x2 {
                f(x, y);
            }
        }
    }
    // 下帯: y in [iy2+1, src.y2], x in [src.x1, src.x2]
    if iy2 < src.y2 {
        for y in (iy2 + 1)..=src.y2 {
            for x in src.x1..=src.x2 {
                f(x, y);
            }
        }
    }
    // 左帯: x in [src.x1, ix1-1], y in [iy1, iy2]
    if src.x1 < ix1 {
        for y in iy1..=iy2 {
            for x in src.x1..ix1 {
                f(x, y);
            }
        }
    }
    // 右帯: x in [ix2+1, src.x2], y in [iy1, iy2]
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

        let old_rect = Rect::centered(old_center, radius);
        let new_rect = Rect::centered(new_center, radius);

        // 旧範囲にあって新範囲にないセルを削除（矩形差分の 4 帯のみ走査）
        for_each_rect_diff(old_rect, new_rect, |x, y| {
            self.cell_remove(x, y, id, kind_idx);
        });

        // 新範囲にあって旧範囲にないセルを挿入
        for_each_rect_diff(new_rect, old_rect, |x, y| {
            self.cell_insert(x, y, id, kind_idx);
        });

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

    pub fn update(&mut self, id: ID, new_tile_pos: (i32, i32), radius: i32, kind_idx: usize) {
        self.update_diff(id, new_tile_pos, radius, kind_idx);
    }

    #[inline(always)]
    fn layer_mut(&mut self, kind_idx: usize) -> &mut BitBoard<W, H, L> {
        &mut self.kind_boards[kind_idx]
    }
}
