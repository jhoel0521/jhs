//! jhs: agente ligero con modo privacidad y apagado programado, para
//! Windows y Linux, controlable por CLI y agentes de IA.
//!
//! Arquitectura hexagonal:
//! - [`domain`]: reglas puras (config, combos, horas, apagado, protocolo).
//! - [`ports`]: interfaces de entrada (casos de uso) y salida (SO).
//! - [`application`]: CLI y núcleo del agente, escritos contra los puertos.
//! - [`adapters`]: implementaciones por SO (`windows`, `linux`).
//!
//! Dos binarios comparten este crate:
//! - `jhs`  (consola): CLI para terminal y agentes de IA.
//! - `jhsw` (sin consola en Windows): agente en segundo plano y accesos directos.

#[cfg(not(any(windows, target_os = "linux")))]
compile_error!("jhs soporta Windows y Linux (ver docs/issues/001-soporte-linux.md)");

pub mod adapters;
pub mod application;
pub mod domain;
pub mod ports;

/// Versión del crate (se muestra en `status` y `--version`).
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
