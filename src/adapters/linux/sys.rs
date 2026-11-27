//! Utilidades de Linux: ejecutar comandos, sesión de logind y escritorio.

use std::path::PathBuf;
use std::process::{Command, Stdio};

/// Ejecuta un programa y devuelve su stdout (recortado) si terminó bien.
pub fn run(prog: &str, args: &[&str]) -> Result<String, String> {
    let out = Command::new(prog)
        .args(args)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("no se pudo ejecutar {prog}: {e}"))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    } else {
        let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
        Err(format!("{prog} {} falló: {err}", args.join(" ")))
    }
}

/// ¿Existe `prog` en el PATH?
pub fn have(prog: &str) -> bool {
    std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).any(|d| d.join(prog).is_file()))
        .unwrap_or(false)
}

/// UID del usuario actual (dueño de `/proc/self`).
pub fn uid() -> u32 {
    use std::os::unix::fs::MetadataExt;
    std::fs::metadata("/proc/self").map(|m| m.uid()).unwrap_or(0)
}

/// Sesión gráfica de logind del usuario: la del proceso (`XDG_SESSION_ID`) o,
/// si se lanzó fuera de una (p. ej. un servicio), la sesión "Display" del usuario.
pub fn session_id() -> Option<String> {
    if let Some(id) = std::env::var("XDG_SESSION_ID").ok().filter(|s| !s.is_empty()) {
        return Some(id);
    }
    run("loginctl", &["show-user", &uid().to_string(), "-p", "Display", "--value"])
        .ok()
        .filter(|s| !s.is_empty())
}

/// Propiedad de una sesión de logind (`LockedHint`, `Type`…).
pub fn session_prop(id: &str, prop: &str) -> Option<String> {
    run("loginctl", &["show-session", id, "-p", prop, "--value"]).ok()
}

pub fn session_locked(id: &str) -> Option<bool> {
    session_prop(id, "LockedHint").map(|v| v == "yes")
}

/// Escritorio actual en minúsculas (`XDG_CURRENT_DESKTOP`, p. ej. "ubuntu:gnome").
pub fn desktop() -> String {
    std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default().to_lowercase()
}

pub fn is_gnome() -> bool {
    desktop().split(':').any(|d| d == "gnome")
}

/// Tipo de sesión: "wayland", "x11" o "tty".
pub fn session_type() -> String {
    std::env::var("XDG_SESSION_TYPE")
        .ok()
        .filter(|s| !s.is_empty())
        .or_else(|| session_id().and_then(|id| session_prop(&id, "Type")))
        .unwrap_or_else(|| "desconocido".into())
}

pub fn home() -> PathBuf {
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(std::env::temp_dir)
}

/// RAM residente del proceso en MB (de `/proc/self/status`).
pub fn ram_mb() -> f64 {
    let kb = std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|s| {
            s.lines()
                .find(|l| l.starts_with("VmRSS:"))
                .and_then(|l| l.split_whitespace().nth(1))
                .and_then(|v| v.parse::<f64>().ok())
        })
        .unwrap_or(0.0);
    (kb / 1024.0 * 10.0).round() / 10.0
}

/// Abre un archivo con la app predeterminada del escritorio.
pub fn open_file(path: &std::path::Path) {
    let _ = Command::new("xdg-open")
        .arg(path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
}
