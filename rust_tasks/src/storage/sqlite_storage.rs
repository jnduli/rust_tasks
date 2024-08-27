use std::collections::{HashMap, HashSet};

use anyhow::bail;
use chrono::{naive::NaiveDate, Duration, Local, Utc};
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

const CREATE_DELETED_TASKS_QUERY: &str = "CREATE TABLE IF NOT EXISTS deleted_tasks (
  task_ulid text not null primary key,
  modified_utc text
);";

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
            task.recurrence_duration.map(|x| x.to_string()),
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
        let cnts = self.count_tasks(&format!("ulid = '{}'", task.ulid));
        if cnts == 0 {
            bail!("Task with ulid: {} doesn't exist", task.ulid);
        }
        let query = "DELETE FROM tasks WHERE ulid = ?";
        let mut stmt = self.connection.prepare(query)?;
        stmt.execute(params![task.ulid])?;
        let mut delete_insert_stmt = self
            .connection
            .prepare("INSERT INTO deleted_tasks (task_ulid, modified_utc) VALUES (?, ?)")?;
        delete_insert_stmt.execute(params![task.ulid, get_utc_now_db_str()])?;
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
            task.recurrence_duration.map(|x| x.to_string()),
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
        let extra_sql_clause = format!("WHERE ulid LIKE '%{}'", ulid);
        self.get_tasks(Some(&extra_sql_clause))
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
        let total_tasks = self.count_tasks(
            r#"
                (DATE(due_utc) <= DATE('now') AND DATE(closed_utc) IS NULL) OR 
                DATE(closed_utc) = DATE('now')
            "#,
        );
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

    fn deleted_ulids(&self, n_days: &usize) -> anyhow::Result<HashSet<String>> {
        let check_date = Utc::now().date_naive() - Duration::days(*n_days as i64);
        let query = format!(
            "SELECT task_ulid FROM deleted_tasks WHERE modified_utc > '{}' OR modified_utc IS NULL",
            check_date
        );
        let mut stmt = self.connection.prepare(&query)?;
        let ulids: HashSet<String> = stmt
            .query_map([], |row| Ok(row.get(0)?))?
            .map(|x| x.unwrap())
            .collect();
        Ok(ulids)
    }

    fn sync(&self, task_storage: &dyn TaskStorage, n_days: usize) -> anyhow::Result<()> {
        let date = Utc::now().date_naive() - Duration::days(n_days as i64);
        self.sync_deleted(task_storage, &n_days)?;
        let updated_clause = format!("WHERE modified_utc > '{}' OR modified_utc IS NULL", date);
        let self_tasks = self.unsafe_query(&updated_clause)?;
        let other_tasks = task_storage.unsafe_query(&updated_clause)?;
        let self_map = create_tasks_hashmap(self_tasks);
        let other_map = create_tasks_hashmap(other_tasks);
        let mut upstream_added = 0;
        let mut local_updated = 0;
        let mut upstream_updated = 0;
        for k in self_map.keys() {
            let self_task = self_map.get(k).unwrap(); // I'm sure this exists
            let other_task = other_map.get(k);
            match other_task {
                None => {
                    upstream_added += 1;
                    if let Err(_e) = task_storage.save(self_task) {
                        upstream_added -= 1;
                        upstream_updated += 1;
                        task_storage.update(self_task)?
                    }
                }
                Some(other) => {
                    if other != self_task {
                        // FIXME! custom code to ensure all other fields are the same excluding the
                        // modfied utc
                        let other_clean = Task {
                            modified_utc: None,
                            ..other.clone()
                        };
                        let self_clean = Task {
                            modified_utc: None,
                            ..self_task.clone()
                        };
                        if self_clean != other_clean {
                            if other.modified_utc > self_task.modified_utc {
                                self.update(other)?;
                                local_updated += 1;
                            } else {
                                task_storage.update(self_task)?;
                                upstream_updated += 1;
                            }
                        }
                    }
                }
            }
        }

        let mut local_added = 0;
        for k in other_map.keys() {
            if !self_map.contains_key(k) {
                local_added += 1;
                let other_task = other_map.get(k).unwrap();
                if let Err(_e) = self.save(other_task) {
                    local_added -= 1;
                    local_updated += 1;
                    self.update(other_task)?
                };
            }
        }
        println!(
            "Successful sync: \n added {} and updated {} tasks to self\n added {} and updated {} tasks",
            local_added, local_updated,
            upstream_added, upstream_updated
        );
        Ok(())
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
        self.connection.execute(CREATE_DELETED_TASKS_QUERY, ())?;
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
                    recurrence_duration: {
                        let recur: Option<String> = row.get(6)?;
                        recur.map(|x| x.parse().unwrap())
                    },
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

    fn sync_deleted(&self, task_storage: &dyn TaskStorage, n_days: &usize) -> anyhow::Result<()> {
        let self_deleted = self.deleted_ulids(n_days)?;
        let other_deleted = task_storage.deleted_ulids(n_days)?;

        for self_task_ulid in &self_deleted {
            if other_deleted.contains(self_task_ulid) {
                continue;
            }
            let other_exists = task_storage.search_using_ulid(&self_task_ulid)?;
            if other_exists.len() > 0 {
                task_storage.delete(&other_exists[0])?;
            }
        }

        for other_task_ulid in &other_deleted {
            if self_deleted.contains(other_task_ulid) {
                continue;
            }
            let self_exists = self.search_using_ulid(&other_task_ulid)?;
            if self_exists.len() > 0 {
                self.delete(&self_exists[0])?;
            }
        }
        Ok(())
    }
}

