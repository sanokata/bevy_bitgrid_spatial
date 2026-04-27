use super::SpatialHash;
use core::hash::Hash;
use lexaos_bitboard::{BitBoard, BitLayout};

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

    /// 静的レイヤー全体を一括更新し、リビジョンを上げる
    pub fn full_sync_static_layer(
        &mut self,
        layer_idx: usize,
        board: &BitBoard<W, H, L>,
        revision: u32,
    ) {
        if layer_idx < S {
            self.static_layers[layer_idx] = board.clone();

            // 収縮済みキャッシュの更新 (radius=1, radius=2)
            self.eroded_layers[layer_idx][0] = board.erode(1);
            self.eroded_layers[layer_idx][1] = board.erode(2);

            self.static_revision = revision;
        }
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

    /// 指定した静的レイヤー (地形等) の範囲内がすべてセットされているか判定 (通行判定用)
    pub fn is_static_area_all_set(&self, layer_idx: usize, x: i32, y: i32, radius: i32) -> bool {
        if layer_idx >= S {
            return false;
        }
        match radius {
            0 => self.static_layers[layer_idx].get(x, y),
            1 => self.eroded_layers[layer_idx][0].get(x, y),
            2 => self.eroded_layers[layer_idx][1].get(x, y),
            _ => self.static_layers[layer_idx].is_area_all_set(x, y, radius),
        }
    }

    /// 指定した静的レイヤー (地形等) の範囲内に一つでもセットされたビットがあるか判定 (衝突判定用)
    pub fn is_static_area_any_set(&self, layer_idx: usize, x: i32, y: i32, radius: i32) -> bool {
        if layer_idx >= S {
            return false;
        }
        self.static_layers[layer_idx].is_area_any_set(x, y, radius)
    }
}
