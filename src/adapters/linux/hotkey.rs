//! Atajo global en GNOME: un "atajo personalizado" de GNOME Settings (gsettings)
//! que ejecuta `jhs privacy toggle`. Funciona en Wayland y X11, porque lo
//! escucha el propio GNOME Shell (un programa normal no puede capturar teclas
//! globales en Wayland).

use std::path::Path;

use crate::adapters::linux::sys;
use crate::domain::combo::Combo;
use crate::ports::HotkeyPort;

const MEDIA_KEYS: &str = "org.gnome.settings-daemon.plugins.media-keys";
const LIST_KEY: &str = "custom-keybindings";
const OUR_PATH: &str = "/org/gnome/settings-daemon/plugins/media-keys/custom-keybindings/jhs/";

/// Esquemas de GNOME con atajos que podrían chocar con el nuestro.
const SCHEMAS: &[&str] = &[
    "org.gnome.desktop.wm.keybindings",
    "org.gnome.shell.keybindings",
    "org.gnome.mutter.keybindings",
    "org.gnome.mutter.wayland.keybindings",
    MEDIA_KEYS,
];

/// Nombre de la tecla principal como keysym de GTK.
fn keysym(c: &Combo) -> String {
    match c.vk {
        0x41..=0x5A => ((c.vk as u8) as char).to_ascii_lowercase().to_string(),
        0x30..=0x39 => ((c.vk as u8) as char).to_string(),
        0x70..=0x87 => format!("F{}", c.vk - 0x70 + 1),
        0x60..=0x69 => format!("KP_{}", c.vk - 0x60),
        0x1B => "Escape".into(),
        0x20 => "space".into(),
        0x0D => "Return".into(),
        0x09 => "Tab".into(),
        0x08 => "BackSpace".into(),
        0x21 => "Page_Up".into(),
        0x22 => "Page_Down".into(),
        0x91 => "Scroll_Lock".into(),
        0x2C => "Print".into(),
        _ => c.key_name(),
    }
}

/// `Ctrl+Shift+Q` → `<Control><Shift>q`.
pub fn accelerator(c: &Combo) -> String {
    let mut s = String::new();
    if c.ctrl {
        s.push_str("<Control>");
    }
    if c.shift {
        s.push_str("<Shift>");
    }
    if c.alt {
        s.push_str("<Alt>");
    }
    if c.win {
        s.push_str("<Super>");
    }
    s.push_str(&keysym(c));
    s
}

/// Forma canónica de un acelerador para compararlos (modificadores ordenados,
/// tecla en minúsculas).
fn normalize(accel: &str) -> Option<String> {
    let mut mods = Vec::new();
    let mut rest = accel.trim();
    while let Some(r) = rest.strip_prefix('<') {
        let end = r.find('>')?;
        let m = match r[..end].to_ascii_lowercase().as_str() {
            "control" | "ctrl" | "primary" => "ctrl",
            "shift" => "shift",
            "alt" | "mod1" => "alt",
            "super" | "mod4" | "meta" => "super",
            other => return Some(format!("?{other}")),
        };
        mods.push(m);
        rest = &r[end + 1..];
    }
    if rest.is_empty() {
        return None;
    }
    mods.sort();
    mods.dedup();
    Some(format!("{}+{}", mods.join("+"), rest.to_ascii_lowercase()))
}

/// Cadenas entre comillas simples de una salida de gsettings.
fn quoted(s: &str) -> Vec<String> {
    s.split('\'').skip(1).step_by(2).map(str::to_string).collect()
}

fn gsettings(args: &[&str]) -> Result<String, String> {
    sys::run("gsettings", args)
}

fn custom_list() -> Result<Vec<String>, String> {
    gsettings(&["get", MEDIA_KEYS, LIST_KEY]).map(|o| quoted(&o))
}

fn set_custom_list(list: &[String]) -> Result<(), String> {
    let v = format!("[{}]", list.iter().map(|p| format!("'{p}'")).collect::<Vec<_>>().join(", "));
    gsettings(&["set", MEDIA_KEYS, LIST_KEY, &v]).map(|_| ())
}

fn relocatable(path: &str) -> String {
    format!("{MEDIA_KEYS}.custom-keybinding:{path}")
}

/// ¿Hay atajos de GNOME configurables en este escritorio?
pub fn available() -> bool {
    sys::is_gnome() && sys::have("gsettings") && custom_list().is_ok()
}

