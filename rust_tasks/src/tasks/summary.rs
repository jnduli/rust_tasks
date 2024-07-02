use std::collections::HashMap;

use chrono::{Duration, NaiveTime, Utc};
use serde::{Deserialize, Deserializer};

use crate::storage::storage::{DaySummaryResult, TaskStorage};

#[derive(Deserialize, Debug, PartialEq)]
pub struct SummaryConfig {
    start: NaiveTime,
    end: NaiveTime,
    #[serde(deserialize_with = "tags_deserialize")]
    tags: HashMap<String, Duration>,
    #[serde(deserialize_with = "duration_deserialize")]
    goal: Duration,
}

fn tags_deserialize<'de, D>(deserializer: D) -> Result<HashMap<String, Duration>, D::Error>
where
    D: Deserializer<'de>,
{
    #[derive(Deserialize)]
    struct Wrapper(#[serde(deserialize_with = "duration_deserialize")] Duration);

    let v = HashMap::<String, Wrapper>::deserialize(deserializer)?;
    Ok(v.into_iter().map(|(k, Wrapper(v))| (k, v)).collect())
}

fn duration_deserialize<'de, D>(deserializer: D) -> Result<Duration, D::Error>
where
    D: Deserializer<'de>,
{
    let str_sequence = String::deserialize(deserializer)?;
    println!("{}", str_sequence);
    let iso_duration = str_sequence.parse::<iso8601_duration::Duration>().unwrap();
    println!("{}", iso_duration);
    let chrono_duration = iso_duration.to_chrono().unwrap();
    Ok(chrono_duration)
}

impl Default for SummaryConfig {
    fn default() -> Self {
        Self {
            start: NaiveTime::from_hms_opt(5, 0, 0).unwrap(),
            end: NaiveTime::from_hms_opt(14, 0, 0).unwrap(),
            tags: HashMap::from([
                ("meeting".into(), Duration::minutes(30)),
                ("work".into(), Duration::minutes(30)),
            ]),
            goal: Duration::minutes(30),
        }
    }
}

impl SummaryConfig {
    pub fn relevant_tags(&self) -> Vec<String> {
        self.tags.keys().map(|x| x.into()).collect()
    }

    pub fn get_summary_stats(&self, summary_result: DaySummaryResult) -> anyhow::Result<()> {
        let total_due = summary_result.total_tasks;
        let done_tasks = summary_result.done_tasks;

        let ratio_done = (done_tasks as f32) / (total_due as f32);
        let now = Utc::now().time();
        let mut end_time = self.end;
        let mut non_tagged_counts = (total_due - done_tasks);
        for (tag, cnt) in summary_result.open_tags_count.iter() {
            let time_for_tag = self.tags.get(tag).unwrap();
            end_time -= time_for_tag.checked_mul(*cnt as i32).unwrap();
            non_tagged_counts -= cnt;
        }
        let minutes_per_task = {
            let delta = end_time - now;
            delta.num_minutes() / non_tagged_counts as i64
        };

        println!("Total: {}", total_due);
        println!("NotDone: {}", (total_due - done_tasks));
        println!("Done: {}", done_tasks);
        for (tag, cnt) in summary_result.open_tags_count.iter() {
            if cnt < &1 {
                continue;
            }
            let time_for_tag = self.tags.get(tag).unwrap();
            println!(
                "Tag.{} left (~{} mins): {}",
                tag,
                time_for_tag.num_minutes(),
                cnt
            );
        }
        println!("Ratio done: {:.2}", ratio_done);
        let mut color = "";
        if minutes_per_task < self.goal.num_minutes() {
            color = "\x1b[31m"
        }
        println!(
            "{}Minutes per task: {}\x1b[0m",
            color, minutes_per_task as i32
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use chrono::{Duration, NaiveTime};

    use super::SummaryConfig;

    #[test]
    fn deserialized_correctly() {
        let summary: SummaryConfig = toml::from_str(
            r#"
        start = "08:00"
        end = "17:00"
        tags.meeting = "PT30M"
        tags.work = "PT30M"
        goal = "PT30M"
        "#,
        )
        .unwrap();
        let expected = SummaryConfig {
            start: NaiveTime::from_hms_opt(8, 0, 0).unwrap(),
            end: NaiveTime::from_hms_opt(17, 0, 0).unwrap(),
            tags: HashMap::from([
                ("meeting".into(), Duration::minutes(30)),
                ("work".into(), Duration::minutes(30)),
            ]),
            goal: Duration::minutes(30),
        };
        assert_eq!(summary, expected);
    }
}
