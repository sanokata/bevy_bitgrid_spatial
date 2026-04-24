#[cfg(feature = "bevy")]
use bevy::prelude::*;

pub mod spatial_hash;
pub mod query_builder;

pub use spatial_hash::SpatialHash;
pub use query_builder::SpatialQuery;

/// 空間ハッシュによって管理されるエンティティに付与するコンポーネント
#[cfg(feature = "bevy")]
#[derive(Component, Debug, Clone, Copy, Reflect)]
pub struct SpatialManaged {
    pub radius: f32,
    pub kind_idx: usize,
}

/// 空間ハッシュの同期と管理を行う Bevy プラグイン
#[cfg(feature = "bevy")]
pub struct SpatialHashPlugin<ID, const W: usize, const H: usize, const E: usize, const S: usize, L: BitLayout<W, H>> {
    _phantom: PhantomData<(ID, L)>,
}

#[cfg(feature = "bevy")]
impl<ID, const W: usize, const H: usize, const E: usize, const S: usize, L: BitLayout<W, H>> Default 
    for SpatialHashPlugin<ID, W, H, E, S, L> 
{
    fn default() -> Self {
        Self { _phantom: PhantomData }
    }
}

#[cfg(feature = "bevy")]
impl<ID, const W: usize, const H: usize, const E: usize, const S: usize, L: BitLayout<W, H>> Plugin 
    for SpatialHashPlugin<ID, W, H, E, S, L>
where 
    ID: From<Entity> + Copy + Eq + Hash + Send + Sync + 'static,
    L: BitLayout<W, H> + Send + Sync + 'static,
{
    fn build(&self, app: &mut App) {
        app.register_type::<SpatialManaged>()
           .insert_resource(SpatialHash::<ID, W, H, E, S, L>::default())
           .add_systems(PostUpdate, spatial_hash_sync_system::<ID, W, H, E, S, L>);
    }
}

/// Transform の変更を空間ハッシュに同期するシステム
#[cfg(feature = "bevy")]
fn spatial_hash_sync_system<ID, const W: usize, const H: usize, const E: usize, const S: usize, L: BitLayout<W, H>>(
    mut spatial_hash: ResMut<SpatialHash<ID, W, H, E, S, L>>,
    query: Query<(Entity, &Transform, &SpatialManaged), Changed<Transform>>,
) where 
    ID: From<Entity> + Copy + Eq + Hash + Send + Sync + 'static,
    L: BitLayout<W, H> + Send + Sync + 'static,
{
    for (entity, transform, managed) in query.iter() {
        let pos = transform.translation.truncate();
        let tile_pos = L::world_to_tile((pos.x, pos.y));
        // 位置の更新（閾値1タイルで更新）
        spatial_hash.update_with_threshold(
            ID::from(entity), 
            tile_pos, 
            managed.radius as i32, 
            managed.kind_idx, 
            1
        );
    }
}
