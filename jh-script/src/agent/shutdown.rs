//! Apagado programado con `shutdown.exe` (nivel sistema: se cumple aunque el
//! agente muera; no requiere administrador).

use std::os::windows::process::CommandExt;
use std::process::Command;

use chrono::{DateTime, Local, TimeZone};

use crate::state::{self, State};
use crate::timeparse::{self, MAX_SECONDS};

const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const COMMENT: &str = "jh-script: apagado programado. Para cancelar: jh-script cancelar-apagado";

pub struct Scheduler {
    deadline: Option<i64>,
    total: i64,
    warned: Vec<u32>,
    pub dry_run: bool,
}

fn now_unix() -> i64 {
    Local::now().timestamp()
}

fn run_shutdown(args: &[&str]) -> Result<(), String> {
    let out = Command::new("shutdown.exe")
        .args(args)
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|e| format!("no se pudo ejecutar shutdown.exe: {e}"))?;
    if out.status.success() {
        Ok(())
    } else {
        let code = out.status.code().unwrap_or(-1);
        let msg = String::from_utf8_lossy(&out.stderr).trim().to_string();
        Err(format!("shutdown {} falló (código {code}) {msg}", args.join(" ")))
    }
}

impl Scheduler {
    /// Recupera un apagado programado anterior (si sigue en el futuro).
    pub fn new(dry_run: bool) -> Scheduler {
        let st = State::load();
        let deadline = st.apagado_unix.filter(|&d| d > now_unix());
        if st.apagado_unix.is_some() && deadline.is_none() {
            State { apagado_unix: None }.save();
        }
        let total = deadline.map(|d| d - now_unix()).unwrap_or(0);
        Scheduler { deadline, total, warned: Vec::new(), dry_run }
    }

    pub fn remaining(&self) -> Option<i64> {
        self.deadline.map(|d| (d - now_unix()).max(0))
    }

    pub fn deadline_local(&self) -> Option<DateTime<Local>> {
        self.deadline.and_then(|d| Local.timestamp_opt(d, 0).single())
    }

    pub fn schedule(&mut self, seconds: u64, force: bool) -> Result<DateTime<Local>, String> {
        if seconds == 0 || seconds > MAX_SECONDS {
            return Err(format!("tiempo fuera de rango (1 s – {MAX_SECONDS} s)"));
        }
        let secs = seconds.to_string();
        let mut args = vec!["/s"];
        if force {
            args.push("/f");
        }
        args.extend(["/t", &secs, "/c", COMMENT]);
        if self.dry_run {
            state::log(&format!("modo prueba: se omitió shutdown {}", args.join(" ")));
        } else {
            // Si ya había uno pendiente, `shutdown /s` falla: se cancela primero.
            let _ = run_shutdown(&["/a"]);
            run_shutdown(&args)?;
        }
        let deadline = now_unix() + seconds as i64;
        self.deadline = Some(deadline);
        self.total = seconds as i64;
        self.warned.clear();
        State { apagado_unix: Some(deadline) }.save();
        state::log(&format!("apagado programado en {}", timeparse::human_duration(seconds as i64)));
        Ok(self.deadline_local().unwrap_or_else(Local::now))
    }

    /// Cancela. Devuelve `true` si había un apagado programado.
    pub fn cancel(&mut self) -> Result<bool, String> {
        let had = self.deadline.is_some();
        let pending_in_windows = if self.dry_run {
            state::log("modo prueba: se omitió shutdown /a");
            false
        } else {
            // 1116 = no había apagado pendiente: no es un error.
            match run_shutdown(&["/a"]) {
                Ok(()) => true,
                Err(e) if e.contains("1116") => false,
                Err(e) => return Err(e),
            }
        };
        self.deadline = None;
        self.warned.clear();
        State { apagado_unix: None }.save();
        if had || pending_in_windows {
            state::log("apagado cancelado");
        }
        Ok(had || pending_in_windows)
    }

    /// Llamar cada segundo. Devuelve un aviso cuando se cruza un umbral.
    pub fn tick(&mut self, avisos_min: &[u32]) -> Option<String> {
        let rem = self.remaining()?;
        if rem == 0 {
            self.deadline = None;
            State { apagado_unix: None }.save();
            return Some(if self.dry_run {
                state::log("modo prueba: aquí se habría apagado el equipo");
                "Modo prueba: aquí se habría apagado el equipo.".into()
            } else {
                "Apagando el equipo…".into()
            });
        }
        for &m in avisos_min {
            let t = m as i64 * 60;
            if self.total > t && rem <= t && !self.warned.contains(&m) {
                self.warned.push(m);
                let unidad = if m == 1 { "minuto" } else { "minutos" };
                return Some(format!("El equipo se apagará en {m} {unidad}. Cancelar: jh-script cancelar-apagado"));
            }
        }
        None
    }
}
