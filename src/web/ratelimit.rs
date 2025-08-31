use std::{
    collections::HashMap,
    net::{IpAddr, Ipv4Addr},
    sync::Arc,
};

use http::HeaderMap;
use parking_lot::Mutex;
use tokio::task::JoinHandle;
use tracing::{info, warn};

#[derive(Clone, Default)]
pub struct AppState {
    pathfinding_tasks: Arc<Mutex<HashMap<RatelimitIp, JoinHandle<()>>>>,
}

impl AppState {
    pub fn start_pathfinding_task(&self, headers: &HeaderMap, join_handle: JoinHandle<()>) {
        let mut pathfinding_ips = self.pathfinding_tasks.lock();
        let ip = ip_from_headers(headers);
        // only one task per RatelimitIp is allowed, so abort the existing one
        if let Some(existing_handle) = pathfinding_ips.get(&ip) {
            existing_handle.abort();
        }
        pathfinding_ips.insert(ip, join_handle);
    }

    pub fn stop_pathfinding_task(&self, headers: &HeaderMap) {
        let mut pathfinding_ips = self.pathfinding_tasks.lock();
        let ip = ip_from_headers(headers);
        if let Some(existing_handle) = pathfinding_ips.remove(&ip) {
            existing_handle.abort();
        }
    }
}

fn ip_from_headers(headers: &HeaderMap) -> RatelimitIp {
    let ip = headers
        .get("X-Forwarded-For")
        .and_then(|h| h.to_str().ok())
        .and_then(|h| h.split(',').next())
        .and_then(|h| h.parse::<IpAddr>().ok())
        .unwrap_or(Ipv4Addr::UNSPECIFIED.into());

    if ip.is_unspecified() {
        warn!("got request from unspecified ip!");
    } else {
        info!("got request from ip: {ip}");
    }

    RatelimitIp::from(ip)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RatelimitIp(u8, u8, u8, u8);
impl From<IpAddr> for RatelimitIp {
    fn from(ip: IpAddr) -> Self {
        match ip {
            IpAddr::V4(ipv4) => {
                let octets = ipv4.octets();
                RatelimitIp(octets[0], octets[1], octets[2], 0)
            }
            IpAddr::V6(ipv6) => {
                let octets = ipv6.octets();
                RatelimitIp(octets[0], octets[1], octets[2], octets[3])
            }
        }
    }
}
