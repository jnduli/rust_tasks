use anyhow::Result;
use chrono::Local;
use std::io::{ErrorKind, Write};
use termcolor::{Color, ColorSpec, StandardStream, WriteColor};

use super::Task;

pub fn show_tasks_table(tasks: &[Task]) -> Result<()> {
    let mut stdout = StandardStream::stdout(termcolor::ColorChoice::Always);
    let ulid_length = ulid_output_length(tasks.len());
    stdout.set_color(ColorSpec::new().set_underline(true))?;
    writeln!(&mut stdout, "{:7}{:23}body", "id", "due_utc")?;
    stdout.reset()?;

    tasks.iter().for_each(
        |x| match show_task_table(x, &mut stdout, Some(ulid_length)) {
            Ok(()) => (),
            Err(e) => match e.downcast_ref::<std::io::Error>() {
                Some(x) => match x.kind() {
                    ErrorKind::BrokenPipe => (),
                    _ => panic!("{:#?}", x),
                },
                _ => panic!("{:#?}", e),
            },
        },
    );
    Ok(())
}

fn show_task_table(
    task: &Task,
    stdout: &mut StandardStream,
    ulid_len: Option<usize>,
) -> Result<()> {
    let ulid = ulid_len.map_or_else(|| &task.ulid[..], |x| &task.ulid[task.ulid.len() - x..]);
    let due_utc = task.due_utc.map_or("".to_string(), |x| {
        x.format("%Y-%m-%d %H:%M:%S").to_string()
    });
    stdout.set_color(ColorSpec::new().set_fg(Some(Color::Green)))?;
    write!(stdout, "{:7}", ulid)?;
    stdout.set_color(ColorSpec::new().set_fg(Some(Color::Yellow)))?;
    write!(stdout, "{:23}", due_utc)?;
    let mut body_color = match task.due_utc.as_ref() {
        None => Color::White,
        Some(&due_chrono) => {
            let now_utc = Local::now().naive_utc().and_utc();
            let difference = (due_chrono - now_utc).num_minutes();
            if difference < 0 {
                Color::Red
            } else if difference < 120 {
                Color::White
            } else {
                Color::Rgb(105, 105, 105)
            }
        }
    };
    if task.closed_utc.is_some() {
        body_color = Color::Green;
    }

    stdout.set_color(ColorSpec::new().set_fg(Some(body_color)))?;
    write!(stdout, "{}", task.body)?;
    stdout.set_color(ColorSpec::new().set_fg(Some(Color::Blue)))?;
    let tags_str = task.tags.clone().map_or("".to_string(), |x| x.join(","));
    writeln!(stdout, " {}", tags_str)?;
    stdout.reset()?;
    Ok(())
}

fn ulid_output_length(total_tasks: usize) -> usize {
    // copied from tasklite implementation https://github.com/jnduli/TaskLite/blob/e36e1cb7998ff35185d86b7b3c988cb062622db5/tasklite-core/source/Lib.hs#L2227
    let base_32_expected_characters = 32.0;
    let minimum_length = 4;
    let total_collision_prob = 0.00001;
    let length = (total_tasks as f32 / total_collision_prob)
        .log(base_32_expected_characters)
        .ceil() as usize;
    length.max(minimum_length)
}

#[cfg(test)]
mod tests {
    use crate::tasks::display_utils::ulid_output_length;

    #[test]
    fn ulid_output_length_is_correct() {
        assert_eq!(ulid_output_length(10), 4);
        assert_eq!(ulid_output_length(20), 5);
        assert_eq!(ulid_output_length(100), 5);
    }
}
