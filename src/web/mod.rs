use std::{collections::HashMap, env, sync::LazyLock};

use axum::{
    Json, Router,
    extract::{Path, Query},
    response::{IntoResponse, Response},
    routing::get,
};
use http::{Method, StatusCode, header};
use simd_json::json;
use tokio::{fs, net::TcpListener};
use tower_http::cors::CorsLayer;
use tracing::info;

use crate::{
    db::DB,
    model::{PanoId, SizedTile},
    web::ratelimit::AppState,
};

pub mod path;
pub mod ratelimit;

static SECRET: LazyLock<String> =
    LazyLock::new(|| env::var("PATHFINDER_SECRET").unwrap_or_default());

pub async fn serve() {
    let cors = CorsLayer::new()
        .allow_methods([Method::GET])
        .allow_origin(tower_http::cors::Any);

    let app = Router::new()
        .route("/path", get(path::get_path))
        .route("/stats", get(get_stats))
        .route("/slow-get-pano-id/{pano_id}", get(get_slow_get_pano_id))
        .route(
            "/internal-pano-id/{internal_pano_id}",
            get(get_internal_pano_id),
        )
        .route("/tile/{size}/{x}/{z}", get(get_tile))
        .route(
            "/meowing",
            get(|| async {
                let path = std::path::Path::new("static/index.html");
                let file = fs::read(path).await.unwrap();
                (StatusCode::OK, [(header::CONTENT_TYPE, "text/html")], file)
            }),
        )
        .route(
            "/pathfinder.user.js",
            get(|| async {
                let path = std::path::Path::new("static/pathfinder.user.js");
                let file = fs::read(path).await.unwrap();
                (
                    StatusCode::OK,
                    [(header::CONTENT_TYPE, "text/javascript")],
                    file,
                )
            }),
        )
        .layer(cors)
        .with_state(AppState::default());

    let port = env::var("PORT").unwrap_or_else(|_| "2397".to_string());

    let bind_to = format!("[::]:{port}");
    info!("binding to {bind_to}");
    let listener = TcpListener::bind(bind_to).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

async fn get_stats() -> Response {
    let tiles = DB
        .slow_list_tiles()
        .into_iter()
        .map(|tile| [tile.x, tile.y, tile.size as u32])
        .collect::<Vec<_>>();

    let pano_count = DB.get_pano_count();

    Json(json!({
        "panos": pano_count,
        "tiles": tiles,
    }))
    .into_response()
}

async fn get_slow_get_pano_id(
    Query(query): Query<HashMap<String, String>>,
    Path(pano_id): Path<u32>,
) -> String {
    if !SECRET.is_empty() {
        // in theory this is vulnerable to timing attacks, but the latency difference is
        // nanoseconds and it's impractical to exploit over the network so it's
        // acceptable here
        if query.get("key").cloned().unwrap_or_default() != *SECRET {
            return "incorrect key".to_string();
        }
    }

    let txn = DB.read_txn();
    for entry in DB.pano_ids_db.iter(&txn).unwrap() {
        let (candidate_pano_id_str, candidate_pano_id) = entry.unwrap();
        if candidate_pano_id == pano_id {
            return format!("{candidate_pano_id_str}\n");
        }
    }

    "no result\n".to_string()
}

async fn get_internal_pano_id(Path(pano_id): Path<String>) -> String {
    let txn = DB.read_txn();
    if let Some(pano_id) = DB.pano_ids_db.get(&txn, &pano_id).unwrap() {
        let getmetadata_res = DB.lookup_getmetadata_location_with_txn(&txn, &PanoId(pano_id));

        return format!("{pano_id}\n{getmetadata_res:?}\n");
    };

    "no result\n".to_string()
}

async fn get_tile(Path((size, x, y)): Path<(u8, u32, u32)>) -> impl IntoResponse {
    let res = DB.lookup_listentityphotos(&SizedTile { size, x, y });

    let res = if let Some(Some(res)) = res {
        Some(Some(res.to_vec()))
    } else {
        None
    };

    Json(res)
}
