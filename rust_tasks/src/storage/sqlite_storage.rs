use std::collections::HashMap;

use chrono::Local;
use rusqlite::{params, Connection};
use ulid::Ulid;

use crate::tasks::{summary::SummaryConfig, Task};

use super::storage::{DaySummaryResult, TaskStorage};

const CREATE_TASKS_TABLE_QUERY: &str = "CREATE TABLE IF NOT EXISTS tasks (
  ulid text not null primary key,
  body text not null,
  modified_utc text,
  ready_utc text,
  due_utc text,
  closed_utc text,
  recurrence_duration text,
  priority_adjustment float,
  user text,
  metadata text
);
";

const CREATE_TAGS_TABLE_QUERY: &str = "CREATE TABLE IF NOT EXISTS task_to_tag (
    ulid TEXT NOT NULL PRIMARY KEY,
    task_ulid TEXT NOT NULL,
    tag text NOT NULL,
    FOREIGN KEY(task_ulid) REFERENCES tasks(ulid),
    CONSTRAINT no_duplicate_tags UNIQUE(task_ulid, tag)
);
";

const CREATE_TASKS_VIEW: &str = "CREATE VIEW IF NOT EXISTS tasks_view AS
SELECT 
    tasks.*,
    tasks.priority_adjustment AS priority,
    group_concat(distinct task_to_tag.tag) AS tags
FROM tasks LEFT JOIN task_to_tag ON tasks.ulid = task_to_tag.task_ulid
GROUP BY tasks.ulid;
";

pub struct SQLiteStorage {
    pub connection: Connection,
}

impl TaskStorage for SQLiteStorage {
    fn save(&self, task: &Task) -> anyhow::Result<()> {
        let query = "INSERT INTO tasks (ulid, body, modified_utc, ready_utc, due_utc, closed_utc, recurrence_duration, priority_adjustment, user, metadata) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)";
        let mut stmt = self.connection.prepare(query)?;
        stmt.execute(params![
            task.ulid,
            task.body,
            get_utc_now_db_str(),
            task.ready_utc,
            task.due_utc,
            task.closed_utc,
            task.recurrence_duration,
            task.priority_adjustment,
            task.user,
            task.metadata,
        ])?;

        let tags_query = "INSERT INTO task_to_tag (ulid, task_ulid, tag) VALUES (?, ?, ?) ON CONFLICT DO NOTHING";
        let mut stmt = self.connection.prepare(tags_query)?;
        if let Some(tags) = &task.tags {
            tags.iter().for_each(|x| {
                stmt.execute(params![
                    Ulid::new().to_string().to_lowercase(),
                    task.ulid,
                    x
                ])
                .unwrap();
            });
        }

        Ok(())
    }

    fn delete(&self, task: &Task) -> anyhow::Result<()> {
        let query = "DELETE FROM tasks WHERE ulid = ?";
        let mut stmt = self.connection.prepare(query)?;
        stmt.execute(params![task.ulid])?;
        let drop_tags_query = "DELETE FROM task_to_tag WHERE task_ulid = ?";
        self.connection
            .prepare(drop_tags_query)?
            .execute(params![task.ulid])?;
        Ok(())
    }

    fn update(&self, task: &Task) -> anyhow::Result<()> {
        let query = r#"UPDATE tasks SET 
            body = ?, modified_utc = ?, ready_utc = ?, due_utc = ?, closed_utc = ?,
            recurrence_duration = ?, priority_adjustment = ?, user = ?, metadata =?
            WHERE ulid = ?;"#;
        let mut stmt = self.connection.prepare(query)?;
        stmt.execute(params![
            task.body,
            get_utc_now_db_str(),
            task.ready_utc,
            task.due_utc,
            task.closed_utc,
            task.recurrence_duration,
            task.priority_adjustment,
            task.user,
            task.metadata,
            task.ulid,
        ])?;
        let drop_tags_query = "DELETE FROM task_to_tag WHERE task_ulid = ?";
        self.connection
            .prepare(drop_tags_query)?
            .execute(params![task.ulid])?;
        let tags_query = "INSERT INTO task_to_tag (ulid, task_ulid, tag) VALUES (?, ?, ?) ON CONFLICT DO NOTHING";
        let mut stmt = self.connection.prepare(tags_query)?;
        if let Some(tags) = &task.tags {
            tags.iter().for_each(|x| {
                stmt.execute(params![
                    Ulid::new().to_string().to_lowercase(),
                    task.ulid,
                    x
                ])
                .unwrap();
            });
        }
        Ok(())
    }

