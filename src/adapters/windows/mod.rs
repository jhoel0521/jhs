//! Adaptadores para Windows (Win32): named pipe, escritorio privado, bandeja,
//! `RegisterHotKey`, `shutdown.exe`, clave Run y accesos directos `.lnk`.
//!
//! El agente de Windows (`agent`) conserva por ahora su propio bucle de
//! mensajes Win32; usa el dominio compartido (config, combos, apagado) y el
//! CLI le habla a través del puerto [`Platform`].

pub mod agent;
pub mod doctor;
pub mod hotkey;
pub mod install;
pub mod ipc;
pub mod win;

use std::time::Duration;

use serde_json::Value;
use windows_sys::Win32::UI::WindowsAndMessaging::AllowSetForegroundWindow;

use crate::domain::combo::Combo;
use crate::domain::config::Config;
use crate::domain::doctor::Check;
use crate::domain::protocol::{ClientError, Request, Response};
use crate::ports::{DialogPort, Platform};

pub struct WinDialog;

impl DialogPort for WinDialog {
    fn message(&self, text: &str, title: &str, error: bool) {
        win::message_box(text, title, error);
    }
    fn ask(&self, _title: &str, _prompt: &str) -> Option<String> {
        None
    }
}

pub struct Windows;

pub static PLATFORM: Windows = Windows;

impl Platform for Windows {
    fn os_name(&self) -> &'static str {
        "Windows"
    }

    fn call(&self, req: &Request, timeout: Duration, give_focus: bool) -> Result<Response, ClientError> {
        ipc::call_with(req, timeout, |pid| {
            if give_focus && let Some(pid) = pid {
                unsafe { AllowSetForegroundWindow(pid) };
            }
        })
    }

    fn spawn_agent(&self) -> Result<(), String> {
        install::spawn_agent(&install::windowless_exe()).map_err(|e| e.to_string())
    }

    fn run_agent(&self, dry_run: bool) -> i32 {
        agent::run(dry_run)
    }

    fn dialog(&self) -> &dyn DialogPort {
        &WinDialog
    }

    fn rescue_privacy(&self) -> Option<Result<(), String>> {
        // El agente no responde pero la pantalla sigue en el escritorio privado.
        agent::privacy::private_desktop_visible().then(agent::privacy::rescue_to_default)
    }

    fn hotkey_free(&self, combo: &Combo) -> Result<(), String> {
        hotkey::test_free(combo).map_err(hotkey::explain_error)
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
