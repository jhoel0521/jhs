//! Adaptadores para Linux (desarrollado en Ubuntu 26.04, GNOME 50, Wayland):
//!
//! | Puerto | Adaptador |
//! |---|---|
//! | IPC | socket Unix en `$XDG_RUNTIME_DIR` ([`ipc`]) |
//! | Privacidad | pantalla de bloqueo vía logind ([`privacy`]) |
//! | Apagado | logind `ScheduleShutdown` ([`power`]) |
//! | Atajo | atajo personalizado de GNOME ([`hotkey`]) |
//! | Bandeja | StatusNotifierItem con `ksni` (`tray`, feature `tray`) |
//! | Avisos y ventanas | `notify-send` y `zenity` ([`notify`]) |
//! | Autoarranque | `~/.config/autostart` ([`install`]) |

pub mod doctor;
pub mod gnome_unlock;
pub mod hotkey;
pub mod install;
pub mod ipc;
pub mod notify;
pub mod power;
pub mod privacy;
pub mod sys;
#[cfg(feature = "tray")]
pub mod tray;

use std::sync::{Arc, mpsc};
use std::time::Duration;

use serde_json::Value;

use crate::application::agent::{Agent, Event, Ports};
use crate::domain::combo::Combo;
use crate::domain::config::Config;
use crate::domain::doctor::Check;
use crate::domain::protocol::{ClientError, Request, Response};
use crate::domain::state;
use crate::ports::{DialogPort, HotkeyPort, Platform, TrayPort};

pub struct Linux;

pub static PLATFORM: Linux = Linux;

/// Atajo cuando no hay GNOME: se informa y el usuario lo crea a mano.
struct NoHotkey;

impl HotkeyPort for NoHotkey {
    fn bind(&mut self, _combo: &Combo) -> Result<(), String> {
        Err("este escritorio no permite registrarlo automáticamente; crea un atajo que ejecute \
             `jhs privacy toggle`"
            .into())
    }
    fn unbind(&mut self) {}
}

fn tray(events: &mpsc::Sender<Event>) -> Option<Box<dyn TrayPort>> {
    #[cfg(feature = "tray")]
    return tray::Tray::spawn(events.clone()).map(|t| Box::new(t) as Box<dyn TrayPort>);
    #[cfg(not(feature = "tray"))]
    {
        let _ = events;
        None
    }
}

/// Ejecuta el agente: un solo ejemplar por usuario (candado en `$XDG_RUNTIME_DIR`).
fn run_agent(dry_run: bool) -> i32 {
    let lock = match std::fs::OpenOptions::new().create(true).truncate(false).write(true).open(ipc::lock_path()) {
        Ok(f) => f,
        Err(e) => {
            state::log(&format!("no se pudo abrir el candado del agente: {e}"));
            return 1;
        }
    };
    if lock.try_lock().is_err() {
        return 0; // ya hay un agente corriendo
    }
    Config::ensure_file();
    let listener = match ipc::bind() {
        Ok(l) => l,
        Err(e) => {
            state::log(&format!("servidor IPC: {e}"));
            return 1;
        }
    };

    let (tx, rx) = mpsc::channel();
    let hotkey: Box<dyn HotkeyPort> = if hotkey::available() {
        Box::new(hotkey::GnomeHotkey { exe: install::windowless_exe() })
    } else {
        Box::new(NoHotkey)
    };
    let ports = Ports {
        privacy: Box::new(privacy::LockScreen::new()),
        power: Box::new(power::Logind::default()),
        notifier: Box::new(notify::NotifySend),
        hotkey,
        tray: tray(&tx),
        autostart: Box::new(install::DesktopAutostart),
        dialog: Arc::new(notify::Zenity),
        open_file: sys::open_file,
        ram_mb: sys::ram_mb,
        doctor: doctor::collect,
    };
    let agent = Agent::new(ports, dry_run, tx.clone());

    let events = tx;
    std::thread::Builder::new()
        .name("ipc".into())
        .spawn(move || ipc::serve(listener, events))
        .ok();

    let code = agent.run(rx);
    let _ = std::fs::remove_file(ipc::socket_path());
    drop(lock);
    code
}

impl Platform for Linux {
    fn os_name(&self) -> &'static str {
        "Linux"
    }

    fn call(&self, req: &Request, timeout: Duration, _give_focus: bool) -> Result<Response, ClientError> {
        ipc::call(req, timeout)
    }

    fn spawn_agent(&self) -> Result<(), String> {
        install::spawn_agent(&install::windowless_exe()).map_err(|e| e.to_string())
    }

    fn run_agent(&self, dry_run: bool) -> i32 {
        run_agent(dry_run)
    }

    fn dialog(&self) -> &dyn DialogPort {
        &notify::Zenity
    }

    fn rescue_privacy(&self) -> Option<Result<(), String>> {
        // Solo si el modo lo activó jhs (no un bloqueo normal con Super+L).
        let session = std::fs::read_to_string(privacy::marker_path()).ok()?;
        let session = session.trim();
        let r = privacy::unlock(session);
        let _ = std::fs::remove_file(privacy::marker_path());
        Some(r)
    }

    fn hotkey_free(&self, combo: &Combo) -> Result<(), String> {
        if !hotkey::available() {
            return Err("no se puede comprobar: solo se gestionan atajos en GNOME".into());
        }
        match hotkey::conflict(combo) {
            None => Ok(()),
            Some(who) => Err(format!("ya lo usa {who}")),
        }
    }

    fn doctor(&self, status: Option<&Value>, cfg: &Config, cfg_error: Option<&str>) -> Vec<Check> {
        doctor::collect(status, cfg, cfg_error)
    }

    fn skill_path(&self) -> Option<std::path::PathBuf> {
        install::skill_path()
    }

    fn install(&self) -> Result<Vec<String>, String> {
        install::install()
    }

    fn uninstall(&self) -> Result<Vec<String>, String> {
        install::uninstall()
    }
}
