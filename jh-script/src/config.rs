//! Configuración en `%APPDATA%\jh-script\config.toml`.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Config {
    pub privacidad: Privacidad,
    pub apagado: Apagado,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Privacidad {
    /// Atajo global para activar el modo privacidad. Vacío = sin atajo.
    pub atajo_activar: String,
    /// Combo que desbloquea estando dentro del modo privacidad.
    pub combo_desbloqueo: String,
    /// Qué hacer si alguien sale del escritorio privado (p. ej. con Ctrl+Alt+Supr).
    pub al_detectar_escape: EscapeAction,
    /// Mantener el monitor encendido (sin ahorro de energía) mientras está activo.
    pub mantener_pantalla_encendida: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum EscapeAction {
    /// Bloquea Windows (pide tu contraseña). Tus programas siguen corriendo.
    BloquearWindows,
    /// Vuelve a mostrar el escritorio privado.
    Volver,
    /// Solo lo registra en el log.
    Nada,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Apagado {
    /// Cierra aplicaciones sin preguntar (`shutdown /f`).
    pub forzar: bool,
    /// Opciones rápidas del menú de la bandeja, en minutos.
    pub presets_min: Vec<u32>,
    /// Avisos antes del apagado, en minutos.
    pub avisos_min: Vec<u32>,
}

impl Default for Privacidad {
    fn default() -> Self {
        Self {
            atajo_activar: "Ctrl+Shift+Q".into(),
            combo_desbloqueo: "Ctrl+Shift+Q".into(),
            al_detectar_escape: EscapeAction::BloquearWindows,
            mantener_pantalla_encendida: true,
        }
    }
}

impl Default for Apagado {
    fn default() -> Self {
        Self { forzar: true, presets_min: vec![15, 30, 60, 90, 120], avisos_min: vec![10, 5, 1] }
    }
}

/// Plantilla con comentarios que se escribe la primera vez.
pub const TEMPLATE: &str = r#"# Configuración de jh-script
# Tras editar: menú de la bandeja > "Recargar configuración" o `jh-script agent reload`.
# Teclas válidas: A-Z, 0-9, F1-F24, Esc, Space, Enter, Tab, Insert, Delete, Home, End,
# PageUp, PageDown, Up, Down, Left, Right, Pause, ScrollLock, PrintScreen, Num0-Num9.
# Modificadores: Ctrl, Shift, Alt, Win.  Ejemplo: "Ctrl+Alt+F12"

[privacidad]
# Atajo global para activar el modo privacidad ("" = sin atajo).
atajo_activar = "Ctrl+Shift+Q"
# Combo secreto que desbloquea estando dentro del modo privacidad.
combo_desbloqueo = "Ctrl+Shift+Q"
# Si alguien sale del modo privacidad (p. ej. Ctrl+Alt+Supr > Administrador de tareas):
#   "bloquear-windows" = bloquea Windows con tu contraseña (recomendado)
#   "volver"           = vuelve a poner la pantalla negra
#   "nada"             = solo lo anota en el log
al_detectar_escape = "bloquear-windows"
# true = el monitor nunca se duerme mientras el modo está activo.
mantener_pantalla_encendida = true

[apagado]
# Cerrar aplicaciones sin preguntar (shutdown /f).
forzar = true
# Opciones rápidas del menú de la bandeja (minutos).
presets_min = [15, 30, 60, 90, 120]
# Avisos antes del apagado (minutos).
avisos_min = [10, 5, 1]
"#;

pub fn config_dir() -> PathBuf {
    std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("jh-script")
}

pub fn config_path() -> PathBuf {
    config_dir().join("config.toml")
}

impl Config {
    /// Carga la configuración. Si el archivo tiene errores, devuelve los valores
    /// por defecto junto al mensaje de error.
    pub fn load() -> (Config, Option<String>) {
        match std::fs::read_to_string(config_path()) {
            Ok(text) => match Self::parse(&text) {
                Ok(c) => (c, None),
                Err(e) => (Config::default(), Some(e)),
            },
            Err(_) => (Config::default(), None),
        }
    }

    pub fn parse(text: &str) -> Result<Config, String> {
        toml::from_str(text).map_err(|e| format!("config.toml inválido: {e}"))
    }

    /// Crea `config.toml` con la plantilla si aún no existe.
    pub fn ensure_file() {
        let p = config_path();
        if !p.exists() {
            let _ = std::fs::create_dir_all(config_dir());
            let _ = std::fs::write(&p, TEMPLATE);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plantilla_equivale_a_default() {
        assert_eq!(Config::parse(TEMPLATE).unwrap(), Config::default());
    }

    #[test]
    fn parcial_usa_defaults() {
        let c = Config::parse("[privacidad]\ncombo_desbloqueo = \"Ctrl+Alt+F12\"\n").unwrap();
        assert_eq!(c.privacidad.combo_desbloqueo, "Ctrl+Alt+F12");
        assert_eq!(c.privacidad.atajo_activar, "Ctrl+Shift+Q");
        assert_eq!(c.apagado, Apagado::default());
    }

    #[test]
    fn escape_kebab() {
        let c = Config::parse("[privacidad]\nal_detectar_escape = \"volver\"\n").unwrap();
        assert_eq!(c.privacidad.al_detectar_escape, EscapeAction::Volver);
        assert!(Config::parse("[privacidad]\nal_detectar_escape = \"x\"\n").is_err());
    }
}
