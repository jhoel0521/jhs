//! Protocolo CLI ⇄ agente: un objeto JSON por línea. El transporte (named pipe
//! en Windows, socket Unix en Linux) lo pone cada adaptador.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "cmd", rename_all = "snake_case")]
pub enum Request {
    Ping,
    Status,
    PrivacyOn {
        #[serde(default)]
        for_secs: Option<u64>,
    },
    PrivacyOff,
    PrivacyToggle,
    ShutdownSchedule { seconds: u64 },
    ShutdownCancel,
    OpenShutdownWindow,
    ReloadConfig,
    Stop,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Response {
    pub ok: bool,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

impl Response {
    pub fn ok(message: impl Into<String>) -> Self {
        Self { ok: true, message: message.into(), data: None }
    }
    pub fn err(message: impl Into<String>) -> Self {
        Self { ok: false, message: message.into(), data: None }
    }
    pub fn with_data(mut self, data: serde_json::Value) -> Self {
        self.data = Some(data);
        self
    }
}

#[derive(Debug)]
pub enum ClientError {
    /// No hay agente escuchando.
    NotRunning,
    Io(String),
}

impl std::fmt::Display for ClientError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ClientError::NotRunning => write!(f, "el agente de jhs no está corriendo"),
            ClientError::Io(e) => write!(f, "error de comunicación con el agente: {e}"),
        }
    }
}

/// Serializa a JSON escapando todo lo que no sea ASCII (`á`), para que
/// consolas con code pages antiguas (PowerShell 5.1) no rompan los acentos.
pub fn to_ascii_json<T: Serialize>(value: &T, pretty: bool) -> String {
    let s = if pretty {
        serde_json::to_string_pretty(value)
    } else {
        serde_json::to_string(value)
    }
    .unwrap_or_else(|_| "{}".into());
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if c.is_ascii() {
            out.push(c);
        } else {
            let mut buf = [0u16; 2];
            for unit in c.encode_utf16(&mut buf) {
                out.push_str(&format!("\\u{unit:04x}"));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protocolo_roundtrip() {
        let reqs = [
            Request::Ping,
            Request::PrivacyOn { for_secs: Some(30) },
            Request::PrivacyOn { for_secs: None },
            Request::ShutdownSchedule { seconds: 3600 },
            Request::Stop,
        ];
        for r in reqs {
            let s = serde_json::to_string(&r).unwrap();
            assert_eq!(serde_json::from_str::<Request>(&s).unwrap(), r);
        }
        assert_eq!(
            serde_json::from_str::<Request>(r#"{"cmd":"privacy_on"}"#).unwrap(),
            Request::PrivacyOn { for_secs: None }
        );
    }

    #[test]
    fn respuesta_sin_data() {
        let s = serde_json::to_string(&Response::ok("listo")).unwrap();
        assert_eq!(s, r#"{"ok":true,"message":"listo"}"#);
    }

    #[test]
    fn ascii_json_escapa_acentos() {
        let v = serde_json::json!({"m": "Programación ✓"});
        let s = to_ascii_json(&v, false);
        assert!(s.is_ascii());
        let back: serde_json::Value = serde_json::from_str(&s).unwrap();
        assert_eq!(back, v);
    }
}
