use std::{
    cmp::{self},
    collections::BinaryHeap,
    hash::{BuildHasherDefault, Hash, Hasher},
    sync::Arc,
    time::Instant,
};

use eyre::{OptionExt, bail};
use indexmap::{IndexMap, IndexSet};
use parking_lot::Mutex;
use rustc_hash::FxHasher;
use tracing::{debug, info};

use crate::{
    ProgressUpdate,
    db::DB,
    math::{self, approx_distance_sqr},
    model::{Location, Pano},
    roadtrip, streetview,
};

pub type FxIndexMap<K, V> = IndexMap<K, V, BuildHasherDefault<FxHasher>>;
pub type FxIndexSet<T> = IndexSet<T, BuildHasherDefault<FxHasher>>;

pub const MIN_HEURISTIC_FACTOR: f64 = 1.;
pub const RECOMMENDED_HEURISTIC_FACTOR: f64 = 3.3;
pub const MAX_HEURISTIC_FACTOR: f64 = 4.;

#[derive(Clone)]
pub struct PathSettings {
    pub heuristic_factor: f64,
    /// Disables portals/wormholes
    pub no_long_jumps: bool,
    /// Whether we should use the cache that returns the allowed options per
    /// node. This is meant for debugging/benchmarking purposes.
    pub use_option_cache: bool,
    /// A cost penalty that's applied when we go forward when there was more
    /// than 1 option.
    pub forward_penalty_on_intersections: Cost,
}

