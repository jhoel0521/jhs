//! Modelo del diagnóstico (`jhs doctor`). Los chequeos concretos los
//! aporta cada plataforma; aquí solo vive el formato común.

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Level {
    Ok,
    Aviso,
    Error,
}

#[derive(Debug, Clone, Serialize)]
pub struct Check {
    pub nombre: String,
    pub nivel: Level,
    pub detalle: String,
}

pub fn check(nombre: &str, nivel: Level, detalle: impl Into<String>) -> Check {
    Check { nombre: nombre.into(), nivel, detalle: detalle.into() }
}

/// Chequeo del agente a partir del JSON de `Request::Status` (si respondió).
pub fn agent_check(status: Option<&serde_json::Value>) -> Check {
    match status {
        Some(s) => {
            let a = &s["agente"];
            let prueba = if a["modo_prueba"].as_bool() == Some(true) { ", MODO PRUEBA" } else { "" };
            check(
                "Agente",
                Level::Ok,
                format!(
                    "activo: pid {}, {} MB de RAM, desde {}{prueba}",
                    a["pid"],
                    a["ram_mb"],
                    a["desde"].as_str().unwrap_or("?")
                ),
            )
        }
        None => check("Agente", Level::Error, "no está corriendo (inícialo con `jhs agent` o `jhs install`)"),
    }
}

pub fn render_text(checks: &[Check]) -> String {
    checks
        .iter()
        .map(|c| {
            let mark = match c.nivel {
                Level::Ok => "✔",
                Level::Aviso => "!",
                Level::Error => "✖",
            };
            format!("{mark} {}: {}", c.nombre, c.detalle)
        })
        .collect::<Vec<_>>()
        .join("\n")
}
