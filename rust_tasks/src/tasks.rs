use std::env::var;
use std::fs;
use std::io::Write;
use std::ops::Add;
use std::process::Command;

use anyhow::{bail, Result};
use chrono::{
    DateTime, Datelike, Duration, Local, NaiveDate, NaiveDateTime, TimeZone, Utc, Weekday,
};
use serde::{Deserialize, Serialize};
use tempfile::Builder;
use termcolor::{Color, ColorChoice, ColorSpec, StandardStream, WriteColor};
use ulid::Ulid;

use self::display_utils::show_tasks_table;

use crate::storage::storage::TaskStorage;

pub mod add_utils;
pub mod display_utils;
pub mod edit_utils;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Task {
    pub ulid: String,
    pub body: String,
    pub modified_utc: Option<String>,
    pub ready_utc: Option<String>,
    pub due_utc: Option<String>,
    pub closed_utc: Option<String>,
    pub recurrence_duration: Option<String>,
    pub priority_adjustment: Option<f64>,
    pub user: Option<String>,
    pub metadata: Option<String>,
    pub tags: Option<Vec<String>>,
}

impl Default for Task {
    fn default() -> Self {
        let ulid = Ulid::new();
        Task {
            ulid: ulid.to_string().to_lowercase(),
            body: "".to_string(),
            // compatibility with tlite, temporary
            user: Some("rookie".to_string()),
            modified_utc: None,
            due_utc: None,
            ready_utc: None,
            closed_utc: None,
            recurrence_duration: None,
            priority_adjustment: None,
            metadata: None,
            tags: None,
        }
    }
}

impl Task {
    fn next_task(&self) -> Option<Task> {
        match &self.recurrence_duration {
            None => None,
            Some(x) => {
                let due_chrono = NaiveDateTime::parse_from_str(
                    self.due_utc.as_ref().unwrap().as_str(),
                    "%Y-%m-%d %H:%M:%S",
                )
                .unwrap()
                .and_utc();

                // only support P1DJ for now
                let mut duration_string = x.to_string();
                if x.ends_with("1JD") {
                    duration_string = match due_chrono.weekday() {
                        Weekday::Fri => "P3D".to_string(),
                        Weekday::Sat => "P2D".to_string(),
                        _ => "P1D".to_string(),
                    }
                }

                let duration = duration_string
                    .parse::<iso8601_duration::Duration>()
                    .unwrap();
                // let duration = match x {
                //     "P1DJ" => "P1D".parse():
                // x.parse::<iso8601_duration::Duration>().unwrap();
                // }
                let chrono_duration = duration.to_chrono_at_datetime(due_chrono);

                let new_due_date = due_chrono
                    .add(chrono_duration)
                    .format("%Y-%m-%d %H:%M:%S")
                    .to_string();
                // Temporary fix that assumes all my ready dates happen on the same date as due_utc
                let new_ready_date = &self.ready_utc.as_ref().map(|x| {
                    NaiveDateTime::parse_from_str(x, "%Y-%m-%d %H:%M:%S")
                        .unwrap()
                        .and_utc()
                        .add(chrono_duration)
                        .format("%Y-%m-%d %H:%M:%S")
                        .to_string()
                });
                let default_task = Task::default();
                let mut new_task = self.clone();
                new_task.ulid = default_task.ulid;
                new_task.due_utc = Some(new_due_date);
                new_task.ready_utc.clone_from(new_ready_date);
                Some(new_task)
            }
        }
    }

    pub fn undo_task(&mut self, storage: &impl TaskStorage) -> Result<()> {
        match self.closed_utc {
            Some(_) => {
                self.closed_utc = None;
                storage.update(self)?;
                Ok(())
            }
            None => Ok(()),
        }
    }

    pub fn do_task(&mut self, storage: &dyn TaskStorage) -> Result<()> {
        match self.closed_utc {
            Some(_) => {
                println!("Task already closed");
                Ok(())
            }
            None => {
                if let Some(x) = self.next_task() {
                    storage.save(&x)?;
                }
                self.closed_utc = Some(Utc::now().format("%Y-%m-%d %H:%M:%S").to_string());
                storage.update(self)?;
                Ok(())
            }
        }
    }

    fn from_yaml(yml: &str) -> Task {
        serde_yaml::from_str(yml).unwrap()
    }

    fn to_yaml(&self) -> String {
        serde_yaml::to_string(self).unwrap()
    }

    fn update_to_db(&self, storage: &dyn TaskStorage) -> Result<()> {
        storage.update(self)
    }

