//! Carpetas de configuración y datos según la convención de cada SO.
//!
//! - Windows: `%APPDATA%\jhs` (config) y `%LOCALAPPDATA%\jhs` (datos).
//! - Linux: XDG, `~/.config/jhs` y `~/.local/state/jhs`.

use std::path::PathBuf;

fn env_dir(var: &str) -> Option<PathBuf> {
    std::env::var_os(var).filter(|v| !v.is_empty()).map(PathBuf::from)
}

#[cfg(unix)]
fn home() -> PathBuf {
    env_dir("HOME").unwrap_or_else(std::env::temp_dir)
}

/// Carpeta de `config.toml`.
pub fn config_dir() -> PathBuf {
    #[cfg(windows)]
    let base = env_dir("APPDATA").unwrap_or_else(std::env::temp_dir);
    #[cfg(unix)]
    let base = env_dir("XDG_CONFIG_HOME").unwrap_or_else(|| home().join(".config"));
    base.join("jhs")
}

/// Carpeta del estado persistente y del log del agente.
pub fn data_dir() -> PathBuf {
    #[cfg(windows)]
    let base = env_dir("LOCALAPPDATA").unwrap_or_else(std::env::temp_dir);
    #[cfg(unix)]
    let base = env_dir("XDG_STATE_HOME").unwrap_or_else(|| home().join(".local").join("state"));
    base.join("jhs")
}

/// Carpeta temporal privada del usuario (sockets, marcas de sesión).
#[cfg(unix)]
pub fn runtime_dir() -> PathBuf {
    env_dir("XDG_RUNTIME_DIR").unwrap_or_else(data_dir)
}

/// Nombre de la versión anterior del programa (antes de llamarse `jhs`).
pub const LEGACY_NAME: &str = "jh-script";

/// Copia `config.toml` y `state.json` de las carpetas de `jh-script` si las
/// nuevas aún no los tienen. Silencioso e idempotente.
pub fn migrate_legacy() {
    for (dir, file) in [(config_dir(), "config.toml"), (data_dir(), "state.json")] {
        let new = dir.join(file);
        let old = dir.with_file_name(LEGACY_NAME).join(file);
        if !new.exists() && old.is_file() {
            let _ = std::fs::create_dir_all(&dir);
            let _ = std::fs::copy(&old, &new);
        }
    }
}
