use std::io::Write;

use crate::storage::storage::TaskStorage;

use anyhow::Result;
use termcolor::{Color, ColorSpec, StandardStream, WriteColor};

pub fn edit_task(storage: &dyn TaskStorage, ulid_suffix: &str) -> Result<()> {
    let mut tasks = storage.search_using_ulid(ulid_suffix)?;
    if tasks.len() > 1 {
        panic!(
            "Expected 1 task but found {}\n{}",
            tasks.len(),
            tasks.iter().fold("".to_string(), |acc, x| format!(
                "{}\n{}: {}",
                acc, x.ulid, x.body
            ))
        );
    } else if tasks.is_empty() {
        panic!("No tasks found with ulid: {}", ulid_suffix);
    }

    let task = &mut tasks[0];
    task.edit_with_editor()?;
    storage.update(task)?;

    let mut stdout = StandardStream::stdout(termcolor::ColorChoice::Always);
    write!(&mut stdout, "Done: {} ", task.ulid)?;
    stdout.set_color(ColorSpec::new().set_fg(Some(Color::Yellow)))?;
    writeln!(&mut stdout, "{}", task.body)?;
    stdout.reset()?;
    Ok(())
}

pub fn delete_task(storage: &dyn TaskStorage, ulid_suffix: &str) -> Result<()> {
    let mut tasks = storage.search_using_ulid(ulid_suffix)?;
    if tasks.len() > 1 {
        panic!(
            "Expected 1 task but found {}\n{}",
            tasks.len(),
            tasks.iter().fold("".to_string(), |acc, x| format!(
                "{}\n{}: {}",
                acc, x.ulid, x.body
            ))
        );
    }
    if tasks.is_empty() {
        panic!("No tasks found with ulid: {}", ulid_suffix);
    }

    let task = &mut tasks[0];
    storage.delete(task)?;
    println!("Deleted: '{}' {}", task.body, task.ulid);
    Ok(())
}
