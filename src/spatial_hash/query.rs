use core::hash::Hash;
use lexaos_bitboard::{BitBoard, BitLayout};
use super::SpatialHash;

impl<ID, const W: usize, const H: usize, const E: usize, const S: usize, L: BitLayout<W, H>>
    SpatialHash<ID, W, H, E, S, L>
where
    ID: Copy + Eq + Hash,
{
    /// クエリビルダーを取得
    pub fn query(&self) -> crate::query_builder::SpatialQuery<'_, ID, W, H, E, S, L> {
        crate::query_builder::SpatialQuery::new(self)
    }

    /// 指定インデックスのエンティティレイヤーを取得
    #[inline(always)]
    pub fn layer(&self, kind_idx: usize) -> &BitBoard<W, H, L> {
        &self.kind_boards[kind_idx]
    }

    /// 円形範囲内のエンティティを検索 (正確な半径判定)
    pub fn query_circle_callback<F>(
        &self,
        center: (i32, i32),
        radius: f32,
        kind_mask: Option<u64>,
        exclude: Option<ID>,
        mut callback: F,
    ) where
        F: FnMut(ID),
    {
        let circle_mask = BitBoard::<W, H, L>::mask_sector(center.0, center.1, radius, 0.0, 360.0);

        let target_board = if let Some(mask) = kind_mask {
            if mask.count_ones() == 1 {
                let k = mask.trailing_zeros() as usize;
                if k < E { self.layer(k) } else { &self.presence }
            } else {
                &self.presence
            }
        } else {
            &self.presence
        };

        circle_mask.for_each_overlap(target_board, |_x, _y, idx| {
            for &(e, k) in &self.cells[idx] {
                if exclude.map_or(true, |ex| e != ex) {
                    if kind_mask.map_or(true, |mask| (mask >> k) & 1 == 1) {
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
        kind_mask: Option<u64>,
        exclude: Option<ID>,
        mut callback: F,
    ) where
        F: FnMut(ID),
    {
        let sector_mask = BitBoard::<W, H, L>::mask_sector(
            center.0,
            center.1,
            radius,
            start_angle_deg,
            sweep_angle_deg,
        );

        let target_board = if let Some(mask) = kind_mask {
            if mask.count_ones() == 1 {
                let k = mask.trailing_zeros() as usize;
                if k < E { self.layer(k) } else { &self.presence }
            } else {
                &self.presence
            }
        } else {
            &self.presence
        };

        sector_mask.for_each_overlap(target_board, |_x, _y, idx| {
            for &(e, k) in &self.cells[idx] {
                if exclude.map_or(true, |ex| e != ex) {
                    if kind_mask.map_or(true, |mask| (mask >> k) & 1 == 1) {
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
        exclude: Option<ID>,
        kind_mask: Option<u64>,
        mut callback: F,
    ) where
        F: FnMut(ID),
    {
        let target_board = if let Some(mask) = kind_mask {
            if mask.count_ones() == 1 {
                let k = mask.trailing_zeros() as usize;
                if k < E { self.layer(k) } else { &self.presence }
            } else {
                &self.presence
            }
        } else {
            &self.presence
        };

        let min_tile = (center.0 - radius, center.1 - radius);
        let max_tile = (center.0 + radius, center.1 + radius);

        target_board.for_each_overlap_in(target_board, min_tile, max_tile, |_x, _y, idx| {
            for &(e, k) in &self.cells[idx] {
                if exclude.map_or(true, |ex| e != ex) {
                    if kind_mask.map_or(true, |mask| (mask >> k) & 1 == 1) {
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
        kind_mask: Option<u64>,
        exclude: Option<ID>,
        mut callback: F,
    ) where
        F: FnMut(ID),
    {
        let target_board = if let Some(mask) = kind_mask {
            if mask.count_ones() == 1 {
                let k = mask.trailing_zeros() as usize;
                if k < E { self.layer(k) } else { &self.presence }
            } else {
                &self.presence
            }
        } else {
            &self.presence
        };

        mask.for_each_overlap(target_board, |_x, _y, idx| {
            for &(e, k) in &self.cells[idx] {
                if exclude.map_or(true, |ex| e != ex) {
                    if kind_mask.map_or(true, |mask| (mask >> k) & 1 == 1) {
                        callback(e);
                    }
                }
            }
        });
    }

    /// 任意のマスク、種別（任意）、および範囲制限でエンティティを検索
    pub fn query_mask_bounded_callback<F>(
        &self,
        mask: &BitBoard<W, H, L>,
        kind_mask: Option<u64>,
        exclude: Option<ID>,
        min_tile: (i32, i32),
        max_tile: (i32, i32),
        mut callback: F,
    ) where
        F: FnMut(ID),
    {
        let target_board = if let Some(mask) = kind_mask {
            if mask.count_ones() == 1 {
                let k = mask.trailing_zeros() as usize;
                if k < E { self.layer(k) } else { &self.presence }
            } else {
                &self.presence
            }
        } else {
            &self.presence
        };

        mask.for_each_overlap_in(target_board, min_tile, max_tile, |_x, _y, idx| {
            for &(e, k) in &self.cells[idx] {
                if exclude.map_or(true, |ex| e != ex) {
                    if kind_mask.map_or(true, |mask| (mask >> k) & 1 == 1) {
                        callback(e);
                    }
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
