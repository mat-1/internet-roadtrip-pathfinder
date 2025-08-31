use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use axum::{
    extract::{
        State, WebSocketUpgrade,
        ws::{self, WebSocket},
    },
    response::IntoResponse,
};
use futures::{SinkExt, StreamExt, channel::mpsc};
use http::HeaderMap;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use tokio::{task::JoinSet, time::sleep};
use tracing::{debug, error, info};

use crate::{
    FullProgressUpdate, ProgressUpdate,
    astar::{
        self, MAX_HEURISTIC_FACTOR, MIN_HEURISTIC_FACTOR, PathSettings,
        RECOMMENDED_HEURISTIC_FACTOR,
    },
    math,
    model::{Location, Pano},
    streetview::get_nearest_pano,
    web::ratelimit::AppState,
};

#[derive(Deserialize)]
#[serde(tag = "kind")]
#[serde(rename_all = "snake_case")]
enum ServerboundMessage {
    Path(GetPathQuery),
    /// Stop calculating the current path.
    Abort {
        #[serde(default)]
        id: u32,
    },
}

#[derive(Deserialize)]
struct GetPathQuery {
    #[serde(default)]
    id: u32,
    start: [f64; 2],
    /// Optionally allows us to set the start pano ID, which makes it not snap
    /// the coordinates to the nearest pano.
    #[serde(default)]
    start_pano: Option<String>,
    end: [f64; 2],
    heading: f32,
    #[serde(default)]
    stops: Vec<[f64; 2]>,

    #[serde(default = "get_recommended_heuristic_factor")]
    heuristic_factor: f64,
    #[serde(default)]
    no_long_jumps: bool,
}
fn get_recommended_heuristic_factor() -> f64 {
    RECOMMENDED_HEURISTIC_FACTOR
}

pub async fn get_path(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(socket, state, headers))
}

#[derive(Serialize)]
#[serde(tag = "type")]
#[serde(rename_all = "snake_case")]
pub enum SocketEvent {
    Progress(FullProgressUpdate),
    Error { message: String },
}

async fn handle_socket(socket: WebSocket, state: AppState, headers: HeaderMap) {
    let (mut sender, mut receiver) = socket.split();

    info!("/path websocket opened");

    let (tx, rx) = mpsc::channel::<SocketEvent>(1);

    let is_pathing = Arc::new(Mutex::new(false));

    let is_pathing_ = is_pathing.clone();
    let task = tokio::spawn(async move {
        let is_pathing = is_pathing_;
        let mut rx = rx;
        while let Some(msg) = rx.next().await {
            match &msg {
                SocketEvent::Progress(progress) => {
                    if progress.percent_done == 1. || progress.percent_done < 0. {
                        *is_pathing.lock() = false;
                        info!("percent done is {}", progress.percent_done);
                    }
                }
                SocketEvent::Error { .. } => {
                    *is_pathing.lock() = false;
                }
            }

            let msg = simd_json::to_string(&msg)
                .unwrap_or_else(|_| "Error serializing message".to_string());
            let _ = sender.send(ws::Message::text(msg)).await;
        }
    });

    while let Some(msg) = receiver.next().await {
        let Ok(msg) = msg else {
            break;
        };
        if let ws::Message::Close(_) = msg {
            break;
        }

        if let ws::Message::Ping(_) | ws::Message::Pong(_) = msg {
            continue;
        }

        *is_pathing.lock() = true;
        let task = tokio::spawn(handle_socket_message(tx.clone(), msg));

        state.start_pathfinding_task(&headers, task);
    }

    info!("Socket closed!");
    task.abort();
    if *is_pathing.lock() {
        // only abort the task if the pathfinding task was started by this websocket
        state.stop_pathfinding_task(&headers);
    }
}

async fn send_error(tx: &mut mpsc::Sender<SocketEvent>, error: &str) {
    let _ = tx
        .send(SocketEvent::Error {
            message: error.to_string(),
        })
        .await;
}

