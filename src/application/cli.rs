//! Definición de la línea de comandos (compartida por `jhs` y `jhsw`).

use clap::{Parser, Subcommand};

const EXAMPLES: &str = "\
Ejemplos:
  jhs --off-screen               Activa el modo privacidad (todas las pantallas)
  jhs privacy on --for 60        Modo privacidad por 60 segundos
  jhs --on-screen                Desactiva el modo privacidad
  jhs shutdown in 90             Apaga el PC en 90 minutos
  jhs programar-apagado 23:30    Apaga el PC a las 23:30
  jhs cancelar-apagado           Cancela el apagado programado
  jhs status --json              Estado en JSON (para agentes de IA)
  jhs doctor                     Diagnóstico (atajos libres, autoarranque...)
  jhs hotkey test \"Ctrl+Alt+F12\" ¿Está libre ese atajo?
  jhs update --check             ¿Hay una versión nueva? (update la instala)

Códigos de salida: 0 ok · 1 error · 2 agente no disponible · 3 argumentos inválidos";

#[derive(Debug, Parser)]
#[command(
    name = "jhs",
    version,
    about = "Modo privacidad y apagado programado para Windows y Linux (agente ligero + CLI)",
    after_help = EXAMPLES,
    arg_required_else_help = true
)]
pub struct Cli {
    /// Salida en JSON (para scripts y agentes de IA)
    #[arg(long, global = true)]
    pub json: bool,

    #[command(subcommand)]
    pub cmd: Option<Cmd>,
}

#[derive(Debug, Subcommand)]
pub enum Cmd {
    /// Modo privacidad: pantallas ocultas y teclado/mouse bloqueados
    #[command(visible_alias = "privacidad")]
    Privacy {
        #[command(subcommand)]
        action: PrivacyAction,
    },

    /// Activa el modo privacidad (atajo de `privacy on`)
    #[command(long_flag = "off-screen", visible_aliases = ["apagar-pantalla", "ocultar"])]
    OffScreen {
        /// Desbloquear solo tras N segundos
        #[arg(long = "for", value_name = "SEGUNDOS")]
        for_secs: Option<u64>,
    },

    /// Desactiva el modo privacidad (atajo de `privacy off`)
    #[command(long_flag = "on-screen", visible_aliases = ["encender-pantalla", "mostrar"])]
    OnScreen,

    /// Apagado programado del PC
    #[command(visible_alias = "apagado")]
    Shutdown {
        #[command(subcommand)]
        action: ShutdownAction,
    },

    /// Programa el apagado: minutos (60) u hora exacta (23:30)
    #[command(name = "programar-apagado")]
    ProgramarApagado {
        /// Minutos (60) u hora HH:MM (23:30)
        cuando: String,
    },

    /// Cancela el apagado programado
    #[command(name = "cancelar-apagado")]
    CancelarApagado,

    /// Estado del agente, del modo privacidad y del apagado
    #[command(visible_alias = "estado")]
    Status,

    /// Diagnóstico: agente, atajos libres, autoarranque, PATH...
    #[command(visible_alias = "diagnostico")]
    Doctor,

    /// Atajos de teclado
    Hotkey {
        #[command(subcommand)]
        action: HotkeyAction,
    },

    /// Ejecuta el agente en segundo plano (o lo controla)
    Agent {
        #[command(subcommand)]
        action: Option<AgentAction>,
        /// Modo prueba: no apaga el equipo de verdad (solo lo registra)
        #[arg(long)]
        dry_run: bool,
    },

    /// Instala para el usuario actual (PATH, autoarranque, accesos directos, atajo)
    Install,

    /// Quita autoarranque, PATH, accesos directos y archivos instalados
    Uninstall,

    /// Actualiza a la última versión publicada (binarios, agente y skill)
    #[command(visible_alias = "actualizar")]
    Update {
        /// Solo comprobar si hay una versión nueva (no instala nada)
        #[arg(long)]
        check: bool,
    },
}

#[derive(Debug, Subcommand)]
pub enum PrivacyAction {
    /// Activa el modo privacidad
    #[command(visible_alias = "activar")]
    On {
        /// Desbloquear solo tras N segundos
        #[arg(long = "for", value_name = "SEGUNDOS")]
        for_secs: Option<u64>,
    },
    /// Desactiva el modo privacidad
    #[command(visible_alias = "desactivar")]
    Off,
    /// Alterna activado/desactivado
    #[command(visible_alias = "alternar")]
    Toggle,
}

#[derive(Debug, Subcommand)]
pub enum ShutdownAction {
    /// Apagar dentro de N minutos
    #[command(visible_alias = "en")]
    In {
        /// Minutos hasta el apagado
        minutos: u64,
    },
    /// Apagar a una hora exacta (si ya pasó, mañana)
    #[command(visible_alias = "a")]
    At {
        /// Hora HH:MM (24 h)
        hora: String,
    },
    /// Cancelar el apagado programado
    #[command(visible_alias = "cancelar")]
    Cancel,
    /// Ver si hay un apagado programado
    #[command(visible_alias = "estado")]
    Status,
    /// Abrir la ventanita para programar el apagado
    #[command(visible_alias = "ventana")]
    Ui,
}

#[derive(Debug, Subcommand)]
pub enum HotkeyAction {
    /// Comprueba si un atajo está libre en el sistema
    Test {
        /// Ej. "Ctrl+Shift+Q", "Ctrl+Alt+F12"
        combo: String,
    },
}

#[derive(Debug, Subcommand)]
pub enum AgentAction {
    /// Detiene el agente
    Stop,
    /// Recarga config.toml
    Reload,
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn cli_valida() {
        Cli::command().debug_assert();
    }

    fn parse(args: &[&str]) -> Cmd {
        let mut v = vec!["jhs"];
        v.extend_from_slice(args);
        Cli::try_parse_from(v).unwrap().cmd.unwrap()
    }

    #[test]
    fn alias_estilo_flag() {
        assert!(matches!(parse(&["--off-screen"]), Cmd::OffScreen { for_secs: None }));
        assert!(matches!(parse(&["--on-screen"]), Cmd::OnScreen));
        assert!(matches!(parse(&["apagar-pantalla", "--for", "5"]), Cmd::OffScreen { for_secs: Some(5) }));
    }

    #[test]
    fn alias_espanol() {
        assert!(matches!(
            parse(&["privacidad", "activar"]),
            Cmd::Privacy { action: PrivacyAction::On { .. } }
        ));
        assert!(matches!(
            parse(&["apagado", "en", "30"]),
            Cmd::Shutdown { action: ShutdownAction::In { minutos: 30 } }
        ));
        assert!(matches!(parse(&["programar-apagado", "23:30"]), Cmd::ProgramarApagado { .. }));
        assert!(matches!(parse(&["estado", "--json"]), Cmd::Status));
    }
}
