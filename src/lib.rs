use bevy::prelude::*;
pub mod spatial_hash;
use spatial_hash::SpatialHash;

/// 空間ハッシュで管理されるエンティティへの付与コンポーネント
#[derive(Component, Debug, Clone)]
pub struct SpatialManaged {
    pub kind: spatial_hash::SpatialEntityKind,
    pub radius: i32,
}

/// 空間ハッシュの同期と管理を行うプラグイン
pub struct SpatialGridPlugin<const W: usize, const H: usize>;

impl<const W: usize, const H: usize> Plugin for SpatialGridPlugin<W, H> {
    fn build(&self, app: &mut App) {
        if !app.world().contains_resource::<SpatialHash<W, H>>() {
            app.insert_resource(SpatialHash::<W, H>::default());
        }
        app.add_systems(Update, sync_spatial_hash_system::<W, H>);
    }
}

/// Transform の変化を監視し、空間ハッシュの内容を同期
fn sync_spatial_hash_system<const W: usize, const H: usize>(
    mut spatial_hash: ResMut<SpatialHash<W, H>>,
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
        let tile_x = pos.x.floor() as i32;
        let tile_y = pos.y.floor() as i32;
        
        spatial_hash.update(entity, (tile_x, tile_y), managed.radius, managed.kind);
    }
}
