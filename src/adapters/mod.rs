//! Adaptadores: implementaciones de los puertos para cada sistema operativo.
//!
//! Se elige en compilación con `cfg(target_os)`; dentro de Linux, el escritorio
//! concreto (GNOME u otro) se detecta en tiempo de ejecución.

pub mod github;

#[cfg(windows)]
pub mod windows;

#[cfg(target_os = "linux")]
pub mod linux;

/// Releases del repositorio del programa.
pub fn releases() -> github::GitHub {
    github::GitHub { repo: crate::domain::version::repo_slug() }
}

/// Plataforma del sistema actual (puerto de salida del CLI).
pub fn platform() -> &'static dyn crate::ports::Platform {
    #[cfg(windows)]
    return &windows::PLATFORM;
    #[cfg(target_os = "linux")]
    return &linux::PLATFORM;
}