    fn search_using_ulid(&self, ulid: &str) -> anyhow::Result<Vec<Task>> {
        let query = format!("SELECT ulid, body, modified_utc, ready_utc, due_utc, closed_utc, recurrence_duration, priority, user, metadata, tags FROM tasks_view WHERE ulid LIKE '%{}'", ulid);
        let mut stmt = self.connection.prepare(&query)?;

        let tasks: Vec<Task> = stmt
            .query_map([], |row| {
                Ok(Task {
                    ulid: row.get(0)?,
                    body: row.get(1)?,
                    modified_utc: row.get(2)?,
                    ready_utc: row.get(3)?,
                    due_utc: row.get(4)?,
                    closed_utc: row.get(5)?,
                    recurrence_duration: row.get(6)?,
                    // filler value to stop weird increments
                    priority_adjustment: None,
                    user: row.get(8)?,
                    metadata: row.get(9)?,
                    tags: {
                        let tags: Option<String> = row.get(10)?;
                        tags.map(|x| x.split(',').map(|x| x.to_string()).collect::<Vec<String>>())
                    },
                })
            })?
            .map(|x| x.unwrap())
            .collect();
        Ok(tasks)
    }

    fn next_tasks(&self, number: usize) -> anyhow::Result<Vec<Task>> {
        let extra_clause = format!(
            r#"WHERE
                    DATE(due_utc) <= DATE('now') AND
                    closed_utc IS NULL AND
                    (ready_utc IS NULL OR DATETIME('now') >= DATETIME(ready_utc))
                ORDER BY due_utc ASC, priority DESC LIMIT {}"#,
            number
        );
        self.get_tasks(Some(&extra_clause))
    }

    fn summarize_day(&self, summary: &SummaryConfig) -> anyhow::Result<DaySummaryResult> {
        let total_tasks = self.count_tasks("(DATE(due_utc) <= DATE('now') AND DATE(closed_utc) IS NULL) OR DATE(closed_utc) = DATE('now')");
        let done_tasks = self.count_tasks("DATE(closed_utc) = DATE('now')");
        let mut open_tags_count = HashMap::new();
        for tag in summary.relevant_tags() {
            let count_query = format!(
                "DATE(due_utc) = DATE('now') AND closed_utc IS NUll AND tags LIKE '%{}%'",
                tag
            );
            let local_count = self.count_tasks(&count_query);
            open_tags_count.insert(tag, local_count);
        }
        Ok(DaySummaryResult {
            total_tasks,
            done_tasks,
            open_tags_count: Some(open_tags_count),
        })
    }

    fn unsafe_query(&self, clause: &str) -> anyhow::Result<Vec<Task>> {
        self.get_tasks(Some(clause))
    }
}

impl SQLiteStorage {
    pub fn new(db_path: &str) -> Self {
        let sql_storage = SQLiteStorage {
            connection: Connection::open(db_path).unwrap(),
        };
        sql_storage.create_tasks_table().unwrap();
        sql_storage
    }

    pub fn create_tasks_table(&self) -> anyhow::Result<()> {
        self.connection.execute(CREATE_TASKS_TABLE_QUERY, ())?;
        self.connection.execute(CREATE_TAGS_TABLE_QUERY, ())?;
        self.connection.execute(CREATE_TASKS_VIEW, ())?;
        Ok(())
    }

