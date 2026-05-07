# bevy_bitgrid_spatial

A spatial hash implementation for the [Bevy](https://bevyengine.org/) game engine, leveraging the efficiency of [`bitgrid`](https://github.com/sanokata/bitgrid.rs).

## Features

- **Efficient Spatial Indexing**: Uses `bitgrid` for fast bitwise spatial operations.
- **Bevy Integration**: Simple plugin to track entities and perform spatial queries.
- **Configurable**: Generic over dimensions, grid layout, and entity ID type.
- **Automatic Sync**: `SpatialHashPlugin` keeps the spatial hash in sync with entity `Transform`s.

## Installation

Add this to your `Cargo.toml`:

```toml
[dependencies]
bevy_bitgrid_spatial = { git = "https://github.com/sanokata/bevy_bitgrid_spatial" }
```

## Usage

### 1. Register the Plugin

```rust
use bevy::prelude::*;
use bevy_bitgrid_spatial::{SpatialHashPlugin, SpatialHash};
use bitgrid::layouts::StandardLayout;

fn main() {
    App::new()
        .add_plugins(DefaultPlugins)
        // ID type, Width, Height, Entities per cell, Static layers, Layout
        .add_plugins(SpatialHashPlugin::<Entity, 128, 128, 4, 1, StandardLayout>::default())
        .run();
}
```

### 2. Add `SpatialManaged` to Entities

Entities with `SpatialManaged` and `Transform` will be automatically tracked.

```rust
use bevy_bitgrid_spatial::SpatialManaged;

fn setup(mut commands: Commands) {
    commands.spawn((
        Transform::from_xyz(10.0, 20.0, 0.0),
        SpatialManaged {
            radius: 1.0,
            kind_idx: 0,
        },
    ));
}
```

### 3. Query the Spatial Hash

```rust
use bevy_bitgrid_spatial::{SpatialHash, SpatialQuery};

fn query_system(spatial_hash: Res<SpatialHash<Entity, 128, 128, 4, 1, StandardLayout>>) {
    let mut found = Vec::new();
    spatial_hash.query()
        .with_kind(1) // Optional: filter by entity kind index
        .circle((10, 20), 5.0, |entity| {
            found.push(entity);
        });
    
    for entity in found {
        println!("Found entity: {:?}", entity);
    }
}
```

## License

Licensed under either of

 * Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
 * MIT license ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.

---
**Note**: Generative AI was used to assist in the development of this project. However, all code has been reviewed, tested, and the author takes full responsibility for the final quality and functionality