async fn handle_socket_message(mut tx: mpsc::Sender<SocketEvent>, msg: ws::Message) {
    let Ok(msg) = msg.to_text() else {
        return send_error(&mut tx, "Message must be UTF-8").await;
    };

    // let Ok(msg) = simd_json::from_slice::<ServerboundMessage>(&mut
    // msg.to_owned().into_bytes()) else {
    //     return send_error(&mut tx, "Message must be valid query").await;
    // };
    let msg = match simd_json::from_slice::<ServerboundMessage>(&mut msg.to_owned().into_bytes()) {
        Ok(msg) => msg,
        Err(_) => {
            return send_error(&mut tx, &format!("Message must be valid query: '{msg}'")).await;
        }
    };

    match msg {
        ServerboundMessage::Path(get_path_query) => {
            handle_get_path_query(&mut tx, get_path_query).await;
        }
        ServerboundMessage::Abort { id } => {
            // we already implicitly stopped calculating a path, since
            // start_pathfinding_task is called whenever we receive a websocket
            // message, and that function makes sure that only one task exists
            // per IP

            // this is just to make sure that the latest message the client received from us
            // was to clear the path
            let _ = tx
                .send(SocketEvent::Progress(FullProgressUpdate::clear(id)))
                .await;
        }
    }
}

