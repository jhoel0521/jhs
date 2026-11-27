//! Apagado con logind (`org.freedesktop.login1.Manager.ScheduleShutdown`) vía
//! `busctl`: lo cumple el sistema aunque el agente muera, sin root (polkit lo
//! permite al usuario de la sesión activa).
//!
//! Si polkit no lo permite (p. ej. hay otros usuarios conectados), se usa un
//! temporizador interno y `systemctl poweroff` al llegar la hora.

use crate::adapters::linux::sys;
use crate::domain::state;
use crate::ports::PowerPort;

const DEST: &str = "org.freedesktop.login1";
const PATH: &str = "/org/freedesktop/login1";
const IFACE: &str = "org.freedesktop.login1.Manager";

#[derive(Default)]
pub struct Logind {
    /// El apagado actual lo lleva el agente (logind no lo aceptó).
    internal: Option<bool>,
}

fn busctl(args: &[&str]) -> Result<String, String> {
    let mut v = vec!["--allow-interactive-authorization=no", "call", DEST, PATH, IFACE];
    v.extend_from_slice(args);
    sys::run("busctl", &v)
}

/// `(st) "poweroff" 1700000000000000` → segundos UNIX.
fn parse_scheduled(out: &str) -> Option<i64> {
    let mut it = out.split_whitespace();
    let _sig = it.next()?;
    let kind = it.next()?.trim_matches('"');
    let usec: i64 = it.next()?.parse().ok()?;
    (!kind.is_empty() && usec > 0).then_some(usec / 1_000_000)
}

impl PowerPort for Logind {
    fn schedule(&mut self, seconds: u64, force: bool) -> Result<(), String> {
        let at_usec = (chrono::Local::now().timestamp() as u64 + seconds) * 1_000_000;
        match busctl(&["ScheduleShutdown", "st", "poweroff", &at_usec.to_string()]) {
            Ok(_) => {
                self.internal = None;
                Ok(())
            }
            Err(e) => {
                state::log(&format!("logind rechazó ScheduleShutdown ({e}); se usará el temporizador del agente"));
                if !sys::have("systemctl") {
                    return Err(format!("logind no permitió programar el apagado: {e}"));
                }
                self.internal = Some(force);
                Ok(())
            }
        }
    }

    fn cancel(&mut self) -> Result<bool, String> {
        let internal = self.internal.take().is_some();
        let pending = self.pending().is_some();
        if pending {
            busctl(&["CancelScheduledShutdown"])?;
        }
        Ok(internal || pending)
    }

    fn pending(&self) -> Option<i64> {
        sys::run("busctl", &["get-property", DEST, PATH, IFACE, "ScheduledShutdown"])
            .ok()
            .and_then(|o| parse_scheduled(&o))
    }

    fn deadline_reached(&mut self) {
        let Some(force) = self.internal.take() else { return };
        let mut args = vec!["poweroff"];
        if force {
            args.push("--check-inhibitors=no");
        }
        if let Err(e) = sys::run("systemctl", &args) {
            state::log(&format!("no se pudo apagar: {e}"));
        }
    }

    fn describe(&self, seconds: u64, _force: bool) -> String {
        format!("logind ScheduleShutdown poweroff en {seconds} s")
    }
}

#[cfg(test)]
mod tests {
    use super::parse_scheduled;

    #[test]
    fn parsea_scheduled_shutdown() {
        assert_eq!(parse_scheduled(r#"(st) "poweroff" 1700000000123456"#), Some(1_700_000_000));
        assert_eq!(parse_scheduled(r#"(st) "" 0"#), None);
        assert_eq!(parse_scheduled(""), None);
    }
}
