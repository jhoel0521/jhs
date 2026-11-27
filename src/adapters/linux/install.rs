//! Instalación por usuario (sin root): binarios en `~/.local/bin`, autoarranque
//! con `~/.config/autostart/jhs.desktop`, lanzadores en el menú de
//! aplicaciones y skill de Claude Code. El atajo de GNOME lo registra el agente.

use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use crate::adapters::linux::{gnome_unlock, hotkey, ipc, sys};
use crate::domain::combo::Combo;
use crate::application::app::{agent_running, wait_agent};
use crate::domain::config::{Config, config_dir, config_path};
use crate::domain::protocol::Request;
use crate::domain::state::data_dir;
use crate::ports::AutostartPort;

use crate::domain::version::SKILL;
const BINARIES: [&str; 2] = ["jhs", "jhsw"];

pub fn install_dir() -> PathBuf {
    sys::home().join(".local").join("bin")
}

fn exe_dir() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(Path::to_path_buf))
        .unwrap_or_else(|| PathBuf::from("."))
}

/// `jhsw` instalado, o junto al ejecutable actual, o el actual.
pub fn windowless_exe() -> PathBuf {
    for dir in [install_dir(), exe_dir()] {
        let w = dir.join("jhsw");
        if w.is_file() {
            return w;
        }
    }
    std::env::current_exe().unwrap_or_else(|_| PathBuf::from("jhsw"))
}

fn applications_dir() -> PathBuf {
    std::env::var_os("XDG_DATA_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| sys::home().join(".local").join("share"))
        .join("applications")
}

/// Comillas para la clave `Exec` de un `.desktop`.
fn desktop_quote(p: &Path) -> String {
    let s = p.display().to_string();
    if s.contains([' ', '"', '\\', '$', '`']) {
        format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\"").replace('$', "\\$").replace('`', "\\`"))
    } else {
        s
    }
}

fn desktop_entry(name: &str, comment: &str, exe: &Path, args: &str, icon: &str, extra: &str) -> String {
    format!(
        "[Desktop Entry]\nType=Application\nName={name}\nComment={comment}\nExec={} {args}\nIcon={icon}\nTerminal=false\n{extra}",
        desktop_quote(exe)
    )
}

// ─── Autoarranque ────────────────────────────────────────────────────────────

pub fn autostart_file() -> PathBuf {
    config_dir().parent().map(Path::to_path_buf).unwrap_or_else(|| sys::home().join(".config")).join("autostart").join("jhs.desktop")
}

/// Ejecutable al que apunta el autoarranque, si está configurado.
pub fn autostart_target() -> Option<PathBuf> {
    let text = std::fs::read_to_string(autostart_file()).ok()?;
    let exec = text.lines().find_map(|l| l.strip_prefix("Exec="))?;
    let exe = match exec.strip_prefix('"') {
        Some(r) => r.split('"').next()?.to_string(),
        None => exec.split_whitespace().next()?.to_string(),
    };
    Some(PathBuf::from(exe))
}

pub struct DesktopAutostart;

impl AutostartPort for DesktopAutostart {
    fn enabled(&self) -> bool {
        autostart_file().is_file()
    }

    fn set(&self, enable: bool) -> Result<(), String> {
        let f = autostart_file();
        if enable {
            if let Some(d) = f.parent() {
                std::fs::create_dir_all(d).map_err(|e| format!("no se pudo crear {}: {e}", d.display()))?;
            }
            let entry = desktop_entry(
                "jhs",
                "Agente de jhs: modo privacidad y apagado programado",
                &windowless_exe(),
                "agent",
                "system-lock-screen",
                "NoDisplay=true\nX-GNOME-Autostart-enabled=true\n",
            );
            std::fs::write(&f, entry).map_err(|e| format!("no se pudo escribir {}: {e}", f.display()))
        } else {
            match std::fs::remove_file(&f) {
                Ok(()) => Ok(()),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(e) => Err(format!("no se pudo borrar {}: {e}", f.display())),
            }
        }
    }
}

// ─── Lanzadores del menú de aplicaciones ─────────────────────────────────────

const LAUNCHERS: &[(&str, &str, &str, &str, &str)] = &[
    (
        "jhs-privacidad.desktop",
        "Modo privacidad",
        "Bloquea la pantalla dejando todo corriendo (jhs)",
        "privacy on",
        "system-lock-screen",
    ),
    ("jhs-apagado.desktop", "Programar apagado", "Apagar el equipo más tarde (jhs)", "shutdown ui", "system-shutdown"),
    (
        "jhs-cancelar-apagado.desktop",
        "Cancelar apagado",
        "Cancela el apagado programado (jhs)",
        "cancelar-apagado",
        "process-stop",
    ),
];

fn create_launchers(exe: &Path) -> Result<(), String> {
    let dir = applications_dir();
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    for (file, name, comment, args, icon) in LAUNCHERS {
        std::fs::write(dir.join(file), desktop_entry(name, comment, exe, args, icon, "Categories=Utility;\n"))
            .map_err(|e| format!("{file}: {e}"))?;
    }
    Ok(())
}

fn remove_launchers() {
    let dir = applications_dir();
    for (file, ..) in LAUNCHERS {
        let _ = std::fs::remove_file(dir.join(file));
    }
}

// ─── Agente ──────────────────────────────────────────────────────────────────

/// Lanza el agente desacoplado de la terminal (su propio grupo de procesos).
pub fn spawn_agent(exe: &Path) -> std::io::Result<()> {
    Command::new(exe)
        .arg("agent")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0)
        .spawn()
        .map(|_| ())
}

