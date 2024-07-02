use std::collections::HashMap;

use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::tasks::summary::SummaryConfig;
use crate::tasks::Task;

#[derive(Serialize, Deserialize, Debug)]
pub struct DaySummaryResult {
    pub total_tasks: usize,
    pub done_tasks: usize,
    pub open_tags_count: HashMap<String, usize>,
}

pub trait TaskStorage {
    fn save(&self, task: &Task) -> Result<()>;
    fn delete(&self, task: &Task) -> Result<()>;
    fn update(&self, task: &Task) -> Result<()>;
    fn search_using_ulid(&self, ulid: &str) -> Result<Vec<Task>>;
    fn next_tasks(&self, count: usize) -> Result<Vec<Task>>;
    fn summarize_day(&self, summary: &SummaryConfig) -> Result<DaySummaryResult>;
    // FIXME! remove this method
    fn unsafe_query(&self, clause: &str) -> Result<Vec<Task>>;
}
