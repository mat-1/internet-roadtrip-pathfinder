use std::time::{Duration, Instant};

use futures::StreamExt;
use simd_json::derived::ValueTryAsScalar;
use tokio::time::sleep;
use tokio_tungstenite::{
    connect_async,
    tungstenite::{self, client::IntoClientRequest},
};
use tracing::{debug, error, info, warn};

use crate::{model::Location, streetview::reset_cache_nearby};

const WEBSOCKET_URL: &str = "wss://internet-roadtrip-listen-eqzms.ondigitalocean.app";
const CLEAR_CACHE_INTERVAL_SECONDS: u64 = 60 * 3;

pub async fn watch_websocket() {
    let mut last_cache_cleared = Instant::now();

    // wait some time before connecting to avoid spamming connections if we're
    // repeatedly restarting the pathfinder
    sleep(Duration::from_secs(CLEAR_CACHE_INTERVAL_SECONDS)).await;

    loop {
        let request = WEBSOCKET_URL.into_client_request().unwrap();
        let Ok((mut stream, response)) = connect_async(request).await else {
            warn!("Failed to connect to IRT WebSocket, retrying");
            sleep(Duration::from_secs(10)).await;
            continue;
        };

        info!("Connected to IRT WebSocket: {}", response.status());

        while let Some(message) = stream.next().await {
            match message {
                Ok(msg) => {
                    if let Err(e) = handle_message(msg, &mut last_cache_cleared).await {
                        error!("Error handling IRT WebSocket message: {e}");
                    }
                }
                Err(e) => {
                    error!("IRT WebSocket error: {}", e);
                    break;
                }
            }
        }

        sleep(Duration::from_secs(10)).await;
    }
}

async fn handle_message(
    msg: tungstenite::Message,
    last_cache_cleared: &mut Instant,
) -> eyre::Result<()> {
    let text = msg.to_text()?;
    if last_cache_cleared.elapsed().as_secs() < CLEAR_CACHE_INTERVAL_SECONDS {
        return Ok(());
    }

    debug!("Clearing cache around car");

    let data = simd_json::from_slice::<simd_json::OwnedValue>(&mut text.as_bytes().to_vec())?;
    let cur_lat = data["lat"].try_as_f64()?;
    let cur_lng = data["lng"].try_as_f64()?;

    let start = Instant::now();
    reset_cache_nearby(Location::new_deg(cur_lat, cur_lng), 1000.).await?;
    let end = Instant::now();
    debug!("Cache cleared in {:?}", end.duration_since(start));
    *last_cache_cleared = end;

    Ok(())
}
