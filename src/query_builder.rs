use super::spatial_hash::SpatialHash;
use core::hash::Hash;
use bitgrid::{BitBoard, BitLayout};

/// Builder for flexible spatial hash queries.
///
/// Obtained via [`SpatialHash::query`]. Chain filter methods then call a shape
/// method (`circle`, `rect`, `mask`, `sector`) to iterate matching entities.
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
    /// Bitmask of accepted kind indices; `None` accepts all kinds.
    kind_mask: Option<u64>,
    /// Entity to exclude from results.
    exclude: Option<ID>,
}

impl<'a, ID, const W: usize, const H: usize, const E: usize, const S: usize, L: BitLayout<W, H>>
    SpatialQuery<'a, ID, W, H, E, S, L>
where
    ID: Copy + Eq + Hash,
    L: BitLayout<W, H>,
{
    /// Creates a new query builder with no filters applied.
    pub fn new(hash: &'a SpatialHash<ID, W, H, E, S, L>) -> Self {
        Self {
            hash,
            kind_mask: None,
            exclude: None,
        }
    }

    /// Filter results to entities whose `kind_idx` has the corresponding bit set in `mask`.
    ///
    /// Bit `k` selects `kind_idx == k`. Multiple bits yield OR semantics.
    /// `mask = 0` matches nothing and always yields an empty result.
    /// Calling this (or [`with_kind`](Self::with_kind)) multiple times **overwrites** the
    /// previous value; combine masks on the caller side for OR logic across calls.
    pub fn with_kind_mask(mut self, mask: u64) -> Self {
        self.kind_mask = Some(mask);
        self
    }

    /// Filter results to a single entity kind.
    ///
    /// Equivalent to `with_kind_mask(1 << kind_idx)`. If called multiple times, only the
    /// **last** call takes effect. Use [`with_kind_mask`](Self::with_kind_mask) to select
    /// multiple kinds in one call.
    pub fn with_kind(mut self, kind_idx: usize) -> Self {
        self.kind_mask = Some(1u64 << kind_idx);
        self
    }

    /// Exclude a specific entity from results. Only the last call takes effect.
    pub fn exclude(mut self, id: ID) -> Self {
        self.exclude = Some(id);
        self
    }

    /// Iterate entities within a circular area.
    pub fn circle<F>(&self, center: (i32, i32), radius: f32, mut callback: F)
    where
        F: FnMut(ID),
    {
        self.hash
            .query_circle_callback(center, radius, self.kind_mask, self.exclude, |id| {
                callback(id);
            });
    }

    /// Iterate entities within a rectangular area.
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

    /// Iterate entities within an arbitrary [`BitBoard`] mask.
    pub fn mask<F>(&self, mask: &BitBoard<W, H, L>, mut callback: F)
    where
        F: FnMut(ID),
    {
        self.hash
            .query_mask_callback(mask, self.kind_mask, self.exclude, |id| {
                callback(id);
            });
    }

    /// Iterate entities within a sector (cone) defined by a center, radius, and angle range.
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
    use bitgrid::RowMajorLayout;

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

        // 1. Kind filter (Kind 1 only)
        let mut found = Vec::new();
        hash.query()
            .with_kind(1)
            .circle((10, 10), 5.0, |id| found.push(id));
        assert_eq!(found, vec![e2]);

        // 2. Multi-kind filter (Kind 0 or 2)
        let mut found = Vec::new();
        hash.query()
            .with_kind_mask((1 << 0) | (1 << 2))
            .circle((10, 10), 5.0, |id| found.push(id));
        assert!(found.contains(&e1));
        assert!(found.contains(&e3));
        assert!(!found.contains(&e2));

        // 3. Exclusion (exclude e1)
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

        // Rect query
        let mut found = Vec::new();
        hash.query().rect(9, 9, 3, 3, |id| found.push(id));
        assert!(found.contains(&e1));

        // Rect outside range
        let mut found = Vec::new();
        hash.query().rect(20, 20, 5, 5, |id| found.push(id));
        assert!(found.is_empty());

        // Mask query
        let mut mask = BitBoard::<256, 256, RowMajorLayout>::default();
        mask.set(10, 10, true);
        let mut found = Vec::new();
        hash.query().mask(&mask, |id| found.push(id));
        assert!(found.contains(&e1));
    }
}
