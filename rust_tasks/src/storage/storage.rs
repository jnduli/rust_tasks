use anyhow::Result;


use crate::tasks::{DaySummary, Task};

pub trait TaskStorage {
    fn save(&self, task: &Task) -> Result<()>;
    fn delete(&self, task: &Task) -> Result<()>;
    fn update(&self, task: &Task) -> Result<()>;
    fn search_using_ulid(&self, ulid: &str) -> Result<Vec<Task>>;
    fn next_tasks(&self, count: usize) -> Result<Vec<Task>>;
    fn summarize_day(&self) -> Result<DaySummary>;
    // FIXME! remove this method
    fn unsafe_query(&self, clause: &str) -> Result<Vec<Task>>;
}
