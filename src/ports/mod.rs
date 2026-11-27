//! Puertos (arquitectura hexagonal).
//!
//! - **Entrada** ([`AgentService`]): lo que el mundo exterior (CLI por IPC,
//!   bandeja, atajo) le pide al núcleo del agente.
//! - **Salida** (el resto): lo que el núcleo necesita del sistema operativo.
//!   Cada plataforma los implementa en `crate::adapters::<so>`.
//!
//! El núcleo (`crate::application`) solo conoce estos traits; para llevar
//! jhs a otro escritorio basta con escribir otros adaptadores.

use std::time::Duration;

use serde_json::Value;

use crate::domain::combo::Combo;
use crate::domain::config::Config;
use crate::domain::doctor::Check;
use crate::domain::protocol::{ClientError, Request, Response};

// ─── Entrada ─────────────────────────────────────────────────────────────────

/// Casos de uso del agente, invocables por IPC, bandeja o atajo.
pub trait AgentService {
    fn handle(&mut self, req: Request) -> Response;
}

// ─── Salida: agente ──────────────────────────────────────────────────────────

/// Motivo por el que terminó el modo privacidad.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrivacyEnd {
    /// `jhs privacy off`, la bandeja o el atajo.
    Cli,
    /// El combo secreto en la pantalla de bloqueo.
    Combo,
    /// Terminó el tiempo de `--for`.
    Timeout,
    /// El usuario desbloqueó la sesión (p. ej. con su contraseña).
    Unlocked,
    /// Falló el adaptador.
    Error,
}

impl PrivacyEnd {
    pub fn key(self) -> &'static str {
        match self {
            PrivacyEnd::Cli => "cli",
            PrivacyEnd::Combo => "combo",
            PrivacyEnd::Timeout => "tiempo",
            PrivacyEnd::Unlocked => "desbloqueo",
            PrivacyEnd::Error => "error",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            PrivacyEnd::Cli => "desactivado por comando",
            PrivacyEnd::Combo => "desbloqueado con el combo",
            PrivacyEnd::Timeout => "terminó el tiempo",
            PrivacyEnd::Unlocked => "desbloqueado en la pantalla de bloqueo",
            PrivacyEnd::Error => "error",
        }
    }
}

/// Oculta la pantalla y bloquea la entrada mientras todo sigue corriendo.
pub trait PrivacyPort {
    /// Nombre del mecanismo ("pantalla de bloqueo de GNOME"…).
    fn backend(&self) -> String;
    /// Cómo se sale del modo (se muestra al activarlo).
    fn unlock_hint(&self, cfg: &Config) -> String;
    /// Aplica la configuración (p. ej. el combo de desbloqueo). Al arrancar y al recargar.
    fn configure(&mut self, _cfg: &Config) {}
    fn activate(&mut self, keep_display: bool) -> Result<(), String>;
    fn deactivate(&mut self) -> Result<(), String>;
    /// Llamado cada segundo mientras está activo: `Some` si terminó por fuera.
    fn poll(&mut self) -> Option<PrivacyEnd>;
}

/// Apagado del equipo a nivel sistema.
pub trait PowerPort {
    /// Programa el apagado dentro de `seconds`.
    fn schedule(&mut self, seconds: u64, force: bool) -> Result<(), String>;
    /// Cancela. `Ok(true)` si el sistema tenía un apagado pendiente.
    fn cancel(&mut self) -> Result<bool, String>;
    /// Apagado pendiente según el sistema (segundos UNIX), si se puede saber.
    fn pending(&self) -> Option<i64> {
        None
    }
    /// Se llama al llegar la hora (para adaptadores sin apagado a nivel sistema).
    fn deadline_reached(&mut self) {}
    /// Descripción para el log ("logind ScheduleShutdown"…).
    fn describe(&self, seconds: u64, force: bool) -> String;
}

