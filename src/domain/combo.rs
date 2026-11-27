//! Combos de teclado ("Ctrl+Shift+Q"): parser y formato, independientes del SO.
//!
//! La tecla principal se guarda como código de tecla virtual (VK) de Windows,
//! que sirve de identificador neutro; cada adaptador lo traduce a lo suyo
//! (Windows: `RegisterHotKey`; GNOME: aceleradores `<Control><Shift>q`).

use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Combo {
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
    pub win: bool,
    /// Código de tecla virtual (VK) de la tecla principal.
    pub vk: u16,
}

const KEY_NAMES: &[(&str, u16)] = &[
    ("Esc", 0x1B),
    ("Space", 0x20),
    ("Enter", 0x0D),
    ("Tab", 0x09),
    ("Backspace", 0x08),
    ("Insert", 0x2D),
    ("Delete", 0x2E),
    ("Home", 0x24),
    ("End", 0x23),
    ("PageUp", 0x21),
    ("PageDown", 0x22),
    ("Left", 0x25),
    ("Up", 0x26),
    ("Right", 0x27),
    ("Down", 0x28),
    ("Pause", 0x13),
    ("ScrollLock", 0x91),
    ("PrintScreen", 0x2C),
];

const KEY_ALIASES: &[(&str, u16)] = &[
    ("escape", 0x1B),
    ("espacio", 0x20),
    ("return", 0x0D),
    ("intro", 0x0D),
    ("ins", 0x2D),
    ("del", 0x2E),
    ("supr", 0x2E),
    ("inicio", 0x24),
    ("fin", 0x23),
    ("pgup", 0x21),
    ("pgdn", 0x22),
    ("pausa", 0x13),
];

fn key_from_name(name: &str) -> Option<u16> {
    let lower = name.to_ascii_lowercase();
    if name.len() == 1 {
        let c = name.chars().next()?.to_ascii_uppercase();
        if c.is_ascii_uppercase() || c.is_ascii_digit() {
            return Some(c as u16);
        }
    }
    if let Some(n) = lower.strip_prefix('f').and_then(|r| r.parse::<u16>().ok())
        && (1..=24).contains(&n)
    {
        return Some(0x70 + n - 1);
    }
    if let Some(n) = lower.strip_prefix("num").and_then(|r| r.parse::<u16>().ok())
        && n <= 9
    {
        return Some(0x60 + n);
    }
    KEY_NAMES
        .iter()
        .chain(KEY_ALIASES)
        .find(|(k, _)| k.eq_ignore_ascii_case(name))
        .map(|&(_, v)| v)
}

fn key_name(vk: u16) -> String {
    match vk {
        0x30..=0x39 | 0x41..=0x5A => (vk as u8 as char).to_string(),
        0x70..=0x87 => format!("F{}", vk - 0x70 + 1),
        0x60..=0x69 => format!("Num{}", vk - 0x60),
        _ => KEY_NAMES
            .iter()
            .find(|&&(_, v)| v == vk)
            .map(|(k, _)| k.to_string())
            .unwrap_or_else(|| format!("VK{vk:#04x}")),
    }
}

impl Combo {
    pub fn parse(s: &str) -> Result<Combo, String> {
        let mut c = Combo { ctrl: false, shift: false, alt: false, win: false, vk: 0 };
        for part in s.split('+').map(str::trim) {
            if part.is_empty() {
                return Err(format!("combo inválido '{s}'"));
            }
            match part.to_ascii_lowercase().as_str() {
                "ctrl" | "control" | "ctl" => c.ctrl = true,
                "shift" | "mayus" | "mayús" => c.shift = true,
                "alt" => c.alt = true,
                "win" | "windows" | "super" | "meta" => c.win = true,
                _ => {
                    if c.vk != 0 {
                        return Err(format!("'{s}' tiene más de una tecla principal"));
                    }
                    c.vk = key_from_name(part).ok_or_else(|| format!("tecla desconocida '{part}'"))?;
                }
            }
        }
        if c.vk == 0 {
            return Err(format!("'{s}' no tiene tecla principal (ej. Ctrl+Shift+Q)"));
        }
        Ok(c)
    }

    /// Nombre canónico de la tecla principal ("Q", "F12", "Delete"…).
    pub fn key_name(&self) -> String {
        key_name(self.vk)
    }

    /// ¿Coincide con el estado de teclas dado (índice = VK, true = pulsada)?
    pub fn matches(&self, vk: u16, down: &[bool; 256]) -> bool {
        let any = |codes: &[usize]| codes.iter().any(|&k| down[k]);
        vk == self.vk
            && any(&[0x11, 0xA2, 0xA3]) == self.ctrl
            && any(&[0x10, 0xA0, 0xA1]) == self.shift
            && any(&[0x12, 0xA4, 0xA5]) == self.alt
            && any(&[0x5B, 0x5C]) == self.win
    }
}

impl fmt::Display for Combo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut parts = Vec::new();
        if self.ctrl {
            parts.push("Ctrl".to_string());
        }
        if self.shift {
            parts.push("Shift".to_string());
        }
        if self.alt {
            parts.push("Alt".to_string());
        }
        if self.win {
            parts.push("Win".to_string());
        }
        parts.push(key_name(self.vk));
        write!(f, "{}", parts.join("+"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_basico() {
        let c = Combo::parse("Ctrl+Shift+Q").unwrap();
        assert!(c.ctrl && c.shift && !c.alt && !c.win);
        assert_eq!(c.vk, b'Q' as u16);
        assert_eq!(c.to_string(), "Ctrl+Shift+Q");
    }

    #[test]
    fn parse_variantes() {
        assert_eq!(Combo::parse("ctrl + alt + f12").unwrap().to_string(), "Ctrl+Alt+F12");
        assert_eq!(Combo::parse("Win+Supr").unwrap().to_string(), "Win+Delete");
        assert_eq!(Combo::parse("Shift+Num5").unwrap().vk, 0x65);
        assert_eq!(Combo::parse("Alt+1").unwrap().vk, b'1' as u16);
    }

    #[test]
    fn parse_errores() {
        assert!(Combo::parse("Ctrl+Shift").is_err());
        assert!(Combo::parse("Ctrl+Q+W").is_err());
        assert!(Combo::parse("Ctrl+Nada").is_err());
        assert!(Combo::parse("Ctrl++Q").is_err());
        assert!(Combo::parse("F25").is_err());
    }

    #[test]
    fn matches_exacto() {
        let c = Combo::parse("Ctrl+Shift+Q").unwrap();
        let mut down = [false; 256];
        down[0xA2] = true; // LCtrl
        down[0xA1] = true; // RShift
        assert!(c.matches(b'Q' as u16, &down));
        down[0xA4] = true; // + Alt → ya no coincide
        assert!(!c.matches(b'Q' as u16, &down));
    }
}
