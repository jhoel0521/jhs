//! Interpretación de "cuándo apagar": minutos (`60`) u hora exacta (`23:30`).

use chrono::{Duration, NaiveDateTime, NaiveTime};

/// Máximo que acepta `shutdown.exe /t` (10 años).
pub const MAX_SECONDS: u64 = 315_360_000;

/// Segundos desde `now` hasta la próxima vez que el reloj marque `hhmm`.
/// Si esa hora ya pasó hoy, se toma la de mañana.
pub fn seconds_until(hhmm: &str, now: NaiveDateTime) -> Result<u64, String> {
    let t = NaiveTime::parse_from_str(hhmm.trim(), "%H:%M")
        .map_err(|_| format!("hora inválida '{hhmm}', usa HH:MM (ej. 23:30)"))?;
    let mut target = now.date().and_time(t);
    if target <= now {
        target += Duration::days(1);
    }
    // Redondeo hacia arriba: con "23:59" a las 18:09:44.6 el apagado debe caer
    // a las 23:59:00, no a las 23:58:59.
    let ms = (target - now).num_milliseconds();
    Ok(((ms + 999) / 1000).max(1) as u64)
}

/// Minutos → segundos, validando el rango.
pub fn minutes_to_seconds(minutes: u64) -> Result<u64, String> {
    if minutes == 0 {
        return Err("los minutos deben ser mayores que 0".into());
    }
    let s = minutes.saturating_mul(60);
    if s > MAX_SECONDS {
        return Err(format!("máximo {} minutos", MAX_SECONDS / 60));
    }
    Ok(s)
}

/// Acepta `60` (minutos) o `23:30` (hora).
pub fn parse_when(input: &str, now: NaiveDateTime) -> Result<u64, String> {
    let s = input.trim();
    if s.contains(':') {
        seconds_until(s, now)
    } else {
        let m: u64 = s
            .parse()
            .map_err(|_| format!("'{s}' no es válido: usa minutos (60) u hora (23:30)"))?;
        minutes_to_seconds(m)
    }
}

/// `3725` → `1 h 02 min`, `125` → `2 min 05 s`.
pub fn human_duration(secs: i64) -> String {
    let secs = secs.max(0);
    let (h, m, s) = (secs / 3600, (secs % 3600) / 60, secs % 60);
    if h > 0 {
        format!("{h} h {m:02} min")
    } else if m > 0 {
        format!("{m} min {s:02} s")
    } else {
        format!("{s} s")
    }
}

/// `3725` → `1:02:05`, `125` → `02:05`.
pub fn clock(secs: i64) -> String {
    let secs = secs.max(0);
    let (h, m, s) = (secs / 3600, (secs % 3600) / 60, secs % 60);
    if h > 0 { format!("{h}:{m:02}:{s:02}") } else { format!("{m:02}:{s:02}") }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    fn at(h: u32, m: u32, s: u32) -> NaiveDateTime {
        NaiveDate::from_ymd_opt(2026, 11, 26).unwrap().and_hms_opt(h, m, s).unwrap()
    }

    #[test]
    fn hora_hoy() {
        assert_eq!(seconds_until("23:30", at(22, 0, 0)).unwrap(), 5400);
    }

    #[test]
    fn hora_manana() {
        assert_eq!(seconds_until("23:30", at(23, 45, 0)).unwrap(), 23 * 3600 + 45 * 60);
        assert_eq!(seconds_until("22:00", at(22, 0, 0)).unwrap(), 24 * 3600);
    }

    #[test]
    fn hora_redondea_hacia_arriba() {
        let now = at(18, 9, 44) + Duration::milliseconds(600);
        assert_eq!(seconds_until("18:10", now).unwrap(), 16);
    }

    #[test]
    fn hora_invalida() {
        assert!(seconds_until("24:00", at(1, 0, 0)).is_err());
        assert!(seconds_until("ab", at(1, 0, 0)).is_err());
    }

    #[test]
    fn minutos() {
        assert_eq!(parse_when("60", at(1, 0, 0)).unwrap(), 3600);
        assert_eq!(parse_when(" 7:05 ", at(7, 0, 0)).unwrap(), 300);
        assert!(parse_when("0", at(1, 0, 0)).is_err());
        assert!(parse_when("-5", at(1, 0, 0)).is_err());
        assert!(parse_when("99999999", at(1, 0, 0)).is_err());
    }

    #[test]
    fn formatos() {
        assert_eq!(human_duration(3725), "1 h 02 min");
        assert_eq!(human_duration(125), "2 min 05 s");
        assert_eq!(clock(3725), "1:02:05");
        assert_eq!(clock(125), "02:05");
    }
}