async fn handle_get_path_query(tx: &mut mpsc::Sender<SocketEvent>, msg: GetPathQuery) {
    let start = Location::from_latlng(msg.start);
    let end = Location::from_latlng(msg.end);
    let heading = msg.heading;
    let stops = msg
        .stops
        .into_iter()
        .map(Location::from_latlng)
        .collect::<Vec<_>>();

    let heuristic_factor = msg
        .heuristic_factor
        .clamp(MIN_HEURISTIC_FACTOR, MAX_HEURISTIC_FACTOR);
    let no_long_jumps = msg.no_long_jumps;

    if stops.len() > 200 {
        return send_error(tx, "Too many stops (limit of 200)").await;
    }

    // internet roadtrip sometimes has negative headings, just normalize it here
    let heading = (heading + 360.) % 360.;

    info!("/path {start} -> {end} heading {heading}");

    let mut next_stops = stops.clone();
    next_stops.push(end);

    // validate all the stops to make sure there's panos there
    for stop in &mut next_stops {
        let snap_to = snap_end_point_to_pano(*stop).await;
        let Some(snap_to) = snap_to else {
            return send_error(tx, &format!("No nearby pano for {stop}")).await;
        };
        *stop = snap_to.loc;
    }

    // validate total distance
    let mut cur = start;
    let mut total_distance = 0.;
    for &stop in &next_stops {
        let distance = math::distance(cur, stop);
        total_distance += distance;
        cur = stop;
    }
    if total_distance > 1_000_000. {
        return send_error(
            tx,
            &format!(
                "Your path is more than 1000km long ({}km), please segment your path instead.",
                (total_distance / 1000.) as u32
            ),
        )
        .await;
    }

    let mut progress_updates = Vec::<Arc<Mutex<ProgressUpdate>>>::new();

    let mut cur = start;
    let mut previous_stop = None;
    let mut task_set = JoinSet::new();
    for (i, stop) in next_stops.iter().enumerate() {
        let progress = ProgressUpdate::default();
        let progress_update = Arc::new(Mutex::new(progress));
        progress_updates.push(progress_update.clone());
        let assumed_heading = if i == 0 {
            heading
        } else if let Some(previous_stop) = previous_stop {
            math::calculate_heading(previous_stop, cur)
        } else {
            error!("No previous stop!");
            heading
        };

        info!("pathing from {cur} to {stop} with heading {assumed_heading}",);

        // only makes sense for the first stop in the path
        let start_pano_id = if i == 0 { msg.start_pano.clone() } else { None };

        let stop = *stop;
        task_set.spawn(async move {
            let result = astar::astar(
                cur,
                start_pano_id,
                assumed_heading,
                stop,
                progress_update,
                PathSettings {
                    heuristic_factor,
                    no_long_jumps,
                },
            )
            .await;
            if let Err(err) = result {
                error!("{err}");
            }
        });

        previous_stop = Some(cur);
        cur = stop;
    }

    // check for updates every second

    let start = Instant::now();

    let mut last_combined_best_path = vec![];
    let mut last_combined_current_path = vec![];

    loop {
        sleep(Duration::from_millis(100)).await;

        let mut reached_unfinished_path = false;

        let mut lowest_percent_done = 1.0_f64;
        let mut highest_estimated_seconds_remaining = 0.0_f64;
        let mut best_path_cost = 0 as astar::Cost;
        let mut nodes_considered = 0_usize;
        let mut combined_best_path = Vec::<[f32; 2]>::new();
        let mut combined_current_path = Vec::<[f32; 2]>::new();
        for progress_update in &progress_updates {
            let progress = progress_update.lock();

            lowest_percent_done = lowest_percent_done.min(progress.percent_done);
            highest_estimated_seconds_remaining =
                highest_estimated_seconds_remaining.max(progress.estimated_seconds_remaining);
            nodes_considered += progress.nodes_considered;

            if !reached_unfinished_path {
                best_path_cost += progress.best_path_cost;
                combined_best_path.extend(progress.best_path.iter());
                combined_current_path.extend(progress.current_path.iter());
            }

            if progress.percent_done < 1. {
                reached_unfinished_path = true;
            }
        }

        let (best_path_keep_prefix_length, best_path_append) =
            find_path_prefix_and_append(&last_combined_best_path, &combined_best_path);
        let (current_path_keep_prefix_length, current_path_append) =
            find_path_prefix_and_append(&last_combined_current_path, &combined_current_path);

        last_combined_best_path = combined_best_path;
        last_combined_current_path = combined_current_path;

        if tx
            .send(SocketEvent::Progress(FullProgressUpdate {
                id: msg.id,
                percent_done: lowest_percent_done,
                estimated_seconds_remaining: highest_estimated_seconds_remaining,
                best_path_cost,
                nodes_considered,
                elapsed_seconds: start.elapsed().as_secs_f64(),
                best_path_keep_prefix_length,
                best_path_append,
                current_path_keep_prefix_length,
                current_path_append,
            }))
            .await
            .is_err()
        {
            debug!("Failed to send progress update, aborting pathfinding.");
            task_set.abort_all();
            return;
        }

        if lowest_percent_done == 1. {
            info!(
                "Total cost: {best_path_cost} ({} hours)",
                best_path_cost / 3600.
            );

            break;
        }
    }

    info!("Pathfinding complete! waiting for tasks to finish");
    task_set.join_all().await;
    info!("Pathfinding complete!");
}

fn find_path_prefix_and_append(
    old_path: &[[f32; 2]],
    new_path: &[[f32; 2]],
) -> (usize, Box<[[f32; 2]]>) {
    let mut prefix_len = 0;

    for i in 0..old_path.len().min(new_path.len()) {
        if old_path[i] != new_path[i] {
            break;
        }
        prefix_len += 1;
    }

    let to_append = new_path[prefix_len..].to_vec().into_boxed_slice();

    (prefix_len, to_append)
}

/// Finds the closest non-photosphere pano near the given coordinates, intended
/// to be used for determining the end pano in a path.
pub async fn snap_end_point_to_pano(loc: Location) -> Option<Pano> {
    // check at different distances to avoid having to download every nearby tile if
    // there's already a pano immediately nearby
    for distance in [100., 500., 1000., 2000.] {
        let nearest_pano = get_nearest_pano(loc, distance).await.unwrap_or_default();
        if let Some(p) = nearest_pano {
            return Some(p);
        }
    }

    None
}
