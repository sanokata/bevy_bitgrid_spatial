#[cfg(feature = "bevy")]
use bevy::prelude::*;
#[cfg(feature = "bevy")]
use bitgrid::BitLayout;
#[cfg(feature = "bevy")]
use std::hash::Hash;
#[cfg(feature = "bevy")]
use std::marker::PhantomData;

pub mod query_builder;
pub mod spatial_hash;

pub use query_builder::SpatialQuery;
pub use spatial_hash::SpatialHash;

/// Component attached to entities that are tracked by the spatial hash.
#[cfg(feature = "bevy")]
#[derive(Component, Debug, Clone, Copy, Reflect)]
pub struct SpatialManaged {
    /// Radius in tiles used for spatial registration.
    pub radius: f32,
    /// Entity kind index; determines which `kind_boards` layer this entity belongs to.
    pub kind_idx: usize,
}

/// Bevy plugin that registers a [`SpatialHash`] as a resource and keeps it in sync.
///
/// Inserts a default [`SpatialHash`] resource and schedules
/// [`spatial_hash_sync_system`] in `PostUpdate`.
#[cfg(feature = "bevy")]
pub struct SpatialHashPlugin<ID, const W: usize, const H: usize, const E: usize, const S: usize, L>
where
    L: BitLayout<W, H>,
{
    _phantom: PhantomData<(ID, L)>,
}

#[cfg(feature = "bevy")]
impl<ID, const W: usize, const H: usize, const E: usize, const S: usize, L> Default
    for SpatialHashPlugin<ID, W, H, E, S, L>
where
    L: BitLayout<W, H>,
{
    fn default() -> Self {
        Self {
            _phantom: PhantomData,
        }
    }
}

#[cfg(feature = "bevy")]
impl<ID, const W: usize, const H: usize, const E: usize, const S: usize, L> Plugin
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

/// `PostUpdate` system that keeps the spatial hash in sync with the ECS world.
///
/// - Removes entities that lost their [`SpatialManaged`] component (despawned or
///   explicitly removed).
/// - Updates moved or reconfigured entities using a 1-tile Chebyshev threshold so
///   that sub-tile jitter does not trigger unnecessary hash updates every frame.
#[cfg(feature = "bevy")]
fn spatial_hash_sync_system<ID, const W: usize, const H: usize, const E: usize, const S: usize, L>(
    mut spatial_hash: ResMut<SpatialHash<ID, W, H, E, S, L>>,
    query: Query<
        (Entity, &Transform, &SpatialManaged),
        Or<(Changed<Transform>, Changed<SpatialManaged>)>,
    >,
    mut removed: RemovedComponents<SpatialManaged>,
) where
    ID: From<Entity> + Copy + Eq + Hash + Send + Sync + 'static,
    L: BitLayout<W, H> + Send + Sync + 'static,
{
    // Clean up entities that were despawned or lost SpatialManaged.
    for entity in removed.read() {
        spatial_hash.remove(ID::from(entity));
    }

    // Sync entities whose Transform or SpatialManaged changed this frame.
    for (entity, transform, managed) in query.iter() {
        let pos = transform.translation.truncate();
        let tile_pos = L::point_to_coord((pos.x, pos.y));

        // Skip updates smaller than 1 tile (Chebyshev distance).
        spatial_hash.update_with_threshold(
            ID::from(entity),
            tile_pos,
            managed.radius as i32,
            managed.kind_idx,
            1,
        );
    }
}
