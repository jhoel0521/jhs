//! Estado persistente (`%LOCALAPPDATA%\jh-script\state.json`) y log del agente.

use std::io::Write;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq)]
pub struct State {
    /// Momento del apagado programado (segundos UNIX), si hay uno.
    #[serde(default)]
    pub apagado_unix: Option<i64>,
}

pub fn data_dir() -> PathBuf {
    std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("jh-script")
}

pub fn log_path() -> PathBuf {
    data_dir().join("agent.log")
}

impl State {
    fn path() -> PathBuf {
        data_dir().join("state.json")
    }

    pub fn load() -> State {
        std::fs::read_to_string(Self::path())
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) {
        let _ = std::fs::create_dir_all(data_dir());
        if let Ok(t) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(Self::path(), t);
        }
    }
}

/// Añade una línea al log del agente (rota al pasar de 1 MB).
pub fn log(msg: &str) {
    let path = log_path();
    let _ = std::fs::create_dir_all(data_dir());
    if std::fs::metadata(&path).map(|m| m.len() > 1_048_576).unwrap_or(false) {
        let _ = std::fs::rename(&path, path.with_extension("log.old"));
    }
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
        let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S");
        let _ = writeln!(f, "{now}  {msg}");
    }
}