fn stop_agent() {
    if ipc::call(&Request::Stop, Duration::from_secs(5)).is_ok() {
        let t = Instant::now();
        while agent_running() && t.elapsed() < Duration::from_secs(5) {
            std::thread::sleep(Duration::from_millis(150));
        }
    }
}

pub fn skill_path() -> Option<PathBuf> {
    let claude = sys::home().join(".claude");
    claude.is_dir().then(|| claude.join("skills").join("jhs").join("SKILL.md"))
}

pub fn in_path(dir: &Path) -> bool {
    std::env::var_os("PATH").is_some_and(|p| std::env::split_paths(&p).any(|d| d == dir))
}

// ─── install / uninstall ─────────────────────────────────────────────────────

pub fn install() -> Result<Vec<String>, String> {
    let mut log = Vec::new();
    let src = exe_dir();
    let dest = install_dir();
    for name in BINARIES {
        if !src.join(name).is_file() {
            return Err(format!("falta {name} junto a este ejecutable (compila con `cargo build --release`)"));
        }
    }

    stop_agent();
    let same = std::fs::canonicalize(&src).ok() == std::fs::canonicalize(&dest).ok();
    if same {
        log.push(format!("Ya está en {}", dest.display()));
    } else {
        std::fs::create_dir_all(&dest).map_err(|e| format!("no se pudo crear {}: {e}", dest.display()))?;
        for name in BINARIES {
            let to = dest.join(name);
            // Borrar antes de copiar: se puede reemplazar un binario en uso.
            let _ = std::fs::remove_file(&to);
            std::fs::copy(src.join(name), &to).map_err(|e| format!("no se pudo copiar {name}: {e}"))?;
            let _ = std::fs::set_permissions(&to, std::fs::Permissions::from_mode(0o755));
        }
        log.push(format!("Copiado a {}", dest.display()));
    }
    if in_path(&dest) {
        log.push(format!("{} ya está en el PATH", dest.display()));
    } else {
        log.push(format!(
            "Aviso: {} no está en el PATH de esta terminal; en Ubuntu se añade solo al volver a iniciar sesión",
            dest.display()
        ));
    }

    DesktopAutostart.set(true)?;
    log.push(format!("Autoarranque al iniciar sesión: {}", autostart_file().display()));

    match create_launchers(&dest.join("jhsw")) {
        Ok(()) => log.push("Lanzadores en el menú de aplicaciones: Modo privacidad, Programar/Cancelar apagado".into()),
        Err(e) => log.push(format!("Aviso: no se pudieron crear los lanzadores: {e}")),
    }

    Config::ensure_file();
    log.push(format!("Configuración: {}", config_path().display()));

    if sys::is_gnome() {
        match gnome_unlock::install() {
            Ok(relogin) => {
                let (cfg, _) = Config::load();
                if let Ok(c) = Combo::parse(&cfg.privacidad.combo_desbloqueo) {
                    let _ = gnome_unlock::write_combo(&c);
                }
                log.push(if relogin {
                    "Extensión de GNOME para el combo de desbloqueo instalada: cierra sesión y vuelve a entrar una vez para activarla".into()
                } else {
                    "Extensión de GNOME para el combo de desbloqueo activa".into()
                });
            }
            Err(e) => log.push(format!("Aviso: no se pudo instalar la extensión de GNOME: {e}")),
        }
    }

    if let Some(p) = skill_path() {
        if let Some(dir) = p.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        match std::fs::write(&p, SKILL) {
            Ok(()) => log.push(format!("Skill de Claude Code instalada: {}", p.display())),
            Err(e) => log.push(format!("Aviso: no se pudo instalar la skill de Claude: {e}")),
        }
    }

    spawn_agent(&dest.join("jhsw")).map_err(|e| format!("no se pudo iniciar el agente: {e}"))?;
    if wait_agent(Duration::from_secs(4)) {
        log.push("Agente iniciado (registra el atajo de GNOME al arrancar)".into());
    } else {
        log.push("Aviso: el agente no respondió todavía; se iniciará al volver a iniciar sesión".into());
    }
    Ok(log)
}

pub fn uninstall() -> Result<Vec<String>, String> {
    let mut log = Vec::new();
    stop_agent();
    log.push("Agente detenido".into());
    DesktopAutostart.set(false)?;
    log.push("Autoarranque eliminado".into());
    if hotkey::remove_binding() {
        log.push("Atajo de GNOME eliminado".into());
    }
    if gnome_unlock::uninstall() {
        log.push("Extensión de GNOME eliminada".into());
    }
    remove_launchers();
    log.push("Lanzadores eliminados".into());
    if let Some(p) = skill_path().filter(|p| p.exists()) {
        let _ = std::fs::remove_dir_all(p.parent().unwrap_or(&p));
        log.push("Skill de Claude Code eliminada".into());
    }
    let dest = install_dir();
    for name in BINARIES {
        let p = dest.join(name);
        if p.is_file() {
            match std::fs::remove_file(&p) {
                Ok(()) => log.push(format!("Borrado {}", p.display())),
                Err(e) => log.push(format!("Aviso: no se pudo borrar {}: {e}", p.display())),
            }
        }
    }
    let _ = std::fs::remove_file(ipc::socket_path());
    log.push(format!(
        "Se conservan la configuración ({}) y los datos ({})",
        config_dir().display(),
        data_dir().display()
    ));
    Ok(log)
}
