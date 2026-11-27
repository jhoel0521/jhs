//! Diagnóstico en Linux: agente, sesión de logind, permisos de apagado, atajo
//! de GNOME, bandeja, herramientas del escritorio, autoarranque y PATH.

use serde_json::Value;

use crate::adapters::linux::{gnome_unlock, hotkey, install, sys};
use crate::domain::combo::Combo;
use crate::domain::config::{Config, config_path};
use crate::domain::doctor::{Check, Level, agent_check, check};

fn can_power_off() -> Option<String> {
    sys::run(
        "busctl",
        &["call", "org.freedesktop.login1", "/org/freedesktop/login1", "org.freedesktop.login1.Manager", "CanPowerOff"],
    )
    .ok()
    .map(|o| o.trim_start_matches("s ").trim_matches('"').to_string())
}

fn appindicator_enabled() -> Option<bool> {
    let out = sys::run("gnome-extensions", &["list", "--enabled"]).ok()?;
    Some(out.lines().any(|l| l.to_lowercase().contains("appindicator")))
}

pub fn collect(status: Option<&Value>, cfg: &Config, cfg_error: Option<&str>) -> Vec<Check> {
    let mut v = vec![agent_check(status)];

    // Escritorio y sesión
    let desktop = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default();
    let desktop = if desktop.is_empty() { "desconocido".into() } else { desktop };
    v.push(check("Escritorio", Level::Ok, format!("{desktop} ({})", sys::session_type())));
    match sys::session_id() {
        Some(id) => match sys::session_locked(&id) {
            Some(locked) => v.push(check(
                "Sesión de logind",
                Level::Ok,
                format!("sesión {id}{}", if locked { " (bloqueada ahora)" } else { "" }),
            )),
            None => v.push(check("Sesión de logind", Level::Error, format!("no se pudo leer la sesión {id}"))),
        },
        None => v.push(check(
            "Sesión de logind",
            Level::Error,
            "no se encontró una sesión gráfica: el modo privacidad no funcionará",
        )),
    }

    // Modo privacidad
    if !sys::have("loginctl") {
        v.push(check("Modo privacidad", Level::Error, "falta loginctl (systemd)"));
    } else if sys::is_gnome() {
        v.push(check("Modo privacidad", Level::Ok, "pantalla de bloqueo de GNOME; se sale con tu contraseña o `privacy off`"));
    } else {
        v.push(check(
            "Modo privacidad",
            Level::Aviso,
            "pantalla de bloqueo vía logind: tu escritorio debe atender `loginctl lock-session` (KDE sí; en otros, p. ej. xss-lock)",
        ));
    }

    // Apagado
    match can_power_off().as_deref() {
        Some("yes") => v.push(check("Apagado", Level::Ok, "logind permite apagar sin contraseña")),
        Some("challenge") => v.push(check(
            "Apagado",
            Level::Aviso,
            "logind pide contraseña (¿hay otros usuarios conectados?); se usará el temporizador del agente",
        )),
        Some(other) => v.push(check("Apagado", Level::Error, format!("logind no permite apagar (CanPowerOff = {other})"))),
        None => v.push(check("Apagado", Level::Error, "no se pudo consultar logind (busctl)")),
    }

    // Atajo de activación
    let atajo = cfg.privacidad.atajo_activar.trim();
    if atajo.is_empty() {
        v.push(check("Atajo de activación", Level::Aviso, "desactivado (atajo_activar = \"\")"));
    } else {
        match Combo::parse(atajo) {
            Err(e) => v.push(check("Atajo de activación", Level::Error, e)),
            Ok(c) => match status {
                Some(s) if s["hotkey"]["registrada"].as_bool() == Some(true) => v.push(check(
                    "Atajo de activación",
                    Level::Ok,
                    format!("{c} registrado en GNOME ({})", hotkey::accelerator(&c)),
                )),
                Some(s) => v.push(check(
                    "Atajo de activación",
                    Level::Error,
                    format!("{c}: {}", s["hotkey"]["error"].as_str().unwrap_or("no registrado")),
                )),
                None if !hotkey::available() => v.push(check(
                    "Atajo de activación",
                    Level::Aviso,
                    "solo se configura solo en GNOME; en otro escritorio crea un atajo que ejecute `jhs privacy toggle`",
                )),
                None => match hotkey::conflict(&c) {
                    None => v.push(check("Atajo de activación", Level::Ok, format!("{c} está libre en GNOME"))),
                    Some(who) => v.push(check("Atajo de activación", Level::Error, format!("{c} ya lo usa {who}"))),
                },
            },
        }
    }
    v.push(unlock_combo_check(cfg));

    // Bandeja
    if !cfg!(feature = "tray") {
        v.push(check("Bandeja", Level::Aviso, "compilado sin icono de bandeja (feature `tray`)"));
    } else if sys::is_gnome() {
        match appindicator_enabled() {
            Some(true) => v.push(check("Bandeja", Level::Ok, "extensión AppIndicator de GNOME activa")),
            Some(false) => v.push(check(
                "Bandeja",
                Level::Aviso,
                "activa la extensión AppIndicator de GNOME para ver el icono (el resto funciona igual)",
            )),
            None => v.push(check("Bandeja", Level::Aviso, "no se pudo comprobar la extensión AppIndicator")),
        }
    } else {
        v.push(check("Bandeja", Level::Ok, "StatusNotifierItem (si tu panel lo admite)"));
    }

    // Herramientas del escritorio
    for (prog, what, pkg) in [("notify-send", "Notificaciones", "libnotify-bin"), ("zenity", "Ventanas", "zenity")] {
        if sys::have(prog) {
            v.push(check(what, Level::Ok, format!("{prog} disponible")));
        } else {
            v.push(check(what, Level::Aviso, format!("falta {prog} (sudo apt install {pkg})")));
        }
    }

    // Autoarranque
    match install::autostart_target() {
        Some(p) if p.is_file() => v.push(check("Autoarranque", Level::Ok, format!("al iniciar sesión: {}", p.display()))),
        Some(p) => v.push(check("Autoarranque", Level::Error, format!("apunta a un archivo que no existe: {}", p.display()))),
        None => v.push(check("Autoarranque", Level::Aviso, "no configurado (`jhs install`)")),
    }

    // PATH
    let dir = install::install_dir();
    if dir.join("jhs").is_file() && install::in_path(&dir) {
        v.push(check("PATH", Level::Ok, format!("{} está en el PATH", dir.display())));
    } else if dir.join("jhs").is_file() {
        v.push(check("PATH", Level::Aviso, format!("{} no está en el PATH de esta terminal (vuelve a iniciar sesión)", dir.display())));
    } else {
        v.push(check("PATH", Level::Aviso, "jhs no está instalado en ~/.local/bin (`jhs install`)"));
    }

    // Configuración
    match cfg_error {
        Some(e) => v.push(check("Configuración", Level::Error, e)),
        None => v.push(check("Configuración", Level::Ok, config_path().display().to_string())),
    }

    v
}

