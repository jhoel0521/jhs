//! Lógica del apagado programado (plazo, avisos, persistencia y modo prueba),
//! independiente de cómo se apaga el equipo ([`PowerPort`]).

use chrono::{DateTime, Local, TimeZone};

use super::state::{self, State};
use super::timeparse::{self, MAX_SECONDS};
use crate::ports::PowerPort;

pub struct Scheduler<P: PowerPort> {
    port: P,
    deadline: Option<i64>,
    total: i64,
    warned: Vec<u32>,
    pub dry_run: bool,
}

fn now_unix() -> i64 {
    Local::now().timestamp()
}

impl<P: PowerPort> Scheduler<P> {
    /// Recupera un apagado programado anterior (si sigue en el futuro). Si el
    /// sistema informa de uno pendiente, manda el sistema.
    pub fn with_port(port: P, dry_run: bool) -> Scheduler<P> {
        let st = State::load();
        let now = now_unix();
        let deadline = port.pending().or(st.apagado_unix).filter(|&d| d > now);
        if st.apagado_unix != deadline {
            State { apagado_unix: deadline }.save();
        }
        let total = deadline.map(|d| d - now).unwrap_or(0);
        Scheduler { port, deadline, total, warned: Vec::new(), dry_run }
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
        if self.dry_run {
            state::log(&format!("modo prueba: se omitió {}", self.port.describe(seconds, force)));
        } else {
            self.port.schedule(seconds, force)?;
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
        let pending_in_system = if self.dry_run {
            state::log("modo prueba: se omitió la cancelación del apagado");
            false
        } else {
            self.port.cancel()?
        };
        self.deadline = None;
        self.warned.clear();
        State { apagado_unix: None }.save();
        if had || pending_in_system {
            state::log("apagado cancelado");
        }
        Ok(had || pending_in_system)
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
                self.port.deadline_reached();
                "Apagando el equipo…".into()
            });
        }
        for &m in avisos_min {
            let t = m as i64 * 60;
            if self.total > t && rem <= t && !self.warned.contains(&m) {
                self.warned.push(m);
                let unidad = if m == 1 { "minuto" } else { "minutos" };
                return Some(format!("El equipo se apagará en {m} {unidad}. Cancelar: jhs cancelar-apagado"));
            }
        }
        None
    }
}
