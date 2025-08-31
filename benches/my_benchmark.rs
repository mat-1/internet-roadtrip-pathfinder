use std::hint::black_box;

use criterion::{BatchSize, Criterion, criterion_group, criterion_main};
use internet_roadtrip_pathfinder::{
    math::{self, angle::Angle},
    model::Location,
};
use rand::Rng;

fn create_random_nearby_locations() -> Vec<Location> {
    let mut rng = rand::rng();
    let initial = Location::new(
        Angle::from_bits(rng.random()),
        Angle::from_bits(rng.random()),
    );

    let mut locations = vec![initial];

    for _ in 0..127 {
        let offset_lat = Angle::from_bits(rng.random_range(-10000..10000));
        let offset_lng = Angle::from_bits(rng.random_range(-10000..10000));
        locations.push(Location {
            lat: initial.lat + offset_lat,
            lng: initial.lng + offset_lng,
        })
    }

    locations
}

fn criterion_benchmark(c: &mut Criterion) {
    c.bench_function("distance", |b| {
        b.iter_batched(
            create_random_nearby_locations,
            |l| {
                for location in &l {
                    black_box(math::distance(l[0], *location));
                }
            },
            BatchSize::SmallInput,
        );
    });

    c.bench_function("underestimate_distance_sqr", |b| {
        b.iter_batched(
            create_random_nearby_locations,
            |l| {
                let approx_lng_m_per_degree = l[0].calculate_lng_m_per_degree();

                for location in &l {
                    black_box(math::underestimate_distance_sqr(
                        l[0],
                        *location,
                        approx_lng_m_per_degree,
                    ));
                }
            },
            BatchSize::SmallInput,
        );
    });
}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);
