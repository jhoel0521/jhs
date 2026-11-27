//! Aplicación: casos de uso. Solo depende del dominio y de los puertos.

pub mod app;
pub mod cli;
pub mod update;
#[cfg(unix)]
pub mod agent;
