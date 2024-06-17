use anyhow::bail;
use ureq::Error;

use crate::tasks::DaySummary;

use super::storage::TaskStorage;

pub struct APIStorage {
    pub uri: String,
}

impl TaskStorage for APIStorage {
    fn save(&self, task: &crate::tasks::Task) -> anyhow::Result<()> {
        let end_point = format!("{}/tasks/", self.uri);
        let _resp: String = ureq::post(&end_point).send_json(task)?.into_string()?;
        Ok(())
    }

    fn delete(&self, task: &crate::tasks::Task) -> anyhow::Result<()> {
        let end_point = format!("{}/tasks/{}", self.uri, task.ulid);
        let _resp: String = ureq::delete(&end_point).call()?.into_string()?;
        Ok(())
    }

    fn update(&self, task: &crate::tasks::Task) -> anyhow::Result<()> {
        let end_point = format!("{}/tasks/{}", self.uri, task.ulid);
        let resp = ureq::patch(&end_point).send_json(task);
        match resp {
            Ok(_) => Ok(()),
            Err(Error::Status(code, response)) => {
                bail!(
                    "Failed with status code: {}, and response: {:#?}",
                    code,
                    response.into_string()
                )
            }
            Err(err) => Err(err.into()),
        }
    }

    fn search_using_ulid(&self, ulid: &str) -> anyhow::Result<Vec<crate::tasks::Task>> {
        let end_point = format!("{}/tasks/search", self.uri);
        let res = ureq::get(&end_point)
            .query("ulid", ulid)
            .call()?
            .into_json()?;
        Ok(res)
    }

    fn next_tasks(&self, count: usize) -> anyhow::Result<Vec<crate::tasks::Task>> {
        let end_point = format!("{}/tasks/next/{}", self.uri, count);
        let response = ureq::get(&end_point).call()?;
        let tasks: Vec<crate::tasks::Task> = response.into_json()?;
        Ok(tasks)
    }

    fn summarize_day(&self) -> anyhow::Result<crate::tasks::DaySummary> {
        let end_point = format!("{}/tasks/summarize_day/", self.uri);
        let res: DaySummary = ureq::get(&end_point).call()?.into_json()?;
        Ok(res)
    }

    fn unsafe_query(&self, clause: &str) -> anyhow::Result<Vec<crate::tasks::Task>> {
        let end_point = format!("{}/tasks/unsafe_query/", self.uri);
        let res = ureq::get(&end_point)
            .query("clause", clause)
            .call()?
            .into_json()?;
        Ok(res)
    }
}

impl APIStorage {
    pub fn new(uri: String) -> Self {
        Self { uri }
    }
}