/// Notificaciones de escritorio.
pub trait NotifierPort {
    fn notify(&self, title: &str, text: &str, warning: bool);
}

/// Atajo global que ejecuta `privacy toggle`.
pub trait HotkeyPort {
    /// Registra (o reemplaza) el atajo.
    fn bind(&mut self, combo: &Combo) -> Result<(), String>;
    fn unbind(&mut self);
}

/// Lo que la bandeja muestra; el adaptador lo pinta como pueda.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TrayModel {
    pub tooltip: String,
    pub hotkey: Option<String>,
    pub presets_min: Vec<u32>,
    pub remaining: Option<i64>,
    pub autostart: bool,
    pub privacy_active: bool,
}

/// Acciones que llegan desde la bandeja.
#[derive(Debug, Clone, PartialEq)]
pub enum TrayAction {
    PrivacyOn,
    ScheduleMinutes(u32),
    ScheduleCustom,
    CancelShutdown,
    Doctor,
    ToggleAutostart,
    OpenConfig,
    ReloadConfig,
    Exit,
}

pub trait TrayPort {
    fn update(&mut self, model: &TrayModel);
}

/// Diálogos simples (mensajes y preguntas).
pub trait DialogPort: Send + Sync {
    fn message(&self, text: &str, title: &str, error: bool);
    /// Pide un texto al usuario. `None` si canceló o no hay UI.
    fn ask(&self, title: &str, prompt: &str) -> Option<String>;
}

/// Autoarranque del agente al iniciar sesión.
pub trait AutostartPort {
    fn enabled(&self) -> bool;
    fn set(&self, enable: bool) -> Result<(), String>;
}

// ─── Salida: actualizaciones ─────────────────────────────────────────────────

/// Una versión publicada del programa.
#[derive(Debug, Clone)]
pub struct Release {
    pub version: crate::domain::version::Version,
    /// Página de la release.
    pub url: String,
    /// `(nombre, url de descarga)` de cada archivo.
    pub assets: Vec<(String, String)>,
}

/// Dónde se publican las versiones (GitHub Releases).
pub trait ReleasesPort {
    fn latest(&self) -> Result<Release, String>;
    fn download(&self, url: &str, dest: &std::path::Path) -> Result<(), String>;
    /// Descomprime `.zip` / `.tar.gz` en `dest`.
    fn extract(&self, archive: &std::path::Path, dest: &std::path::Path) -> Result<(), String>;
}

// ─── Salida: CLI ─────────────────────────────────────────────────────────────

/// Todo lo que el CLI necesita de la plataforma.
pub trait Platform: Sync {
    /// Nombre del SO para los mensajes ("Windows", "Linux").
    fn os_name(&self) -> &'static str;
    /// Envía una petición al agente. `give_focus`: cederle el foco (ventanas).
    fn call(&self, req: &Request, timeout: Duration, give_focus: bool) -> Result<Response, ClientError>;
    /// Lanza el agente en segundo plano.
    fn spawn_agent(&self) -> Result<(), String>;
    /// Ejecuta el agente en este proceso (bloquea).
    fn run_agent(&self, dry_run: bool) -> i32;
    fn dialog(&self) -> &dyn DialogPort;
    /// Si el agente murió con el modo privacidad activo, intenta salir de él.
    /// `None` = no había nada que rescatar.
    fn rescue_privacy(&self) -> Option<Result<(), String>>;
    /// ¿El combo está libre en el sistema? `Err` explica quién lo usa.
    fn hotkey_free(&self, combo: &Combo) -> Result<(), String>;
    fn doctor(&self, status: Option<&Value>, cfg: &Config, cfg_error: Option<&str>) -> Vec<Check>;
    /// Dónde instala `install` la skill de Claude Code (si aplica).
    fn skill_path(&self) -> Option<std::path::PathBuf>;
    fn install(&self) -> Result<Vec<String>, String>;
    fn uninstall(&self) -> Result<Vec<String>, String>;
}