fn get_utc_now_db_str() -> String {
    Local::now()
        .naive_utc()
        .and_utc()
        .format("%Y-%m-%d %H:%M:%S")
        .to_string()
}

fn create_tasks_hashmap(tasks: Vec<Task>) -> HashMap<String, Task> {
    let mut map = HashMap::new();
    tasks.iter().for_each(|x| {
        map.insert(x.ulid.to_string(), x.clone());
    });
    map
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
        let count_query = r#"
            (DATE(due_utc) <= DATE('now') AND DATE(closed_utc) IS NULL)
            OR DATE(closed_utc) = DATE('now')
        "#;
        let open_tasks = sqlite_storage.count_tasks(count_query);
        let tasks = sqlite_storage.search_using_ulid("6715").unwrap();
        let expected_task = &tasks[0];
        sqlite_storage.delete(expected_task).unwrap();
        let tasks = sqlite_storage.search_using_ulid("6715").unwrap();
        assert_eq!(tasks.len(), 0);
        let new_open_tasks = sqlite_storage.count_tasks(count_query);
        assert_eq!(open_tasks, new_open_tasks + 1);
        let deleted_tasks = sqlite_storage.deleted_ulids(&1).unwrap();
        assert_eq!(deleted_tasks.len(), 1);
        assert!(deleted_tasks.contains("6715"));
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

    #[test]
    fn test_sync() {
        let storage1 = get_sqlite_storage();
        let storage2 = get_sqlite_storage();
        let count_query = r#"
            (DATE(due_utc) <= DATE('now') AND DATE(closed_utc) IS NULL)
            OR DATE(closed_utc) = DATE('now')
        "#;
        let original_tasks_count = storage1.count_tasks(count_query);
        let mut tasks = storage1.search_using_ulid("6715").unwrap();
        let expected_task = &mut tasks[0];
        expected_task.body = "random updated task".to_string();
        storage1.update(expected_task).unwrap();
        let new_task1 = Task::default();
        storage1.save(&new_task1).unwrap();
        let task = storage1.search_using_ulid("3akq").unwrap();
        storage1.delete(&task[0]).unwrap();

        let new_task2 = Task::default();
        storage2.save(&new_task2).unwrap();
        let mut tasks = storage2.search_using_ulid("h2td").unwrap();
        let task2 = &mut tasks[0];
        task2.body = "random mess".to_string();
        storage2.update(task2).unwrap();

        storage1.sync(&storage2, 2).unwrap();
        let tasks = storage2.search_using_ulid("6715").unwrap();
        assert_eq!(tasks[0].body, "random updated task".to_string());
        let tasks2 = storage1.search_using_ulid("h2td").unwrap();

        assert_eq!(tasks2[0].body, "random mess");

        assert_eq!(
            storage2.search_using_ulid(&new_task1.ulid).unwrap().len(),
            1
        );

        assert_eq!(
            storage1.search_using_ulid(&new_task2.ulid).unwrap().len(),
            1
        );

        // deleted task doesn't exist anymore
        let task = storage1.search_using_ulid("3akq").unwrap();
        assert_eq!(task.len(), 0);
        let task = storage2.search_using_ulid("3akq").unwrap();
        assert_eq!(task.len(), 0);
        assert_eq!(storage1.count_tasks(count_query), original_tasks_count - 1);
        assert_eq!(storage2.count_tasks(count_query), original_tasks_count - 1);
    }
}
