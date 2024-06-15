use std::{
    fs::File,
    io::Read,
    path::{Path, PathBuf},
};

use rust_tasks::storage::sqlite_storage::SQLiteStorage;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub struct Config {
    pub db_uri: String,
    pub bind_address: String,
}

impl Config {
    pub fn load(path: Option<&str>) -> anyhow::Result<Self> {
        let config_file = path.map_or(default_config_path()?, |x| Path::new(&x).to_path_buf());
        let mut content = String::new();
        File::open(config_file)?.read_to_string(&mut content)?;
        let config: Config = toml::from_str(&content)?;
        Ok(config)
    }

    pub fn db_connection(&self) -> anyhow::Result<SQLiteStorage> {
        let path = &self.db_uri;
        if !path.starts_with("file://") {
            anyhow::bail!("{} doesnt start with file://", self.db_uri)
        }
        let file_path = self.db_uri.strip_prefix("file://").unwrap();
        Ok(SQLiteStorage::new(file_path))
    }
}

fn default_config_path() -> anyhow::Result<PathBuf> {
    let xdg_dirs = xdg::BaseDirectories::with_prefix("rust_tasks")?;
    match xdg_dirs.find_config_file("tasks_server.toml") {
        None => anyhow::bail!(
            "Couldn't find tasks_server.toml in {:?}",
            xdg_dirs.get_config_home()
        ),
        Some(x) => Ok(x),
    }
}
