#[cfg(feature = "bevy")]
use bevy::prelude::*;
#[cfg(feature = "bevy")]
use lexaos_bitboard::{BitBoard, BitLayout, RowMajorLayout};
use std::marker::PhantomData;

pub mod spatial_hash;
pub use spatial_hash::SpatialHash;

/// 空間ハッシュで管理されるエンティティへの付与コンポーネント (汎用版)
#[cfg(feature = "bevy")]
#[derive(Component, Debug, Clone)]
pub struct SpatialManaged {
    /// エンティティの種別インデックス (SpatialHash の E レイヤーに対応)
    pub kind_idx: usize,
    pub radius: i32,
}

/// 空間ハッシュの同期と管理を行うプラグイン
/// const E: エンティティ種別の数
/// const S: 静的レイヤーの数
/// L: メモリレイアウト
#[cfg(feature = "bevy")]
pub struct SpatialGridPlugin<const W: usize, const H: usize, const E: usize, const S: usize, L = RowMajorLayout>(PhantomData<L>);

#[cfg(feature = "bevy")]
impl<const W: usize, const H: usize, const E: usize, const S: usize, L: BitLayout<W, H>> Default for SpatialGridPlugin<W, H, E, S, L> {
    fn default() -> Self {
        Self(PhantomData)
    }
}

#[cfg(feature = "bevy")]
impl<const W: usize, const H: usize, const E: usize, const S: usize, L: BitLayout<W, H>> Plugin for SpatialGridPlugin<W, H, E, S, L> {
    fn build(&self, app: &mut App) {
        if !app.world().contains_resource::<SpatialHash<Entity, W, H, E, S, L>>() {
            app.insert_resource(SpatialHash::<Entity, W, H, E, S, L>::default());
        }
        app.add_systems(Update, sync_spatial_hash_system::<W, H, E, S, L>);
    }
}

/// Transform の変化を監視し、空間ハッシュの内容を同期 (動的エンティティ)
#[cfg(feature = "bevy")]
fn sync_spatial_hash_system<const W: usize, const H: usize, const E: usize, const S: usize, L: BitLayout<W, H>>(
    mut spatial_hash: ResMut<SpatialHash<Entity, W, H, E, S, L>>,
    query: Query<(Entity, &Transform, &SpatialManaged), Or<(Changed<Transform>, Added<SpatialManaged>)>>,
    mut removed: RemovedComponents<SpatialManaged>,
) {
    // 削除されたエンティティの除去
    for entity in removed.read() {
        spatial_hash.remove(entity);
    }

    // 移動または追加されたエンティティの座標更新
    for (entity, transform, managed) in query.iter() {
        let pos = transform.translation;
        let (tile_x, tile_y) = BitBoard::<W, H, L>::pos_to_tile(pos.x, pos.y);
        
        // スロットリングを活用: 1.0 タイル以上の移動がある場合のみ更新
        spatial_hash.update_with_threshold(entity, (tile_x, tile_y), managed.radius, managed.kind_idx, 1);
    }
}
