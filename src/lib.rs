#[cfg(feature = "bevy")]
use bevy::prelude::*;
#[cfg(feature = "bevy")]
use lexaos_bitboard::{BitBoard, BitLayout, RowMajorLayout};
use std::marker::PhantomData;

pub mod spatial_hash;
pub use spatial_hash::SpatialHash;

use lexaos_core::config::SpatialHashInterface;

impl<E, const W: usize, const H: usize, const EK: usize, const SL: usize, L: BitLayout<W, H>> SpatialHashInterface<BitBoard<W, H, L>> for SpatialHash<E, W, H, EK, SL, L> 
where E: From<Entity> + Into<Entity> + Copy + Eq + std::hash::Hash + Send + Sync + 'static
{
    fn static_revision(&self) -> u32 {
        self.static_revision()
    }

    fn remove(&mut self, entity: Entity) {
        self.remove(E::from(entity));
    }

    fn update_entity(&mut self, entity: Entity, pos: Vec2, radius: f32, kind_idx: usize) {
        let (tile_x, tile_y) = BitBoard::<W, H, L>::pos_to_tile(pos.x, pos.y);
        self.update_with_threshold(E::from(entity), (tile_x, tile_y), radius as i32, kind_idx, 1);
    }

    fn sync_static_layer(&mut self, slot: usize, board: &dyn std::any::Any, revision: u32) {
        if let Some(board) = board.downcast_ref::<BitBoard<W, H, L>>() {
            self.full_sync_static_layer(slot, board, revision);
        }
    }

    fn mask_visibility(&self, x: i32, y: i32, radius: f32, opaque_layer_idx: usize) -> BitBoard<W, H, L> {
        self.mask_visibility(x, y, radius, opaque_layer_idx)
    }

    fn query_filtered_radius_callback(
        &self,
        pos: (i32, i32),
        radius: i32,
        exclude: Entity,
        kind_idx: Option<usize>,
        callback: &mut dyn FnMut(Entity),
    ) {
        self.query_filtered_radius_callback(pos, radius, E::from(exclude), kind_idx, &mut |candidate: E| {
            callback(candidate.into());
        });
    }

    fn query_mask_bounded_callback(
        &self,
        mask: &BitBoard<W, H, L>,
        kind_idx: Option<usize>,
        exclude: Entity,
        min_tile: (i32, i32),
        max_tile: (i32, i32),
        callback: &mut dyn FnMut(Entity),
    ) {
        self.query_mask_bounded_callback(mask, kind_idx, E::from(exclude), min_tile, max_tile, &mut |candidate: E| {
            callback(candidate.into());
        });
    }
}

/// 空間ハッシュで管理されるエンティティへの付与コンポーネント (汎用版)
#[cfg(feature = "bevy")]
#[derive(Component, Debug, Clone)]
pub struct SpatialManaged {
    /// エンティティの種別インデックス (SpatialHash の E レイヤーに対応)
    pub kind_idx: usize,
    pub radius: i32,
}

/// 空間ハッシュの同期と管理を行うプラグイン
pub struct SpatialGridPlugin<C: lexaos_core::config::WorldConfig>(PhantomData<C>);

#[cfg(feature = "bevy")]
impl<C: lexaos_core::config::WorldConfig> Default for SpatialGridPlugin<C> {
    fn default() -> Self {
        Self(PhantomData)
    }
}

#[cfg(feature = "bevy")]
impl<C: lexaos_core::config::WorldConfig> Plugin for SpatialGridPlugin<C> {
    fn build(&self, app: &mut App) {
        // SpatialHash is registered via WorldConfig in main.rs, 
        // or we could initialize it here if not exists.
        app.add_systems(Update, sync_spatial_hash_system::<C>);
    }
}

/// Transform の変化を監視し、空間ハッシュの内容を同期 (動的エンティティ)
#[cfg(feature = "bevy")]
fn sync_spatial_hash_system<C: lexaos_core::config::WorldConfig>(
    mut spatial_hash: ResMut<C::WorldSpatialHash>,
    query: Query<(Entity, &Transform, &SpatialManaged), Or<(Changed<Transform>, Added<SpatialManaged>)>>,
    mut removed: RemovedComponents<SpatialManaged>,
) {
    // 削除されたエンティティの除去
    for entity in removed.read() {
        spatial_hash.remove(entity);
    }

    // 移動または追加されたエンティティの座標更新
    for (entity, transform, managed) in query.iter() {
        spatial_hash.update_entity(entity, transform.translation.truncate(), managed.radius as f32, managed.kind_idx);
    }
}
