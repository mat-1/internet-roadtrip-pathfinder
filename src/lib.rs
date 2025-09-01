#![feature(stmt_expr_attributes)]
#![feature(iter_collect_into)]

use serde::{Deserialize, Serialize};

pub mod astar;
pub mod db;
pub mod math;
pub mod model;
pub mod roadtrip;
pub mod roadtrip_api;
pub mod streetview;
pub mod web;

pub struct ProgressUpdate {
    /// Between 0 and 1
    pub percent_done: f64,
    pub estimated_seconds_remaining: f64,
    pub best_path_cost: astar::Cost,
    pub nodes_considered: usize,
    pub best_path: Box<[[f32; 2]]>,
    pub current_path: Box<[[f32; 2]]>,
}

#[derive(Serialize, Deserialize)]
pub struct FullProgressUpdate {
    pub id: u32,

    /// Between 0 and 1
    pub percent_done: f64,
    pub estimated_seconds_remaining: f64,
    pub best_path_cost: astar::Cost,
    pub nodes_considered: usize,
    pub elapsed_seconds: f64,

    pub best_path_keep_prefix_length: usize,
    pub best_path_append: Box<[[f32; 2]]>,

    pub current_path_keep_prefix_length: usize,
    pub current_path_append: Box<[[f32; 2]]>,
}

impl Default for ProgressUpdate {
    fn default() -> Self {
        Self {
            percent_done: 0.,
            estimated_seconds_remaining: -1.,
            best_path_cost: 0 as astar::Cost,
            nodes_considered: 0,
            best_path: Box::new([]),
            current_path: Box::new([]),
        }
    }
}

impl FullProgressUpdate {
    pub fn clear(id: u32) -> Self {
        Self {
            id,
            // this makes it easy to check on the client
            percent_done: -1.,
            estimated_seconds_remaining: -1.,
            best_path_cost: 0 as astar::Cost,
            nodes_considered: 0,
            elapsed_seconds: 0.,
            best_path_keep_prefix_length: 0,
            best_path_append: Box::new([]),
            current_path_keep_prefix_length: 0,
            current_path_append: Box::new([]),
        }
    }
}
