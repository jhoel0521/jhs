//! jh-script: agente ligero para Windows con modo privacidad y apagado programado.
//!
//! Dos binarios comparten este crate:
//! - `jh-script.exe`  (consola): CLI para terminal y agentes de IA.
//! - `jh-scriptw.exe` (sin consola): agente en segundo plano y accesos directos.

#[cfg(not(windows))]
compile_error!("jh-script por ahora solo soporta Windows (ver docs/issues/001-soporte-linux.md)");

pub mod agent;
pub mod app;
pub mod cli;
pub mod config;
pub mod doctor;
pub mod hotkey;
pub mod install;
pub mod ipc;
pub mod state;
pub mod timeparse;
pub mod win;

/// Versión del crate (se muestra en `status` y `--version`).
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