    pub fn get_tasks(&self, extra_sql_clause: Option<&str>) -> anyhow::Result<Vec<Task>> {
        let mut query = "SELECT ulid, body, modified_utc, ready_utc, due_utc, closed_utc, recurrence_duration, priority, user, metadata, tags FROM tasks_view".to_string();
        if let Some(x) = extra_sql_clause {
            query = format!("{query} {x}")
        }
        let mut stmt = self.connection.prepare(&query)?;
        let tasks: Vec<Task> = stmt
            .query_map([], |row| {
                Ok(Task {
                    ulid: row.get(0)?,
                    body: row.get(1)?,
                    modified_utc: row.get(2)?,
                    ready_utc: row.get(3)?,
                    due_utc: row.get(4)?,
                    closed_utc: row.get(5)?,
                    recurrence_duration: row.get(6)?,
                    // filler value to stop weird increments
                    priority_adjustment: None,
                    user: row.get(8)?,
                    metadata: row.get(9)?,
                    tags: {
                        let tags: Option<String> = row.get(10)?;
                        tags.map(|x| x.split(',').map(|x| x.to_string()).collect::<Vec<String>>())
                    },
                })
            })?
            .map(|x| x.unwrap())
            .collect();
        Ok(tasks)
    }

    fn count_tasks(&self, where_clause: &str) -> usize {
        // TODO: fix tasks to use tasks_view since it has tags or rather use a join instead if possible
        let query = format!("SELECT count(*) FROM tasks_view where {where_clause}");
        let count: usize = self
            .connection
            .query_row(query.as_str(), [], |row| row.get(0))
            .expect("Failed to run query");
        count
    }
}

fn get_utc_now_db_str() -> String {
    Local::now()
        .naive_utc()
        .and_utc()
        .format("%Y-%m-%d %H:%M:%S")
        .to_string()
}

#[cfg(test)]
mod tests {

    use super::*;

    fn get_sqlite_storage() -> SQLiteStorage {
        let sqlite_storage = SQLiteStorage::new(":memory:");
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
        sqlite_storage
    }

    #[test]
    fn tasks_table_exists() {
        let count: u8 = get_sqlite_storage()
            .connection
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE type='table' AND name='tasks'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn count_is_valid() {
        let count = get_sqlite_storage().count_tasks("modified_utc IS NOT NULL");
        assert_eq!(count, 1);
    }

    #[test]
    fn task_saved() {
        let sqlite_storage = get_sqlite_storage();
        let task = Task::default();
        sqlite_storage.save(&task).unwrap();
        let saved_tasks = sqlite_storage.search_using_ulid(&task.ulid).unwrap();
        assert_eq!(saved_tasks.len(), 1);
        let expected_task = &saved_tasks[0];
        let saved_task = Task {
            modified_utc: None,
            ..expected_task.clone()
        };
        assert_eq!(task, saved_task,);
    }

    #[test]
    fn test_get_tasks() {
        let sqlite_storage = get_sqlite_storage();
        let sql_clause = "WHERE ulid = '8vag'";
        let tasks = sqlite_storage.get_tasks(Some(sql_clause)).unwrap();
        assert_eq!(tasks.len(), 1);
    }

    #[test]
    fn task_deleted() {
        let sqlite_storage = get_sqlite_storage();
        let tasks = sqlite_storage.search_using_ulid("6715").unwrap();
        let expected_task = &tasks[0];
        sqlite_storage.delete(expected_task).unwrap();
        let tasks = sqlite_storage.search_using_ulid("6715").unwrap();
        assert_eq!(tasks.len(), 0);
    }

    #[test]
    fn task_updated() {
        let sqlite_storage = get_sqlite_storage();
        let mut tasks = sqlite_storage.search_using_ulid("6715").unwrap();
        let expected_task = &mut tasks[0];
        expected_task.body = "updated task".to_string();
        sqlite_storage.update(expected_task).unwrap();
        let tasks = sqlite_storage.search_using_ulid("6715").unwrap();
        assert_eq!(tasks[0].body, "updated task".to_string());
    }
}
