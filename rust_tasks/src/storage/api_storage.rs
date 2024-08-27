use anyhow::anyhow;
use ureq::Error;

use std::collections::HashSet;

use crate::tasks::summary::SummaryConfig;

use super::storage::{DaySummaryResult, TaskStorage};

pub struct APIStorage {
    pub uri: String,
}

impl TaskStorage for APIStorage {
    fn save(&self, task: &crate::tasks::Task) -> anyhow::Result<()> {
        let end_point = format!("{}/tasks/", self.uri);
        ureq::post(&end_point)
            .send_json(task)
            .map_err(api_error_report)?
            .into_string()?;
        Ok(())
    }

    fn delete(&self, task: &crate::tasks::Task) -> anyhow::Result<()> {
        // FIXME: temporary soln that ensures syncs also works with delete and remove APIs
        let remote_task = self.search_using_ulid(&task.ulid)?;
        if remote_task.len() == 0 {
            self.save(task)?;
        }

        let end_point = format!("{}/tasks/{}", self.uri, task.ulid);
        ureq::delete(&end_point)
            .call()
            .map_err(api_error_report)?
            .into_string()?;
        Ok(())
    }

    fn update(&self, task: &crate::tasks::Task) -> anyhow::Result<()> {
        let end_point = format!("{}/tasks/{}", self.uri, task.ulid);
        ureq::patch(&end_point)
            .send_json(task)
            .map_err(api_error_report)?;
        Ok(())
    }

    fn search_using_ulid(&self, ulid: &str) -> anyhow::Result<Vec<crate::tasks::Task>> {
        let end_point = format!("{}/tasks/search", self.uri);
        let res = ureq::get(&end_point)
            .query("ulid", ulid)
            .call()
            .map_err(api_error_report)?
            .into_json()?;
        Ok(res)
    }

    fn next_tasks(&self, count: usize) -> anyhow::Result<Vec<crate::tasks::Task>> {
        let end_point = format!("{}/tasks/next/{}", self.uri, count);
        let response = ureq::get(&end_point).call().map_err(api_error_report)?;
        let tasks: Vec<crate::tasks::Task> = response.into_json()?;
        Ok(tasks)
    }

    fn summarize_day(&self, summary: &SummaryConfig) -> anyhow::Result<DaySummaryResult> {
        let end_point = format!("{}/tasks/summarize_day/", self.uri);
        let json_summary_config = serde_json::to_string(&summary)?;
        let res: DaySummaryResult = ureq::get(&end_point)
            .query("summary_config", &json_summary_config)
            .call()
            .map_err(api_error_report)?
            .into_json()?;
        Ok(res)
    }

    fn unsafe_query(&self, clause: &str) -> anyhow::Result<Vec<crate::tasks::Task>> {
        let end_point = format!("{}/tasks/unsafe_query/", self.uri);
        let res = ureq::get(&end_point)
            .query("clause", clause)
            .call()
            .map_err(api_error_report)?
            .into_json()?;
        Ok(res)
    }

    fn sync(&self, _task_storage: &dyn TaskStorage, _n_days: usize) -> anyhow::Result<()> {
        todo!()
    }

    fn deleted_ulids(&self, n_days: &usize) -> anyhow::Result<HashSet<String>> {
        let end_point = format!("{}/tasks/deleted_ulids/{}", self.uri, n_days);
        let res = ureq::get(&end_point)
            .call()
            .map_err(api_error_report)?
            .into_json()?;
        Ok(res)
    }
}

impl APIStorage {
    pub fn new(uri: String) -> Self {
        Self { uri }
    }
}

fn api_error_report(err: ureq::Error) -> anyhow::Error {
    match err {
        Error::Status(code, response) => {
            anyhow!(
                "Failed with status code: {}, and response:\n {:#?}",
                code,
                response.into_string()
            )
        }
        Error::Transport(t) => anyhow!(t.to_string()),
    }
}
