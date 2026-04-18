use bevy::prelude::*;
use lexaos_bitboard::BitBoard;
pub mod spatial_hash;
use spatial_hash::SpatialHash;

/// 空間ハッシュで管理されるエンティティへの付与コンポーネント (汎用版)
#[derive(Component, Debug, Clone)]
pub struct SpatialManaged {
    /// エンティティの種別インデックス (SpatialHash の E レイヤーに対応)
    pub kind_idx: usize,
    pub radius: i32,
}

/// 空間ハッシュの同期と管理を行うプラグイン
/// const E: エンティティ種別の数
/// const S: 静的レイヤーの数
pub struct SpatialGridPlugin<const W: usize, const H: usize, const E: usize, const S: usize>;

impl<const W: usize, const H: usize, const E: usize, const S: usize> Plugin for SpatialGridPlugin<W, H, E, S> {
    fn build(&self, app: &mut App) {
        if !app.world().contains_resource::<SpatialHash<W, H, E, S>>() {
            app.insert_resource(SpatialHash::<W, H, E, S>::default());
        }
        app.add_systems(Update, sync_spatial_hash_system::<W, H, E, S>);
    }
}

/// Transform の変化を監視し、空間ハッシュの内容を同期 (動的エンティティ)
fn sync_spatial_hash_system<const W: usize, const H: usize, const E: usize, const S: usize>(
    mut spatial_hash: ResMut<SpatialHash<W, H, E, S>>,
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
        let (tile_x, tile_y) = BitBoard::<W, H>::pos_to_tile(pos.x, pos.y);
        
        spatial_hash.update(entity, (tile_x, tile_y), managed.radius, managed.kind_idx);
    }
}