fn unlock_combo_check(cfg: &Config) -> Check {
    const NAME: &str = "Combo de desbloqueo";
    let combo = match Combo::parse(&cfg.privacidad.combo_desbloqueo) {
        Ok(c) => c,
        Err(e) => return check(NAME, Level::Error, e),
    };
    if !sys::is_gnome() {
        return check(NAME, Level::Aviso, "solo en GNOME; aquí se sale con tu contraseña o `jhs privacy off`");
    }
    if !gnome_unlock::installed() {
        return check(NAME, Level::Aviso, "extensión de GNOME no instalada (`jhs install`); se sale con contraseña o `privacy off`");
    }
    if gnome_unlock::user_extensions_disabled() {
        return check(NAME, Level::Error, "GNOME tiene desactivadas las extensiones de usuario (actívalas en la app Extensiones)");
    }
    if !gnome_unlock::enabled() {
        return check(NAME, Level::Error, "la extensión está instalada pero desactivada (`jhs install` la activa)");
    }
    match gnome_unlock::shell_state() {
        Some(s) if s.eq_ignore_ascii_case("ACTIVE") || s.eq_ignore_ascii_case("ENABLED") => {
            check(NAME, Level::Ok, format!("{combo} desbloquea el modo privacidad desde la pantalla de bloqueo"))
        }
        Some(s) => check(NAME, Level::Error, format!("la extensión de GNOME está en estado {s} (revisa `journalctl --user -b | grep jhs`)")),
        None => check(NAME, Level::Aviso, "extensión instalada: cierra sesión y vuelve a entrar una vez para activarla"),
    }
}
