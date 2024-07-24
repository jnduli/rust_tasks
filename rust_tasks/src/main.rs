use std::error::Error;

use clap::Parser;
use clap::Subcommand;
use rust_tasks::config::Config;

#[derive(Parser, Debug)]
#[command(version, about, verbatim_doc_comment)]
/// Rust Tasks
/// Create a configuration file in $HOME/.config/rust_tasks/config.toml
///
/// [backend]
/// strain = "api" or "db"
/// uri = "http://username:passwd@localhost:8080" or "sqlite://abc.ljasdfkj"
struct Args {
    #[arg(short, long, value_name = "FILE")]
    config: Option<String>,
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Debug, Subcommand)]
enum Commands {
    /// List open tasks due today
    Leo {
        #[arg(default_value_t = 7)]
        number: usize,
    },
    /// Mark task(s) as done
    Do {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        task_ulids: Vec<String>,
    },
    /// edit task details in $EDITOR using yaml
    Edit { task_ulid: String },
    /// Remove task
    Delete {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        task_ulids: Vec<String>,
    },
    /// Create a new task
    Add {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        task_params: Vec<String>,
    },
    /// Transfer tasks to today or next recurring period after today
    QuickClean { date: String },
    /// Direct query into the DB
    Query { clause: String },
    /// Statistics about how my day is going
    Summary {},
    /// Sync with other storages
    Sync {
        #[arg(default_value_t = 3)]
        n_days: usize,
    },
    /// Running tests I'm trying out
    Experiment {},
}

fn main() -> anyhow::Result<(), Box<dyn Error>> {
    color_eyre::install()?;
    let args = Args::parse();
    let task_config = Config::load(args.config)?;
    let task_storage_box = task_config.get_storage_engine()?;
    // FIXME! add support for summary to taskstorage

    // You can check for the existence of subcommands, and if found use their
    // matches just as you would the top level cmd
    match &args.command {
        Some(Commands::Leo { number }) => {
            rust_tasks::tasks::list_next_tasks(task_storage_box.as_ref(), *number)?;
        }
        Some(Commands::Do { task_ulids }) => {
            for task_ulid in task_ulids {
                rust_tasks::tasks::do_task(task_storage_box.as_ref(), task_ulid)?
            }
        }
        Some(Commands::Edit { task_ulid }) => {
            rust_tasks::tasks::edit_utils::edit_task(task_storage_box.as_ref(), task_ulid)?
        }

        Some(Commands::Delete { task_ulids }) => task_ulids.iter().for_each(|task_ulid| {
            rust_tasks::tasks::edit_utils::delete_task(task_storage_box.as_ref(), task_ulid)
                .unwrap()
        }),
        Some(Commands::Summary {}) => rust_tasks::tasks::get_summary_stats(
            task_storage_box.as_ref(),
            &task_config.get_summary_config(),
        )?,
        Some(Commands::Add { task_params }) => {
            let task_params_string = task_params.join(" ");
            rust_tasks::tasks::add_utils::add_task(task_storage_box.as_ref(), &task_params_string)?
        }
        Some(Commands::Query { clause }) => {
            rust_tasks::tasks::query(task_storage_box.as_ref(), clause)?
        }
        Some(Commands::QuickClean { date }) => {
            rust_tasks::tasks::quick_clean(task_storage_box.as_ref(), date)?
        }
        Some(Commands::Sync { n_days }) => {
            let syncs = &task_config.get_sync_engine()?;
            if syncs.len() > 1 {
                println!("I don't currently support multiple syncs");
            }
            task_storage_box.sync(syncs[0].as_ref(), *n_days)?;
        }
        Some(Commands::Experiment {}) => rust_tasks::tasks::experiment()?,
        None => {}
    }

    Ok(())
}
