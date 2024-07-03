use std::{
    fs::File,
    io::Read,
    path::{Path, PathBuf},
};

use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};

use crate::{
    storage::{api_storage::APIStorage, sqlite_storage::SQLiteStorage, storage::TaskStorage},
    tasks::summary::SummaryConfig,
};

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    backend: Backend,
    summary: Option<SummaryConfig>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
enum BackendStrains {
    Api,
    SQLite,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct Backend {
    strain: BackendStrains,
    uri: String,
}

impl Config {
    pub fn load(path: Option<String>) -> Result<Config> {
        let config_file = path.map_or(config_path()?, |x| Path::new(&x).to_path_buf());
        let mut content = String::new();
        File::open(config_file)?.read_to_string(&mut content)?;
        let config: Config = toml::from_str(&content)?;
        Ok(config)
    }

    pub fn get_storage_engine(&self) -> Result<Box<dyn TaskStorage>> {
        let backend = &self.backend;
        match backend.strain {
            BackendStrains::Api => Ok(Box::new(APIStorage::new(backend.uri.clone()))),
            BackendStrains::SQLite => {
                let path = backend.uri.clone();
                if !path.starts_with("file://") {
                    bail!("Expected path to start with file:// but found {path}")
                }
                let stripper = "file://".len();
                let absolute_path = &path[stripper..];
                Ok(Box::new(SQLiteStorage::new(absolute_path)))
            }
        }
    }

    pub fn get_summary_config(&self) -> SummaryConfig {
        self.summary.clone().unwrap_or_default()
    }
}

fn config_path() -> Result<PathBuf> {
    let xdg_dirs = xdg::BaseDirectories::with_prefix("rust_tasks")?;
    match xdg_dirs.find_config_file("config.toml") {
        None => bail!(
            "Couldn't find config.toml in {:?}",
            xdg_dirs.get_config_home()
        ),
        Some(x) => Ok(x),
    }
}
