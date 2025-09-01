use internet_roadtrip_pathfinder::{db::DB, roadtrip_api, web};
use mimalloc::MiMalloc;

#[global_allocator]
static GLOBAL: MiMalloc = MiMalloc;

#[tokio::main]
async fn main() -> eyre::Result<()> {
    tracing_subscriber::fmt::init();

    // dereference the db to make sure it gets created
    let _ = &*DB;

    tokio::spawn(roadtrip_api::watch_websocket());
    web::serve().await;

    Ok(())
}
