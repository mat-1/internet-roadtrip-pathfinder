use std::{sync::Arc, time::Instant};

use parking_lot::Mutex;

use crate::{
    ProgressUpdate,
    astar::{self, PathSettings},
    model::Location,
};

pub async fn benchmark() {
    benchmark_astar().await;
    // benchmark_get_options().await;
}

async fn benchmark_astar() {
    // let start = Coords::new(43.774568171536906, -70.1485684515895);
    let start = Location::new_deg(43.509386511967435, -70.42994356288669);
    let heading = 20.;
    // let end = Coords::new(45.032485517243124, -73.45389367205885);
    let end = Location::new_deg(46.119524, -68.118604);

    let start_time = Instant::now();
    let _ = astar::astar(
        start,
        None,
        heading,
        end,
        Arc::new(Mutex::new(ProgressUpdate::default())),
        PathSettings {
            heuristic_factor: 1.5,
            no_long_jumps: false,
        },
    )
    .await
    .unwrap();

    println!("Pathfinder took: {:?}", start_time.elapsed());
}