    fn save_to_db(&self, storage: &dyn TaskStorage) -> Result<()> {
        storage.save(self)
    }

    fn edit_with_editor(&mut self) -> Result<()> {
        let yml = self.to_yaml();

        let mut tempfile = Builder::new().suffix(".yml").tempfile()?;
        write!(tempfile, "{}", yml)?;

        let editor = var("EDITOR").unwrap_or("vim".to_string());
        Command::new(editor).arg(tempfile.path()).status()?;

        let contents = fs::read_to_string(tempfile)?;
        let task = Task::from_yaml(contents.as_str());
        if task.ulid != self.ulid {
            panic!("ERROR: Changing the ulid is not allowed.");
        }
        *self = task;
        Ok(())
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub struct DaySummary {
    pub total_tasks: usize,
    pub done_tasks: usize,
    pub remaining_meetings: usize,
}

pub fn experiment() -> Result<()> {
    println!("No experiment running");
    todo!()
}

pub fn query(storage: &dyn TaskStorage, clause: &str) -> Result<()> {
    let tasks = storage.unsafe_query(clause)?;
    show_tasks_table(&tasks)
}

pub fn quick_clean(storage: &dyn TaskStorage, date: &str) -> Result<()> {
    // ensure date is a valid object
    let _date_to_clean = NaiveDate::parse_from_str(date, "%Y-%m-%d")
        .unwrap_or_else(|_| panic!("Expected date like `2024-10-23` but found {}", date));

    let clause =
        format!("WHERE DATE(due_utc) = '{date}' AND DATE(closed_utc) IS NULL ORDER BY due_utc ASC");

    let mut tasks = storage.unsafe_query(&clause)?;

    let today_date = Local::now()
        .naive_utc()
        .and_utc()
        .format("%Y-%m-%d")
        .to_string();

    for task in tasks.iter_mut() {
        match &task.recurrence_duration {
            None => {
                let new_due = task.due_utc.clone().map(|x| {
                    let date_part = x.split(' ').collect::<Vec<&str>>()[0];
                    x.replace(date_part, today_date.as_str())
                });
                let new_ready = task.ready_utc.clone().map(|x| {
                    let date_part = x.split(' ').collect::<Vec<&str>>()[0];
                    x.replace(date_part, today_date.as_str())
                });
                task.due_utc = new_due;
                task.ready_utc = new_ready;
                task.update_to_db(storage)?;
            }
            Some(_) => {
                let potential_next_task = task.next_task().unwrap();
                task.due_utc = potential_next_task.due_utc;
                task.ready_utc = potential_next_task.ready_utc;
                task.update_to_db(storage)?;
            }
        }
    }
    Ok(())
}

pub fn do_task(task_storage: &dyn TaskStorage, ulid_suffix: &str) -> Result<()> {
    let mut tasks = task_storage.search_using_ulid(ulid_suffix)?;
    if tasks.len() > 1 {
        bail!(
            "Expected 1 task but found {}\n{}",
            tasks.len(),
            tasks.iter().fold("".to_string(), |acc, x| format!(
                "{}\n{}: {}",
                acc, x.ulid, x.body
            ))
        );
    }
    if tasks.is_empty() {
        bail!("No tasks found with ulid: {}", ulid_suffix);
    }

    let task = &mut tasks[0];
    task.do_task(task_storage)?;
    let mut stdout = StandardStream::stdout(termcolor::ColorChoice::Always);
    write!(&mut stdout, "Done: {} ", task.ulid)?;
    stdout.set_color(ColorSpec::new().set_fg(Some(Color::Yellow)))?;
    writeln!(&mut stdout, "{}", task.body)?;
    stdout.reset()?;
    Ok(())
}

pub fn list_next_tasks(storage: &dyn TaskStorage, number: usize) -> Result<()> {
    let tasks = storage.next_tasks(number)?;
    display_utils::show_tasks_table(&tasks)?;
    Ok(())
}

fn minutes_per_task(time_start: &DateTime<Utc>, time_end: &DateTime<Utc>, no_tasks: usize) -> f32 {
    let duration = time_end.timestamp() - time_start.timestamp();
    (duration as f32) / (60.0 * (no_tasks as f32))
}

fn write_line(line: String, color: Color) {
    let mut stdout = StandardStream::stdout(ColorChoice::Always);
    stdout
        .set_color(ColorSpec::new().set_fg(Some(color)))
        .unwrap();
    writeln!(&mut stdout, "{}", line).unwrap();
    stdout.reset().unwrap();
}

pub fn get_summary_stats(storage: &dyn TaskStorage) -> Result<()> {
    let day_summary = storage.summarize_day()?;
    let total_due = day_summary.total_tasks;
    let done_tasks = day_summary.done_tasks;

    let ratio_done = (done_tasks as f32) / (total_due as f32);
    let now = Utc::now();
    let end_time = Utc
        .with_ymd_and_hms(now.year(), now.month(), now.day(), 17, 0, 0)
        .unwrap();
    let open_meeting_tasks = day_summary.remaining_meetings;
    let approx_meeting_minutes = Duration::minutes(open_meeting_tasks as i64 * 30);
    let start_date_with_minutes = now.add(approx_meeting_minutes);
    let actual_minutes = minutes_per_task(
        &start_date_with_minutes,
        &end_time,
        total_due - done_tasks - open_meeting_tasks,
    );

    println!("Total: {}", total_due);
    println!("NotDone: {}", (total_due - done_tasks));
    println!("Done: {}", done_tasks);
    println!("Meetings left (~30 mins): {}", open_meeting_tasks);
    println!("Ratio done: {:.2}", ratio_done);
    let mut color = Color::Green;
    if actual_minutes < 10.0 {
        color = Color::Red;
    }
    write_line(format!("Minutes per task: {:.2}", actual_minutes), color);
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::storage::sqlite_storage;

    use super::*;
    use chrono::{TimeZone, Utc};
    use rusqlite::Connection;

    fn get_connection() -> Connection {
        // FIXME! Refactor this to return SQLiteStorage
        let conn = Connection::open_in_memory().unwrap();
        let sqlite_storage = sqlite_storage::SQLiteStorage { connection: conn };
        sqlite_storage.create_tasks_table().unwrap();
        let insert_query = r#"INSERT INTO tasks (ulid, body, due_utc, closed_utc, modified_utc) VALUES
            ('8vag','follow up wit','2023-08-23 09:01:34',NULL,NULL),
            ('7nx0','deep dive int','2023-08-06 18:46:41',NULL,NULL),
            ('pvt4','create new ep','2023-08-07 11:23:38',NULL,NULL),
            ('d6bx','retrospect on','2023-08-06 18:49:06',NULL,'2023-08-05 01:00:00'),
            ('6715','rotate passwo','2023-08-06 18:47:09',NULL,NULL),
            ('mvtr','leetcode week','2023-08-07 04:34:03',NULL,NULL),
            ('3akq','cockroach cle','2023-08-06 18:47:09',NULL,NULL),
            ('sa6k','check up on J','2023-08-06 18:47:09',NULL,NULL),
            ('c6ez','plan conversa','2023-08-07 11:12:29',NULL,NULL),
            ('h2td','read/code on ','2023-08-07 18:50:05',NULL,NULL);
        "#;
        let tags_query = r#"INSERT INTO task_to_tag (ulid, task_ulid, tag) VALUES
            ('abcd', '8vag', 'work'),
            ('defg', '8vag', 'meeting');
        "#;
        sqlite_storage.connection.execute(insert_query, ()).unwrap();
        sqlite_storage.connection.execute(tags_query, ()).unwrap();
        sqlite_storage.connection
    }

    #[test]
    fn minutes_per_task_is_correct() {
        let start_date = Utc.with_ymd_and_hms(2023, 11, 4, 10, 0, 0).unwrap();
        let end_date = Utc.with_ymd_and_hms(2023, 11, 4, 12, 0, 0).unwrap();
        let result = minutes_per_task(&start_date, &end_date, 12);
        let expected = 10.00;
        assert_eq!(result, expected)
    }

    #[test]
    fn next_task_is_none() {
        let task = Task::default();
        let new_task = task.next_task();
        assert_eq!(new_task, None);
    }

    #[test]
    fn next_task_has_correct_date() {
        let mut task = Task::default();
        task.due_utc = Some("2023-12-04 10:00:00".to_string());
        task.ready_utc = Some("2023-12-04 09:00:00".to_string());
        task.recurrence_duration = Some("P1M".to_string());
        let expected_due_utc = Some("2024-01-04 10:00:00".to_string());
        let expected_ready_utc = Some("2024-01-04 09:00:00".to_string());
        let new_task = task.next_task().unwrap();
        assert_eq!(new_task.due_utc, expected_due_utc);
        assert_eq!(new_task.ready_utc, expected_ready_utc);
        assert_eq!(new_task.recurrence_duration, Some("P1M".to_string()));
    }

    #[test]
    fn next_task_has_correct_date_with_job_day() {
        let mut task = Task::default();
        task.due_utc = Some("2024-02-09 10:00:00".to_string());
        task.recurrence_duration = Some("P1JD".to_string());
        let expected_recurrence_duration = Some("2024-02-12 10:00:00".to_string());
        let new_task = task.next_task().unwrap();
        assert_eq!(new_task.due_utc, expected_recurrence_duration);
        assert_eq!(new_task.recurrence_duration, Some("P1JD".to_string()));

        task.due_utc = Some("2024-02-10 10:00:00".to_string());
        task.recurrence_duration = Some("P1JD".to_string());
        let new_task = task.next_task().unwrap();
        assert_eq!(new_task.due_utc, expected_recurrence_duration);
        assert_eq!(new_task.recurrence_duration, Some("P1JD".to_string()));
    }

    #[test]
    fn task_saved_to_db() {
        let task_storage = sqlite_storage::SQLiteStorage {
            connection: get_connection(),
        };
        let task = Task::default();
        task.save_to_db(&task_storage).unwrap();

        let saved_tasks = task_storage.search_using_ulid(&task.ulid).unwrap();
        assert_eq!(saved_tasks.len(), 1);
        let expected_task = &saved_tasks[0];
        let saved_task = Task {
            modified_utc: None,
            ..expected_task.clone()
        };
        assert_eq!(task, saved_task,);
    }

    #[test]
    fn task_saved_to_db_with_tags() {
        let conn = get_connection();
        let task = Task {
            tags: Some(vec!["meeting".to_string(), "work".to_string()]),
            ..Default::default()
        };

        let task_storage = sqlite_storage::SQLiteStorage { connection: conn };
        task.save_to_db(&task_storage).unwrap();
        let saved_tasks = task_storage.search_using_ulid(&task.ulid).unwrap();
        assert_eq!(saved_tasks.len(), 1);
        let expected_task = &saved_tasks[0];
        let saved_task = Task {
            modified_utc: None,
            ..expected_task.clone()
        };
        assert_eq!(task, saved_task,);
    }

    #[test]
    fn task_do_task() {
        let task_storage = sqlite_storage::SQLiteStorage {
            connection: get_connection(),
        };
        let mut tasks = task_storage.search_using_ulid("8vag").unwrap();
        let task = &mut tasks[0];
        assert_eq!(task.closed_utc, None);
        task.do_task(&task_storage).unwrap();
        let task = &task_storage.search_using_ulid("8vag").unwrap()[0];
        assert_ne!(task.closed_utc, None);
    }

    #[test]
    fn test_get_tasks_has_valid_tag() {
        let task_storage = sqlite_storage::SQLiteStorage {
            connection: get_connection(),
        };
        let sql_clause = "WHERE ulid = '8vag'";
        let tasks = task_storage.get_tasks(Some(sql_clause)).unwrap();
        assert_eq!(tasks.len(), 1);
        assert_eq!(
            tasks[0].tags,
            Some(vec!["meeting".to_string(), "work".to_string()])
        );
    }

    #[test]
    fn test_update_to_db() {
        let task_storage = sqlite_storage::SQLiteStorage {
            connection: get_connection(),
        };
        let conn = &task_storage.connection;
        let sql_clause = "WHERE ulid = '8vag'";
        let tasks = task_storage.get_tasks(Some(sql_clause)).unwrap();
        let mut task = tasks[0].to_owned();
        task.body = "random new new body".to_string();
        task.update_to_db(&task_storage).unwrap();
        let tasks = task_storage.get_tasks(Some(sql_clause)).unwrap();
        assert_eq!(tasks[0].body, "random new new body".to_string());
    }

    #[test]
    fn test_update_to_db_with_tags() {
        let task_storage = sqlite_storage::SQLiteStorage {
            connection: get_connection(),
        };
        let conn = &task_storage.connection;
        let sql_clause = "WHERE ulid = '8vag'";
        let tasks = task_storage.get_tasks(Some(sql_clause)).unwrap();
        let mut task = tasks[0].to_owned();
        let tags = Some(vec!["r1".to_string(), "r2".to_string()]);
        task.tags = tags.clone();
        task.update_to_db(&task_storage).unwrap();
        let result_tasks = task_storage.get_tasks(Some(sql_clause)).unwrap();
        assert_eq!(result_tasks[0].tags, tags);
    }
}
