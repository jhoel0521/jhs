//! Modo privacidad con la pantalla de bloqueo de la sesión (logind).
//!
//! En Wayland un programa normal no puede tapar la pantalla ni capturar el
//! teclado; la vía segura es el bloqueo real: `loginctl lock-session` lo pide
//! a GNOME (o a cualquier escritorio que escuche a logind) y
//! `loginctl unlock-session` lo quita sin contraseña desde el CLI.
//! Todos los programas siguen corriendo, y si el agente muere la sesión sigue
//! bloqueada (se sale con la contraseña de siempre).
//!
//! Mientras está activo se bloquea la suspensión con `systemd-inhibit`.

use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use crate::adapters::linux::sys;
use crate::domain::config::Config;
use crate::domain::paths::runtime_dir;
use crate::domain::state;
use crate::ports::{PrivacyEnd, PrivacyPort};

/// Marca de "modo privacidad activo" (para el rescate desde el CLI si el agente muere).
pub fn marker_path() -> PathBuf {
    runtime_dir().join("jhs.privacy")
}

pub struct LockScreen {
    session: Option<String>,
    inhibitor: Option<Child>,
    since: Option<Instant>,
    seen_locked: bool,
}

impl LockScreen {
    pub fn new() -> LockScreen {
        LockScreen { session: None, inhibitor: None, since: None, seen_locked: false }
    }

    fn release_inhibitor(&mut self) {
        if let Some(mut c) = self.inhibitor.take() {
            let _ = c.kill();
            let _ = c.wait();
        }
    }

    fn cleanup(&mut self) {
        self.release_inhibitor();
        self.since = None;
        self.seen_locked = false;
        let _ = std::fs::remove_file(marker_path());
    }
}

impl Drop for LockScreen {
    fn drop(&mut self) {
        self.release_inhibitor();
    }
}

/// Bloquea la suspensión (y el reposo por inactividad si `keep_display`).
fn spawn_inhibitor(keep_display: bool) -> Option<Child> {
    if !sys::have("systemd-inhibit") {
        return None;
    }
    let what = if keep_display { "sleep:idle" } else { "sleep" };
    Command::new("systemd-inhibit")
        .args([
            &format!("--what={what}"),
            "--who=jhs",
            "--why=Modo privacidad activo: los programas siguen corriendo",
            "--mode=block",
            "sleep",
            "infinity",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .ok()
}

pub fn unlock(session: &str) -> Result<(), String> {
    sys::run("loginctl", &["unlock-session", session]).map(|_| ())
}

impl PrivacyPort for LockScreen {
    fn backend(&self) -> String {
        if sys::is_gnome() {
            "pantalla de bloqueo de GNOME (logind)".into()
        } else {
            "pantalla de bloqueo de la sesión (logind)".into()
        }
    }

    fn unlock_hint(&self, _cfg: &Config) -> String {
        "tu contraseña en la pantalla de bloqueo o `jhs privacy off`".into()
    }

    fn activate(&mut self, keep_display: bool) -> Result<(), String> {
        let id = sys::session_id().ok_or("no se encontró la sesión gráfica en logind")?;
        sys::run("loginctl", &["lock-session", &id])?;
        let _ = std::fs::write(marker_path(), &id);
        self.inhibitor = spawn_inhibitor(keep_display);
        self.session = Some(id);
        self.since = Some(Instant::now());
        self.seen_locked = false;
        Ok(())
    }

    fn deactivate(&mut self) -> Result<(), String> {
        let id = self.session.clone().or_else(sys::session_id).ok_or("no se encontró la sesión")?;
        unlock(&id)?;
        self.cleanup();
        Ok(())
    }

    fn poll(&mut self) -> Option<PrivacyEnd> {
        let id = self.session.clone()?;
        match sys::session_locked(&id) {
            Some(true) => {
                self.seen_locked = true;
                None
            }
            // El escritorio tarda un momento en bloquear: se espera antes de
            // interpretar "desbloqueada" como que el usuario salió.
            Some(false) if self.seen_locked || self.since.is_some_and(|t| t.elapsed() > Duration::from_secs(10)) => {
                if !self.seen_locked {
                    state::log("la sesión nunca llegó a bloquearse (¿el escritorio no escucha a logind?)");
                }
                let reason = if self.seen_locked { PrivacyEnd::Unlocked } else { PrivacyEnd::Error };
                self.cleanup();
                Some(reason)
            }
            _ => None,
        }
    }
}
