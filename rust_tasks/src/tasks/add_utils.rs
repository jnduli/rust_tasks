use anyhow::Result;
use chrono::{DateTime, NaiveDateTime, Utc};
use iso8601_duration::Duration;

use super::{Task, TaskStorage};

#[derive(Debug, PartialEq)]
struct AddContext {
    body: String,
    due: Option<DateTime<Utc>>,
    tags: Option<Vec<String>>,
    recur: Option<Duration>,
    priority: Option<f64>,
}

pub fn add_task(task_storage: &dyn TaskStorage, input: &str) -> Result<()> {
    let context = get_context(input.to_string())?;
    let task = Task {
        body: context.body,
        due_utc: context.due,
        tags: context.tags,
        recurrence_duration: context.recur,
        priority_adjustment: context.priority,
        ..Default::default()
    };

    task.save_to_db(task_storage)?;
    println!("Saved task: {}", task.ulid);
    Ok(())
}

fn get_context(input: String) -> Result<AddContext> {
    let mut special_identifiers = true;
    let mut body = String::new();
    let mut due = None;
    let mut tags: Vec<String> = vec![];
    let mut recur = None;
    let mut priority = None;
    for word in input.split(' ').rev() {
        if !special_identifiers {
            body = format!("{word} {body}");
            continue;
        }
        if word.starts_with("due:") {
            if due.is_some() {
                panic!("Invalid input string has multiple due dates");
            }
            let due_string = word.replace("due:", "");
            let due_chrono = NaiveDateTime::parse_from_str(&due_string, "%Y-%m-%dT%H:%M")
                .expect("Due period should have the format %Y-%m-%dT%H:%M e.g. 2023-10-09T10:05")
                .and_utc();
            due = Some(due_chrono);
        } else if word.starts_with("recur:") {
            if recur.is_some() {
                panic!("Invalid input string has multiple recurs");
            }
            let recur_string = word.replace("recur:", "");
            recur = Some(
                recur_string
                    .parse()
                    .expect("Recur string should be a valid iso8601 string"),
            );
        } else if word.starts_with("tag:") || word.starts_with('+') {
            let tag = word.replace("tag:", "").replace('+', "");
            tags.push(tag);
        } else if word.starts_with("p:") {
            priority = Some(word.replace("p:", "").parse::<f64>()?);
        } else {
            special_identifiers = false;
            body = format!("{word} {body}");
        }
    }

    if recur.is_some() && due.is_none() {
        panic!("The due date has to exist in a task recurs i.e. add due:XXXX to the command");
    }

    Ok(AddContext {
        body: body.trim().to_string(),
        due,
        recur,
        tags: (!tags.is_empty()).then_some(tags),
        priority,
    })
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::*;

    #[test]
    fn test_get_context_with_body_alone() {
        let input = "task 1".to_string();
        assert_eq!(
            get_context(input).unwrap(),
            AddContext {
                body: "task 1".to_string(),
                due: None,
                recur: None,
                tags: None,
                priority: None,
            }
        );
    }

    #[test]
    fn test_get_context_with_body_and_due_date() {
        let input2 = "task 1 due:2023-10-11T12:00".to_string();
        assert_eq!(
            get_context(input2).unwrap(),
            AddContext {
                body: "task 1".to_string(),
                due: Some(Utc.with_ymd_and_hms(2023, 10, 11, 12, 0, 0).unwrap()),
                recur: None,
                tags: None,
                priority: None,
            }
        );
    }

    #[test]
    fn test_get_context_with_everything() {
        let input2 = "task 1 due:2023-10-11T12:00 recur:P1W p:10 tag:work tag:meeting".to_string();
        assert_eq!(
            get_context(input2).unwrap(),
            AddContext {
                body: "task 1".to_string(),
                due: Some(Utc.with_ymd_and_hms(2023, 10, 11, 12, 0, 0).unwrap()),
                recur: Some("P1W".parse().unwrap()),
                tags: Some(vec!["meeting".to_string(), "work".to_string()]),
                priority: Some(10.0),
            }
        );

        let input3 = "task 1 p:3 due:2023-10-11T12:00 recur:P1W +work +meeting".to_string();
        assert_eq!(
            get_context(input3).unwrap(),
            AddContext {
                body: "task 1".to_string(),
                due: Some(Utc.with_ymd_and_hms(2023, 10, 11, 12, 0, 0).unwrap()),
                recur: Some("P1W".parse().unwrap()),
                tags: Some(vec!["meeting".to_string(), "work".to_string()]),
                priority: Some(3.0),
            }
        );
    }

    #[test]
    #[should_panic]
    fn test_get_context_fails_with_invalid_due_date() {
        let _ = get_context("task 1 due:2023-10-32T12:00".to_string());
    }

    #[test]
    #[should_panic]
    fn test_get_context_fails_with_invalid_recur_period() {
        let _ = get_context("task 1 due:2023-10-20T10:00 recur:P12abcd".to_string());
    }

    #[test]
    #[should_panic]
    fn test_get_context_fails_when_recur_exists_without_due_date() {
        let _ = get_context("task 1 recur:P1D".to_string());
    }
}
