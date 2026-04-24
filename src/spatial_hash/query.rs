use core::hash::Hash;
use lexaos_bitboard::{BitBoard, BitLayout};
use super::SpatialHash;

#[derive(Debug, Clone, Copy)]
pub struct SectorArgs {
    pub start_angle: f32,
    pub sweep_angle: f32,
}

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

    /// 円形範囲内のエンティティを検索
    pub(crate) fn query_circle_callback<F>(
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
                if exclude.is_none_or(|ex| e != ex) && kind_mask.is_none_or(|mask| (mask >> k) & 1 == 1) {
                    callback(e);
                }
            }
        });
    }

    /// 扇形範囲内のエンティティを検索
    pub(crate) fn query_sector_callback<F>(
        &self,
        center: (i32, i32),
        radius: f32,
        args: SectorArgs,
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
            args.start_angle,
            args.sweep_angle,
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
                if exclude.is_none_or(|ex| e != ex) && kind_mask.is_none_or(|mask| (mask >> k) & 1 == 1) {
                    callback(e);
                }
            }
        });
    }

    /// 任意のマスクと種別（任意）でエンティティを検索
    pub(crate) fn query_mask_callback<F>(
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
                if exclude.is_none_or(|ex| e != ex) && kind_mask.is_none_or(|mask| (mask >> k) & 1 == 1) {
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
