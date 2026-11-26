//! Diagnóstico: agente, atajos, escritorio privado, autoarranque, PATH y apps
//! que podrían interceptar el teclado.

use std::mem::{size_of, zeroed};

use serde::Serialize;
use serde_json::Value;
use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
};

use crate::agent::privacy;
use crate::config::{Config, config_path};
use crate::hotkey::{self, Combo};
use crate::install;
use crate::win::from_wide;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Level {
    Ok,
    Aviso,
    Error,
}

#[derive(Debug, Clone, Serialize)]
pub struct Check {
    pub nombre: String,
    pub nivel: Level,
    pub detalle: String,
}

fn check(nombre: &str, nivel: Level, detalle: impl Into<String>) -> Check {
    Check { nombre: nombre.into(), nivel, detalle: detalle.into() }
}

/// Programas conocidos que escuchan el teclado con hooks globales
/// (`RegisterHotKey` no puede detectarlos).
const HOOK_APPS: &[(&str, &str)] = &[
    ("autohotkey.exe", "AutoHotkey"),
    ("autohotkey64.exe", "AutoHotkey"),
    ("autohotkey32.exe", "AutoHotkey"),
    ("autohotkeyu64.exe", "AutoHotkey"),
    ("autohotkeyu32.exe", "AutoHotkey"),
    ("autohotkeyux.exe", "AutoHotkey"),
    ("powertoys.keyboardmanagerengine.exe", "PowerToys Keyboard Manager"),
    ("logioptionsplus_agent.exe", "Logitech Options+"),
    ("keypirinha-x64.exe", "Keypirinha"),
];

fn running_processes() -> Vec<String> {
    let mut out = Vec::new();
    unsafe {
        let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snap == INVALID_HANDLE_VALUE {
            return out;
        }
        let mut pe: PROCESSENTRY32W = zeroed();
        pe.dwSize = size_of::<PROCESSENTRY32W>() as u32;
        if Process32FirstW(snap, &mut pe) != 0 {
            loop {
                out.push(from_wide(&pe.szExeFile).to_ascii_lowercase());
                if Process32NextW(snap, &mut pe) == 0 {
                    break;
                }
            }
        }
        CloseHandle(snap);
    }
    out
}

/// Reúne los chequeos. `status` es el JSON de `Request::Status` si el agente responde.
pub fn collect(status: Option<&Value>, cfg: &Config, cfg_error: Option<&str>) -> Vec<Check> {
    let mut v = Vec::new();

    // Agente
    match status {
        Some(s) => {
            let a = &s["agente"];
            let prueba = if a["modo_prueba"].as_bool() == Some(true) { ", MODO PRUEBA" } else { "" };
            v.push(check(
                "Agente",
                Level::Ok,
                format!(
                    "activo: pid {}, {} MB de RAM, desde {}{prueba}",
                    a["pid"], a["ram_mb"], a["desde"].as_str().unwrap_or("?")
                ),
            ));
        }
        None => v.push(check("Agente", Level::Error, "no está corriendo (inícialo con `jh-script agent` o `jh-script install`)")),
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
                    format!("{c} registrado por jh-script (libre en Windows)"),
                )),
                Some(s) => v.push(check(
                    "Atajo de activación",
                    Level::Error,
                    format!("{c}: {}", s["hotkey"]["error"].as_str().unwrap_or("no registrado")),
                )),
                None => match hotkey::test_free(&c) {
                    Ok(()) => v.push(check("Atajo de activación", Level::Ok, format!("{c} está libre en Windows"))),
                    Err(code) => v.push(check(
                        "Atajo de activación",
                        Level::Error,
                        format!("{c} {}", hotkey::explain_error(code)),
                    )),
                },
            },
        }
    }

    // Combo de desbloqueo
    match Combo::parse(&cfg.privacidad.combo_desbloqueo) {
        Ok(c) => v.push(check(
            "Combo de desbloqueo",
            Level::Ok,
            format!("{c} (exclusivo: dentro del escritorio privado ningún otro programa lo escucha)"),
        )),
        Err(e) => v.push(check("Combo de desbloqueo", Level::Error, format!("{e}; se usará Ctrl+Shift+Q"))),
    }

    // Escritorio privado
    match privacy::test_create_desktop() {
        Ok(()) => v.push(check("Escritorio privado", Level::Ok, "se puede crear (prueba en seco)")),
        Err(e) => v.push(check("Escritorio privado", Level::Error, format!("no se puede crear: {e}"))),
    }

    // Autoarranque
    match install::autostart_command() {
        Some(cmd) if install::autostart_target_exists(&cmd) => {
            v.push(check("Autoarranque", Level::Ok, format!("al iniciar sesión: {cmd}")))
        }
        Some(cmd) => v.push(check("Autoarranque", Level::Error, format!("apunta a un archivo que no existe: {cmd}"))),
        None => v.push(check("Autoarranque", Level::Aviso, "no configurado (`jh-script install`)")),
    }

    // PATH
    if install::in_user_path() {
        v.push(check("PATH", Level::Ok, format!("{} está en el PATH del usuario", install::install_dir().display())));
    } else {
        v.push(check("PATH", Level::Aviso, "jh-script no está en el PATH del usuario (`jh-script install`)"));
    }

    // Configuración
    match cfg_error {
        Some(e) => v.push(check("Configuración", Level::Error, e)),
        None => v.push(check("Configuración", Level::Ok, config_path().display().to_string())),
    }

    // Apps con hooks de teclado
    let procs = running_processes();
    let mut found: Vec<&str> = HOOK_APPS
        .iter()
        .filter(|(exe, _)| procs.iter().any(|p| p == exe))
        .map(|&(_, name)| name)
        .collect();
    found.dedup();
    if found.is_empty() {
        v.push(check("Apps que escuchan el teclado", Level::Ok, "no se detectaron (AutoHotkey, PowerToys…)"));
    } else {
        v.push(check(
            "Apps que escuchan el teclado",
            Level::Aviso,
            format!(
                "{}: podrían interceptar el atajo de activación antes que Windows (dentro del modo privacidad no ven nada)",
                found.join(", ")
            ),
        ));
    }

    v
}

pub fn render_text(checks: &[Check]) -> String {
    checks
        .iter()
        .map(|c| {
            let mark = match c.nivel {
                Level::Ok => "✔",
                Level::Aviso => "!",
                Level::Error => "✖",
            };
            format!("{mark} {}: {}", c.nombre, c.detalle)
        })
        .collect::<Vec<_>>()
        .join("\n")
}
