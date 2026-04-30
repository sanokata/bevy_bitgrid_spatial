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

    /// kind_mask に基づいて走査対象の BitBoard を選択する。
    ///
    /// - `kind_mask = None` または複数ビット指定: `presence`（全エンティティ）を返す
    ///   （per-cell の kind フィルタは呼び出し側 `query_with_mask` 内で実施）
    /// - `kind_mask = Some(1 << k)` の単一ビット指定: `kind_boards[k]` を返し、
    ///   不要なエンティティを走査せず済む
    /// - `k >= E` の場合は `presence` にフォールバックするが、これは想定外入力。
    ///   debug ビルドでは検出できるよう `debug_assert!` する
    #[inline]
    fn select_target_board(&self, kind_mask: Option<u64>) -> &BitBoard<W, H, L> {
        match kind_mask {
            Some(mask) if mask.count_ones() == 1 => {
                let k = mask.trailing_zeros() as usize;
                debug_assert!(
                    k < E,
                    "kind_mask points to layer {} but only {} layers exist",
                    k,
                    E
                );
                if k < E { self.layer(k) } else { &self.presence }
            }
            _ => &self.presence,
        }
    }

    /// 与えられた検索マスク（円・扇形・任意形状）でセルを走査し、
    /// kind_mask と exclude のフィルタを通った ID をすべて callback に渡す。
    fn query_with_mask<F>(
        &self,
        search_mask: &BitBoard<W, H, L>,
        kind_mask: Option<u64>,
        exclude: Option<ID>,
        mut callback: F,
    ) where
        F: FnMut(ID),
    {
        let target_board = self.select_target_board(kind_mask);
        search_mask.for_each_overlap(target_board, |_x, _y, idx| {
            for &(e, k) in &self.cells[idx] {
                let pass_exclude = exclude.is_none_or(|ex| e != ex);
                let pass_kind = kind_mask.is_none_or(|mask| (mask >> k) & 1 == 1);
                if pass_exclude && pass_kind {
                    callback(e);
                }
            }
        });
    }

    /// 円形範囲内のエンティティを検索
    pub(crate) fn query_circle_callback<F>(
        &self,
        center: (i32, i32),
        radius: f32,
        kind_mask: Option<u64>,
        exclude: Option<ID>,
        callback: F,
    ) where
        F: FnMut(ID),
    {
        let circle_mask =
            BitBoard::<W, H, L>::mask_sector(center.0, center.1, radius, 0.0, 360.0);
        self.query_with_mask(&circle_mask, kind_mask, exclude, callback);
    }

    /// 扇形範囲内のエンティティを検索
    pub(crate) fn query_sector_callback<F>(
        &self,
        center: (i32, i32),
        radius: f32,
        args: SectorArgs,
        kind_mask: Option<u64>,
        exclude: Option<ID>,
        callback: F,
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
        self.query_with_mask(&sector_mask, kind_mask, exclude, callback);
    }

    /// 任意のマスクと種別（任意）でエンティティを検索
    pub(crate) fn query_mask_callback<F>(
        &self,
        mask: &BitBoard<W, H, L>,
        kind_mask: Option<u64>,
        exclude: Option<ID>,
        callback: F,
    ) where
        F: FnMut(ID),
    {
        self.query_with_mask(mask, kind_mask, exclude, callback);
    }

    pub fn is_tile_occupied(&self, tile_x: i32, tile_y: i32) -> bool {
        if tile_x < 0 || tile_y < 0 || tile_x >= (W as i32) || tile_y >= (H as i32) {
            return false;
        }
        self.presence.get(tile_x, tile_y)
    }
}