/// ¿Quién usa ya este combo en GNOME? `None` si está libre.
pub fn conflict(c: &Combo) -> Option<String> {
    let want = normalize(&accelerator(c))?;
    for schema in SCHEMAS {
        let Ok(out) = gsettings(&["list-recursively", schema]) else { continue };
        for line in out.lines() {
            let mut parts = line.splitn(3, ' ');
            let (_, Some(key), Some(value)) = (parts.next(), parts.next(), parts.next()) else { continue };
            if quoted(value).iter().any(|a| normalize(a).as_deref() == Some(want.as_str())) {
                return Some(format!("GNOME ({schema} {key})"));
            }
        }
    }
    for path in custom_list().unwrap_or_default() {
        if path == OUR_PATH {
            continue;
        }
        let Ok(b) = gsettings(&["get", &relocatable(&path), "binding"]) else { continue };
        if quoted(&b).iter().any(|a| normalize(a).as_deref() == Some(want.as_str())) {
            let name = gsettings(&["get", &relocatable(&path), "name"]).unwrap_or_default();
            return Some(format!("un atajo personalizado de GNOME ({})", name.trim_matches('\'')));
        }
    }
    None
}

/// Atajo personalizado de GNOME que lanza `<exe> privacy toggle`.
pub struct GnomeHotkey {
    pub exe: std::path::PathBuf,
}

/// `gsettings set` espera un GVariant: los textos van como literal entre comillas.
fn gvariant_str(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

fn shell_quote(p: &Path) -> String {
    format!("'{}'", p.display().to_string().replace('\'', r"'\''"))
}

impl HotkeyPort for GnomeHotkey {
    fn bind(&mut self, combo: &Combo) -> Result<(), String> {
        if !available() {
            return Err("los atajos globales solo se configuran solos en GNOME; crea uno en tu escritorio \
                        que ejecute `jhs privacy toggle`"
                .into());
        }
        if let Some(who) = conflict(combo) {
            return Err(format!("ya lo usa {who}"));
        }
        let path = relocatable(OUR_PATH);
        let command = format!("{} privacy toggle", shell_quote(&self.exe));
        gsettings(&["set", &path, "name", &gvariant_str("jhs: modo privacidad")])?;
        gsettings(&["set", &path, "command", &gvariant_str(&command)])?;
        gsettings(&["set", &path, "binding", &gvariant_str(&accelerator(combo))])?;
        let mut list = custom_list()?;
        if !list.iter().any(|p| p == OUR_PATH) {
            list.push(OUR_PATH.into());
            set_custom_list(&list)?;
        }
        Ok(())
    }

    fn unbind(&mut self) {
        remove_binding();
    }
}

/// Quita nuestro atajo personalizado de GNOME (si existe).
pub fn remove_binding() -> bool {
    let Ok(mut list) = custom_list() else { return false };
    let had = list.iter().any(|p| p == OUR_PATH);
    if had {
        list.retain(|p| p != OUR_PATH);
        let _ = set_custom_list(&list);
    }
    let path = relocatable(OUR_PATH);
    for key in ["name", "command", "binding"] {
        let _ = gsettings(&["reset", &path, key]);
    }
    had
}

/// Atajo actual registrado en GNOME por jhs, si lo hay.
pub fn current_binding() -> Option<String> {
    custom_list().ok()?.iter().find(|p| *p == OUR_PATH)?;
    gsettings(&["get", &relocatable(OUR_PATH), "binding"]).ok().map(|b| b.trim_matches('\'').to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn acelerador_gnome() {
        assert_eq!(accelerator(&Combo::parse("Ctrl+Shift+Q").unwrap()), "<Control><Shift>q");
        assert_eq!(accelerator(&Combo::parse("Win+Alt+F12").unwrap()), "<Alt><Super>F12");
        assert_eq!(accelerator(&Combo::parse("Ctrl+PageUp").unwrap()), "<Control>Page_Up");
        assert_eq!(accelerator(&Combo::parse("Shift+Num5").unwrap()), "<Shift>KP_5");
    }

    #[test]
    fn normaliza_aceleradores() {
        assert_eq!(normalize("<Shift><Primary>Q"), normalize("<Control><Shift>q"));
        assert_ne!(normalize("<Super>l"), normalize("<Control>l"));
        assert_eq!(normalize(""), None);
    }

    #[test]
    fn extrae_comillas() {
        assert_eq!(quoted("['/a/', '/b/']"), vec!["/a/", "/b/"]);
        assert!(quoted("@as []").is_empty());
    }
}
