use super::SpatialHash;
use core::hash::Hash;
use bitgrid::{BitBoard, BitLayout};

/// `is_static_area_all_set` が事前計算済みの収縮レイヤーを使う半径。
/// この配列の要素 `r` が `eroded_layers[layer][r-1]` に対応する。
/// （半径 0 は元のレイヤーを直接参照するためテーブルには含めない）
const CACHED_EROSION_RADII: [i32; 2] = [1, 2];

impl<ID, const W: usize, const H: usize, const E: usize, const S: usize, L: BitLayout<W, H>>
    SpatialHash<ID, W, H, E, S, L>
where
    ID: Copy + Eq + Hash,
{
    /// 指定インデックスの静的レイヤーを取得
    #[inline(always)]
    pub fn static_layer(&self, layer_idx: usize) -> &BitBoard<W, H, L> {
        &self.static_layers[layer_idx]
    }

    pub fn mask_visibility(
        &self,
        cx: i32,
        cy: i32,
        radius: f32,
        opaque_layer_idx: usize,
    ) -> BitBoard<W, H, L> {
        let opaque_board = self.static_layer(opaque_layer_idx);
        BitBoard::<W, H, L>::mask_visibility(cx, cy, radius, opaque_board)
    }

    /// 既存のバッファを使用して視界マスクを計算（アロケーションフリー）
    pub fn mask_visibility_into(
        &self,
        cx: i32,
        cy: i32,
        radius: f32,
        opaque_layer_idx: usize,
        out: &mut BitBoard<W, H, L>,
    ) {
        let opaque_board = self.static_layer(opaque_layer_idx);
        out.mask_visibility_into(cx, cy, radius, opaque_board);
    }

    /// 静的レイヤー全体を一括更新し、リビジョンを上げる。
    ///
    /// `revision` は呼び出し側（典型的には `TileMap::revision`）から伝搬される
    /// 単調増加のソース ID で、`static_revision()` と比較することで「再同期が必要か」を
    /// 判定するための変更検知トークン。値そのものに意味はなく、`!=` 比較のみが使われる。
    ///
    /// **注意**: この関数は `static_layers` と `eroded_layers` の両方を一括で再計算する。
    /// 個別タイル更新を行う `update_static_tile` は `eroded_layers` を更新しないため、
    /// 部分更新後に半径 1〜2 の `is_static_area_all_set` を呼ぶと古いキャッシュ結果を返す。
    /// 一括変更後は本関数を呼んでキャッシュを再構築すること。
    pub fn full_sync_static_layer(
        &mut self,
        layer_idx: usize,
        board: &BitBoard<W, H, L>,
        revision: u32,
    ) {
        if layer_idx >= S {
            return;
        }

        // 既存バッファを流用してコピー（再アロケートなし）
        self.static_layers[layer_idx].clone_from(board);

        // 収縮済みキャッシュ (radius=1, radius=2) も同じ buffer を再利用して in-place 更新
        let mut scratch = BitBoard::<W, H, L>::new();
        self.eroded_layers[layer_idx][0].clone_from(board);
        self.eroded_layers[layer_idx][0].erode_with_buffer(1, &mut scratch);
        self.eroded_layers[layer_idx][1].clone_from(board);
        self.eroded_layers[layer_idx][1].erode_with_buffer(2, &mut scratch);

        self.static_revision = revision;
    }

    /// 特定のタイルの静的レイヤー情報を更新
    pub fn update_static_tile(
        &mut self,
        layer_idx: usize,
        x: i32,
        y: i32,
        val: bool,
        revision: u32,
    ) {
        if layer_idx < S {
            self.static_layers[layer_idx].set(x, y, val);
            self.static_revision = revision;
        }
    }

    pub fn static_revision(&self) -> u32 {
        self.static_revision
    }

    /// 指定した静的レイヤー (地形等) の範囲内がすべてセットされているか判定 (通行判定用)。
    ///
    /// 半径 0 は元のレイヤーへの直接参照、`CACHED_EROSION_RADII` に含まれる半径
    /// （現在は 1, 2）は事前計算済み収縮レイヤーで O(1) 判定する。
    /// それ以外は BitBoard 側の汎用 `is_area_all_set` にフォールバックする。
    pub fn is_static_area_all_set(&self, layer_idx: usize, x: i32, y: i32, radius: i32) -> bool {
        if layer_idx >= S {
            return false;
        }
        if radius == 0 {
            return self.static_layers[layer_idx].get(x, y);
        }
        // CACHED_EROSION_RADII の要素 r に対応する eroded_layers[layer_idx][r - 1] を引く
        if let Some(cache_idx) = CACHED_EROSION_RADII.iter().position(|&r| r == radius) {
            return self.eroded_layers[layer_idx][cache_idx].get(x, y);
        }
        self.static_layers[layer_idx].is_area_all_set(x, y, radius)
    }

    /// 指定した静的レイヤー (地形等) の範囲内に一つでもセットされたビットがあるか判定 (衝突判定用)
    pub fn is_static_area_any_set(&self, layer_idx: usize, x: i32, y: i32, radius: i32) -> bool {
        if layer_idx >= S {
            return false;
        }
        self.static_layers[layer_idx].is_area_any_set(x, y, radius)
    }
}