pub async fn astar(
    start: Location,
    start_pano_id: Option<String>,
    heading: f32,
    goal: Location,
    progress_update: Arc<Mutex<ProgressUpdate>>,
    settings: PathSettings,
) -> eyre::Result<Vec<NodeIdent>> {
    let start_pano = if let Some(start_pano_id) = start_pano_id {
        Pano {
            id: DB.get_pano_id(&start_pano_id),
            loc: start,
        }
    } else {
        streetview::get_nearest_pano(start, 500.)
            .await
            .unwrap_or_default()
            .ok_or_eyre("start position isn't near a pano")?
    };

    let start = NodeIdent {
        pano: start_pano,
        heading,
    };

    let mut open_set = BinaryHeap::new();
    open_set.push(WeightedNode {
        index: 0,
        g_score: 0 as Cost,
        f_score: 0 as Cost,
    });

    let mut nodes: FxIndexMap<NodeIdent, NodeData> = IndexMap::default();
    nodes.insert(
        start.clone(),
        NodeData {
            came_from: u32::MAX,
            g_score: 0 as Cost,
        },
    );

    let overall_heuristic = heuristic(&start, goal, settings.heuristic_factor);
    let overall_distance = math::distance(start.pano.loc, goal);

    let mut best_node_index = 0;
    let mut heuristic_of_best_node = Cost::MAX;

    info!("Path distance: {}km", overall_distance / 1000.);

    let mut nodes_considered = 0_usize;

    let start_time = Instant::now();

    let mut last_update = Instant::now();
    let mut last_log = Instant::now();

    let mut allow_turnaround = true;

    while let Some(WeightedNode { index, g_score, .. }) = open_set.pop() {
        nodes_considered += 1;

        let (node, node_data) = nodes.get_index(index as usize).unwrap();
        if is_goal_reached(node, goal) {
            info!("Found goal: {node:?}");
            info!("Pathfinder took: {:?}", start_time.elapsed());

            let route = reconstruct_path(&nodes, index);

            let mut progress_update = progress_update.lock();
            *progress_update = ProgressUpdate {
                percent_done: 1.,
                estimated_seconds_remaining: 0.,
                nodes_considered,
                best_path_cost: g_score,
                best_path: route.iter().map(|n| n.pano.loc.to_geojson()).collect(),
                current_path: Box::new([]),
            };

            info!("Cost: {g_score} ({} hours)", g_score / 3600.);
            info!("Nodes considered: {nodes_considered}");

            return Ok(route);
        }

        if g_score > node_data.g_score {
            // we know of a confirmed cheaper way to get to this node
            continue;
        }

        if (nodes_considered.is_multiple_of(1024) || nodes_considered < 1024)
            && last_update.elapsed().as_millis() > 100
        {
            // this is necessary to avoid blocking the thread if we're pathfinding fully
            // from cache
            tokio::task::yield_now().await;

            last_update = Instant::now();
            let percent = 1. - (heuristic_of_best_node as f64 / overall_heuristic as f64);

            let mut progress_update = progress_update.lock();

            // estimate time remaining
            let elapsed = start_time.elapsed();
            let estimated_remaining = (elapsed.as_secs_f64() / percent) - elapsed.as_secs_f64();

            if last_log.elapsed().as_secs() > 5 {
                last_log = Instant::now();
                info!(
                    "Visited {} nodes, best found: {:.2}%, distance remaining: {:.2}km",
                    nodes_considered,
                    percent * 100.,
                    (overall_distance as f64 * (1. - percent)) / 1000.,
                );
                info!(
                    "Estimated remaining time: {:.2} minutes",
                    estimated_remaining / 60.
                );
            }

            // debug!(
            //     "Estimated cost to best node by heuristic: {}",
            //     heuristic_of_best_node
            // );
            // debug!(
            //     "Actual cost to best node: {}",
            //     nodes.get_index(best_node_index).unwrap().1.g_score
            // );

            *progress_update = ProgressUpdate {
                percent_done: percent,
                estimated_seconds_remaining: estimated_remaining,
                best_path_cost: nodes.get_index(best_node_index as usize).unwrap().1.g_score,
                nodes_considered,
                best_path: reconstruct_path(&nodes, best_node_index)
                    .into_iter()
                    .map(|n| n.pano.loc.to_geojson())
                    .collect(),
                current_path: reconstruct_path(&nodes, index)
                    .into_iter()
                    .map(|n| n.pano.loc.to_geojson())
                    .collect(),
            };
        }

        let neighbors = roadtrip::get_options(
            &node.pano,
            node.heading,
            allow_turnaround,
            settings.use_option_cache,
        )
        .await?;

        if neighbors.turnaround {
            // we only allow the first attempted turnaround to work, since turnarounds are
            // only expected to be useful at the very beginning of a route.
            allow_turnaround = false;
        }

        let neighbor_count = neighbors.options.len();
        let node_loc = node.pano.loc;
        let node_heading = node.heading;
        let approx_lng_m_per_degree = if settings.no_long_jumps {
            node_loc.calculate_lng_m_per_degree()
        } else {
            // don't bother calculating it if we're not gonna use it
            0.
        };

        // the base delays are 5 and 9, but we add a little extra to account for
        // latency (these numbers were obtained by analyzing historical data)
        let base_neighbor_cost: Cost = match neighbor_count {
            1 => 5.875,
            _ => 9.625,
        };

        let mut is_likely_intersection_to_penalize = false;
        if neighbor_count > 1 && settings.forward_penalty_on_intersections > 0. {
            // if all of the options are forward-ish, don't count it as an intersection
            for neighbor in neighbors.options.iter() {
                let heading_diff = (neighbor.heading - node_heading).abs();
                if heading_diff > 30. {
                    is_likely_intersection_to_penalize = true;
                    break;
                }
            }
        }

        for (i, neighbor) in neighbors.options.into_iter().enumerate() {
            if settings.no_long_jumps {
                let neighbor_approx_distance_sqr =
                    approx_distance_sqr(node_loc, neighbor.pano.loc, approx_lng_m_per_degree);
                let jump_limit = 500.0_f64;
                if neighbor_approx_distance_sqr > jump_limit.powi(2) {
                    continue;
                }
            }

            let mut neighbor_cost = base_neighbor_cost;

            // tiebreaker, prefer going forwards (usually the first option)
            if i == 0 && neighbor_count > 1 {
                neighbor_cost -= 0.001;
            }

            if is_likely_intersection_to_penalize {
                let heading_diff = (neighbor.heading - node_heading).abs();
                if heading_diff < 30. {
                    neighbor_cost += settings.forward_penalty_on_intersections;
                }
            }

            let tentative_g_score = g_score + neighbor_cost;

            let neighbor_node = NodeIdent {
                pano: neighbor.pano,
                heading: neighbor.heading,
            };
            // let neighbor_heuristic = heuristic(&neighbor_node, goal);

            let neighbor_heuristic;
            let neighbor_index;

            match nodes.entry(neighbor_node) {
                indexmap::map::Entry::Occupied(mut e) => {
                    if tentative_g_score < e.get().g_score {
                        neighbor_heuristic = heuristic(e.key(), goal, settings.heuristic_factor);
                        neighbor_index = e.index() as u32;
                        e.insert(NodeData {
                            came_from: index,
                            g_score: tentative_g_score,
                        });
                    } else {
                        continue;
                    }
                }
                indexmap::map::Entry::Vacant(e) => {
                    // unknown neighbors have a default g_score of infinity, so we always "replace"
                    // them

                    neighbor_heuristic = heuristic(e.key(), goal, settings.heuristic_factor);
                    neighbor_index = e.index() as u32;
                    e.insert(NodeData {
                        came_from: index,
                        g_score: tentative_g_score,
                    });
                }
            }

            if neighbor_heuristic < heuristic_of_best_node {
                heuristic_of_best_node = neighbor_heuristic;
                best_node_index = neighbor_index;
            }

            open_set.push(WeightedNode {
                index: neighbor_index,
                g_score: tentative_g_score,
                f_score: tentative_g_score + neighbor_heuristic,
            });
        }
    }

    let mut progress_update = progress_update.lock();
    *progress_update = ProgressUpdate {
        percent_done: 1.,
        estimated_seconds_remaining: 0.,
        nodes_considered,
        best_path: Box::new([]),
        best_path_cost: 0 as Cost,
        current_path: Box::new([]),
    };

    bail!("No path found")
}

