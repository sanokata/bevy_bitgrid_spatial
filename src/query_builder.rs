use super::spatial_hash::SpatialHash;
use core::hash::Hash;
use lexaos_bitboard::{BitBoard, BitLayout};

/// 空間ハッシュに対する柔軟な問い合わせを行うためのクエリビルダー
pub struct SpatialQuery<
    'a,
    ID,
    const W: usize,
    const H: usize,
    const E: usize,
    const S: usize,
    L: BitLayout<W, H>,
> where
    ID: Copy + Eq + Hash,
    L: BitLayout<W, H>,
{
    hash: &'a SpatialHash<ID, W, H, E, S, L>,
    kind_mask: Option<u64>,
    exclude: Option<ID>,
}

impl<'a, ID, const W: usize, const H: usize, const E: usize, const S: usize, L: BitLayout<W, H>>
    SpatialQuery<'a, ID, W, H, E, S, L>
where
    ID: Copy + Eq + Hash,
    L: BitLayout<W, H>,
{
    pub fn new(hash: &'a SpatialHash<ID, W, H, E, S, L>) -> Self {
        Self {
            hash,
            kind_mask: None,
            exclude: None,
        }
    }

    /// 特定の種別（複数指定可）に絞り込む。bit `k` が kind_idx `k` に対応する。
    ///
    /// - `mask = 0` を指定すると「どの種別にも一致しない」になり、結果は常に空。
    /// - 既に `with_kind` または `with_kind_mask` を呼んでいた場合は **上書き** する
    ///   （複数の制約を OR で結合したい場合は呼び出し側でマスクを合成すること）。
    pub fn with_kind_mask(mut self, mask: u64) -> Self {
        self.kind_mask = Some(mask);
        self
    }

    /// 特定の単一種別に絞り込む。
    ///
    /// 内部的には `with_kind_mask(1 << kind_idx)` と等価で、複数回呼び出すと
    /// **最後の呼び出しのみが有効**となる。OR 結合したい場合は `with_kind_mask` を使う。
    pub fn with_kind(mut self, kind_idx: usize) -> Self {
        self.kind_mask = Some(1u64 << kind_idx);
        self
    }

    /// 特定のエンティティを除外する。複数回呼び出した場合は最後の呼び出しが有効。
    pub fn exclude(mut self, id: ID) -> Self {
        self.exclude = Some(id);
        self
    }

    /// 円形範囲内のエンティティを走査
    pub fn circle<F>(&self, center: (i32, i32), radius: f32, mut callback: F)
    where
        F: FnMut(ID),
    {
        self.hash
            .query_circle_callback(center, radius, self.kind_mask, self.exclude, |id| {
                callback(id);
            });
    }

    /// 矩形範囲内のエンティティを走査
    pub fn rect<F>(&self, x: i32, y: i32, width: i32, height: i32, mut callback: F)
    where
        F: FnMut(ID),
    {
        let mask = BitBoard::<W, H, L>::mask_rect(x, y, width, height);
        self.hash
            .query_mask_callback(&mask, self.kind_mask, self.exclude, |id| {
                callback(id);
            });
    }

    /// マスク（BitBoard）範囲内のエンティティを走査
    pub fn mask<F>(&self, mask: &BitBoard<W, H, L>, mut callback: F)
    where
        F: FnMut(ID),
    {
        self.hash
            .query_mask_callback(mask, self.kind_mask, self.exclude, |id| {
                callback(id);
            });
    }

    /// 扇形範囲内のエンティティを走査
    pub fn sector<F>(
        &self,
        center: (i32, i32),
        radius: f32,
        start_angle: f32,
        sweep_angle: f32,
        mut callback: F,
    ) where
        F: FnMut(ID),
    {
        let args = crate::spatial_hash::SectorArgs {
            start_angle,
            sweep_angle,
        };
        self.hash
            .query_sector_callback(center, radius, args, self.kind_mask, self.exclude, |id| {
                callback(id);
            });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spatial_hash::SpatialHash;
    use lexaos_bitboard::RowMajorLayout;

    type TestSpatialHash = SpatialHash<u32, 256, 256, 4, 1, RowMajorLayout>;

    #[test]
    fn test_query_builder_filtering() {
        let mut hash = TestSpatialHash::default();
        let e1 = 1u32; // Kind 0
        let e2 = 2u32; // Kind 1
        let e3 = 3u32; // Kind 2

        hash.insert(e1, (10, 10), 0, 0);
        hash.insert(e2, (11, 11), 0, 1);
        hash.insert(e3, (12, 12), 0, 2);

        // 1. 種別フィルタ (Kind 1 のみ)
        let mut found = Vec::new();
        hash.query()
            .with_kind(1)
            .circle((10, 10), 5.0, |id| found.push(id));
        assert_eq!(found, vec![e2]);

        // 2. 複数種別フィルタ (Kind 0 or 2)
        let mut found = Vec::new();
        hash.query()
            .with_kind_mask((1 << 0) | (1 << 2))
            .circle((10, 10), 5.0, |id| found.push(id));
        assert!(found.contains(&e1));
        assert!(found.contains(&e3));
        assert!(!found.contains(&e2));

        // 3. 除外 (e1 を除外)
        let mut found = Vec::new();
        hash.query()
            .exclude(e1)
            .circle((10, 10), 5.0, |id| found.push(id));
        assert!(!found.contains(&e1));
        assert!(found.contains(&e2));
        assert!(found.contains(&e3));
    }

    #[test]
    fn test_query_builder_shapes() {
        let mut hash = TestSpatialHash::default();
        let e1 = 1u32;
        hash.insert(e1, (10, 10), 0, 0);

        // Rect クエリ
        let mut found = Vec::new();
        hash.query().rect(9, 9, 3, 3, |id| found.push(id));
        assert!(found.contains(&e1));

        // 範囲外の Rect
        let mut found = Vec::new();
        hash.query().rect(20, 20, 5, 5, |id| found.push(id));
        assert!(found.is_empty());

        // Mask クエリ
        let mut mask = BitBoard::<256, 256, RowMajorLayout>::default();
        mask.set(10, 10, true);
        let mut found = Vec::new();
        hash.query().mask(&mask, |id| found.push(id));
        assert!(found.contains(&e1));
    }
}
