//! Extensión de GNOME Shell para salir del modo privacidad con el combo secreto
//! (`combo_desbloqueo`) desde la pantalla de bloqueo.
//!
//! GNOME desactiva los atajos normales con la sesión bloqueada; la extensión
//! (en `assets/gnome-extension`) escucha las teclas en la pantalla de bloqueo y
//! envía `privacy_combo` al agente, que solo desbloquea si el bloqueo es suyo.
//! El combo se le pasa en `~/.config/jhs/unlock-combo.json` (lo vigila, así que
//! un cambio en `config.toml` + `jhs agent reload` se aplica al momento).

use std::path::PathBuf;

use serde_json::json;

use crate::adapters::linux::sys;
use crate::domain::combo::Combo;
use crate::domain::config::config_dir;

pub const UUID: &str = "jhs-unlock@jhoel0521.github.io";
const METADATA: &str = include_str!("../../../assets/gnome-extension/metadata.json");
const EXTENSION_JS: &str = include_str!("../../../assets/gnome-extension/extension.js");

pub fn extension_dir() -> PathBuf {
    std::env::var_os("XDG_DATA_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| sys::home().join(".local").join("share"))
        .join("gnome-shell")
        .join("extensions")
        .join(UUID)
}

pub fn combo_file() -> PathBuf {
    config_dir().join("unlock-combo.json")
}

/// Keysym X11 de la tecla principal del combo (lo que entrega Clutter).
pub fn keyval(c: &Combo) -> u32 {
    let vk = c.vk as u32;
    match vk {
        0x41..=0x5A => vk + 0x20, // a-z
        0x30..=0x39 => vk,        // 0-9
        0x70..=0x87 => 0xFFBE + (vk - 0x70), // F1-F24
        0x60..=0x69 => 0xFFB0 + (vk - 0x60), // KP_0-KP_9
        0x1B => 0xFF1B,
        0x20 => 0x20,
        0x0D => 0xFF0D,
        0x09 => 0xFF09,
        0x08 => 0xFF08,
        0x2D => 0xFF63,
        0x2E => 0xFFFF,
        0x24 => 0xFF50,
        0x23 => 0xFF57,
        0x21 => 0xFF55,
        0x22 => 0xFF56,
        0x25 => 0xFF51,
        0x26 => 0xFF52,
        0x27 => 0xFF53,
        0x28 => 0xFF54,
        0x13 => 0xFF13,
        0x91 => 0xFF14,
        0x2C => 0xFF61,
        _ => 0,
    }
}

/// Escribe el combo para la extensión.
pub fn write_combo(c: &Combo) -> Result<(), String> {
    let data = json!({
        "combo": c.to_string(),
        "keyval": keyval(c),
        "ctrl": c.ctrl,
        "shift": c.shift,
        "alt": c.alt,
        "super": c.win,
    });
    let _ = std::fs::create_dir_all(config_dir());
    std::fs::write(combo_file(), data.to_string()).map_err(|e| e.to_string())
}

fn enabled_list() -> Option<Vec<String>> {
    let out = sys::run("gsettings", &["get", "org.gnome.shell", "enabled-extensions"]).ok()?;
    Some(out.split('\'').skip(1).step_by(2).map(str::to_string).collect())
}

fn set_enabled_list(list: &[String]) -> Result<(), String> {
    let v = format!("[{}]", list.iter().map(|p| format!("'{p}'")).collect::<Vec<_>>().join(", "));
    sys::run("gsettings", &["set", "org.gnome.shell", "enabled-extensions", &v]).map(|_| ())
}

pub fn installed() -> bool {
    extension_dir().join("extension.js").is_file()
}

pub fn enabled() -> bool {
    enabled_list().is_some_and(|l| l.iter().any(|u| u == UUID))
}

/// Estado según GNOME Shell (`ACTIVE`, `INITIALIZED`…); `None` si aún no la cargó
/// (en Wayland, hasta volver a iniciar sesión).
pub fn shell_state() -> Option<String> {
    let out = sys::run("gnome-extensions", &["info", UUID]).ok()?;
    out.lines().find_map(|l| {
        let l = l.trim();
        l.strip_prefix("State:").or_else(|| l.strip_prefix("Estado:")).map(|s| s.trim().to_string())
    })
}

/// ¿El combo funcionará ya en la pantalla de bloqueo?
pub fn active() -> bool {
    shell_state().is_some_and(|s| s.eq_ignore_ascii_case("ACTIVE") || s.eq_ignore_ascii_case("ENABLED"))
}

/// ¿GNOME tiene desactivadas todas las extensiones de usuario?
pub fn user_extensions_disabled() -> bool {
    sys::run("gsettings", &["get", "org.gnome.shell", "disable-user-extensions"]).is_ok_and(|v| v == "true")
}

/// Copia la extensión y la activa. Devuelve si hace falta volver a iniciar sesión.
pub fn install() -> Result<bool, String> {
    let dir = extension_dir();
    std::fs::create_dir_all(&dir).map_err(|e| format!("no se pudo crear {}: {e}", dir.display()))?;
    std::fs::write(dir.join("metadata.json"), METADATA).map_err(|e| e.to_string())?;
    std::fs::write(dir.join("extension.js"), EXTENSION_JS).map_err(|e| e.to_string())?;
    let mut list = enabled_list().ok_or("no se pudo leer org.gnome.shell enabled-extensions")?;
    if !list.iter().any(|u| u == UUID) {
        list.push(UUID.into());
        set_enabled_list(&list)?;
    }
    // Si GNOME ya la conoce, se activa al momento; si no, al volver a entrar.
    let _ = sys::run("gnome-extensions", &["enable", UUID]);
    Ok(!active())
}

pub fn uninstall() -> bool {
    let had = installed();
    let _ = sys::run("gnome-extensions", &["disable", UUID]);
    if let Some(mut list) = enabled_list()
        && list.iter().any(|u| u == UUID)
    {
        list.retain(|u| u != UUID);
        let _ = set_enabled_list(&list);
    }
    let _ = std::fs::remove_dir_all(extension_dir());
    let _ = std::fs::remove_file(combo_file());
    had
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keysyms() {
        assert_eq!(keyval(&Combo::parse("Ctrl+Shift+Q").unwrap()), 0x71);
        assert_eq!(keyval(&Combo::parse("Ctrl+Alt+F12").unwrap()), 0xFFC9);
        assert_eq!(keyval(&Combo::parse("Ctrl+Num5").unwrap()), 0xFFB5);
        assert_eq!(keyval(&Combo::parse("Ctrl+Delete").unwrap()), 0xFFFF);
    }

    #[test]
    fn metadata_valida() {
        let m: serde_json::Value = serde_json::from_str(METADATA).unwrap();
        assert_eq!(m["uuid"], UUID);
        assert!(m["session-modes"].as_array().unwrap().iter().any(|s| s == "unlock-dialog"));
    }
}