pub type Cost = f32;

fn reconstruct_path(nodes: &FxIndexMap<NodeIdent, NodeData>, mut current: u32) -> Vec<NodeIdent> {
    let mut full_path = Vec::new();
    while let Some((node, node_data)) = nodes.get_index(current as usize) {
        if node_data.came_from == u32::MAX {
            break;
        }

        current = node_data.came_from;
        full_path.push(node.clone());
    }
    full_path.push(nodes.get_index(current as usize).unwrap().0.clone());

    full_path.reverse();
    full_path
}

fn heuristic(current: &NodeIdent, goal: Location, factor: f64) -> Cost {
    (math::distance(current.pano.loc, goal) / factor) as Cost
}
fn is_goal_reached(node: &NodeIdent, goal: Location) -> bool {
    let dist = math::distance(node.pano.loc, goal);
    if dist < 30. {
        debug!("Node {node:?} is near goal {goal:?}: distance={dist}");
        if dist < 15. {
            return true;
        }

        // also check the location behind us by 15m, so if we're on a straight path that
        // skips lots of panos we can still find a good one
        let behind_loc = math::point_at_distance(node.pano.loc, node.heading + 180., 15.);
        let behind_dist = math::distance(behind_loc, goal);
        return behind_dist < 15.;
    }

    false
}

#[derive(Debug, Clone)]
pub struct NodeIdent {
    /// The ID is necessary to be able to get neighbors, and the location is
    /// necessary mostly for the heuristic to be able to work (and because part
    /// of the code for getting options depends on it).
    pub pano: Pano,
    /// In degrees. This is necessary because the pathfinder takes heading into
    /// consideration.
    pub heading: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct NodeData {
    pub came_from: u32,
    /// The cost of the currently known cheapest path from the start to this
    /// node
    pub g_score: Cost,
}

#[derive(Debug, Clone, PartialEq)]
pub struct WeightedNode {
    pub index: u32,
    pub g_score: Cost,
    pub f_score: Cost,
}

impl Hash for NodeIdent {
    fn hash<H: Hasher>(&self, state: &mut H) {
        // either id or loc could be used here, but PanoId hashes faster than Location
        self.pano.id.hash(state);
        self.heading.to_bits().hash(state);
    }
}
impl PartialEq for NodeIdent {
    fn eq(&self, other: &Self) -> bool {
        self.pano.id == other.pano.id && self.heading == other.heading
    }
}
impl Eq for NodeIdent {}

impl Ord for WeightedNode {
    #[inline]
    fn cmp(&self, other: &Self) -> cmp::Ordering {
        // intentionally inverted to make the BinaryHeap a min-heap
        other.f_score.total_cmp(&self.f_score)
    }
}
impl Eq for WeightedNode {}
impl PartialOrd for WeightedNode {
    #[inline]
    fn partial_cmp(&self, other: &Self) -> Option<cmp::Ordering> {
        Some(self.cmp(other))
    }
}
