use bevy_bitgrid_spatial::SpatialHash;
use criterion::{Criterion, black_box, criterion_group, criterion_main};
use rand::Rng;

type TestHash = SpatialHash<u32, 256, 256, 1, 1>;

fn bench_spatial_hash(c: &mut Criterion) {
    let mut rng = rand::thread_rng();

    // --- Benchmark: Insert ---
    c.bench_function("insert_1000_entities", |b| {
        b.iter(|| {
            let mut hash = TestHash::default();
            for i in 0..1000 {
                let x = rng.gen_range(0..256);
                let y = rng.gen_range(0..256);
                hash.insert(i, (x, y), 1, 0);
            }
            black_box(hash);
        });
    });

    // --- Benchmark: Update (Differential) ---
    let mut hash_for_update = TestHash::default();
    for i in 0..1000 {
        let x = rng.gen_range(0..256);
        let y = rng.gen_range(0..256);
        hash_for_update.insert(i, (x, y), 1, 0);
    }

    c.bench_function("update_1000_entities_small_move", |b| {
        b.iter(|| {
            for i in 0..1000 {
                // Small move likely to overlap with old position
                let dx = rng.gen_range(-1..=1);
                let dy = rng.gen_range(-1..=1);
                let (cx, cy, _) = hash_for_update.get_entity_info(i).unwrap();
                let new_pos = ((cx + dx).clamp(0, 255), (cy + dy).clamp(0, 255));
                hash_for_update.update_diff(i, new_pos, 1, 0);
            }
            black_box(&hash_for_update);
        });
    });

    // --- Benchmark: Query (Circle) ---
    let mut hash_for_query = TestHash::default();
    for i in 0..1000 {
        let x = rng.gen_range(0..256);
        let y = rng.gen_range(0..256);
        hash_for_query.insert(i, (x, y), 1, 0);
    }

    c.bench_function("query_circle_radius_10", |b| {
        b.iter(|| {
            let mut count = 0;
            let cx = rng.gen_range(0..256);
            let cy = rng.gen_range(0..256);
            hash_for_query.query().circle((cx, cy), 10.0, |_| {
                count += 1;
            });
            black_box(count);
        });
    });

    c.bench_function("query_circle_radius_50", |b| {
        b.iter(|| {
            let mut count = 0;
            let cx = rng.gen_range(0..256);
            let cy = rng.gen_range(0..256);
            hash_for_query.query().circle((cx, cy), 50.0, |_| {
                count += 1;
            });
            black_box(count);
        });
    });
}

criterion_group!(benches, bench_spatial_hash);
criterion_main!(benches);
