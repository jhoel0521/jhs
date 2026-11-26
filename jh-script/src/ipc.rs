//! Protocolo CLI ⇄ agente: JSON por línea sobre el named pipe
//! `\\.\pipe\jh-script-<sesión>` (solo conexiones locales).

use std::fs::File;
use std::io::{BufRead, BufReader, Read, Write};
use std::os::windows::io::{AsRawHandle, FromRawHandle};
use std::ptr::{null, null_mut};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use windows_sys::Win32::Foundation::{
    CloseHandle, ERROR_PIPE_BUSY, ERROR_PIPE_CONNECTED, INVALID_HANDLE_VALUE,
};
use windows_sys::Win32::Storage::FileSystem::{FILE_FLAG_FIRST_PIPE_INSTANCE, PIPE_ACCESS_DUPLEX};
use windows_sys::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, GetNamedPipeServerProcessId, PIPE_READMODE_BYTE,
    PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE, PIPE_UNLIMITED_INSTANCES, PIPE_WAIT, WaitNamedPipeW,
};
use windows_sys::Win32::System::RemoteDesktop::ProcessIdToSessionId;
use windows_sys::Win32::System::Threading::GetCurrentProcessId;

use crate::win::{self, wide};

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
    /// No hay agente escuchando en el pipe.
    NotRunning,
    Io(String),
}

impl std::fmt::Display for ClientError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ClientError::NotRunning => write!(f, "el agente de jh-script no está corriendo"),
            ClientError::Io(e) => write!(f, "error de comunicación con el agente: {e}"),
        }
    }
}

pub fn session_id() -> u32 {
    let mut sid = 0u32;
    unsafe {
        ProcessIdToSessionId(GetCurrentProcessId(), &mut sid);
    }
    sid
}

pub fn pipe_name() -> String {
    format!(r"\\.\pipe\jh-script-{}", session_id())
}

// ─── Servidor (agente) ───────────────────────────────────────────────────────

/// Atiende clientes para siempre (bloquea el hilo actual). Cada cliente se
/// procesa en su propio hilo con `handler`.
pub fn serve<F>(handler: F) -> Result<(), String>
where
    F: Fn(Request) -> Response + Send + Sync + 'static,
{
    let handler = std::sync::Arc::new(handler);
    let name = wide(&pipe_name());
    let mut first = true;
    loop {
        let open_mode = PIPE_ACCESS_DUPLEX | if first { FILE_FLAG_FIRST_PIPE_INSTANCE } else { 0 };
        let h = unsafe {
            CreateNamedPipeW(
                name.as_ptr(),
                open_mode,
                PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
                PIPE_UNLIMITED_INSTANCES,
                64 * 1024,
                64 * 1024,
                0,
                null(),
            )
        };
        if h == INVALID_HANDLE_VALUE {
            let e = win::last_error();
            if first {
                return Err(format!("no se pudo crear el pipe: {}", win::os_error(e)));
            }
            std::thread::sleep(Duration::from_millis(200));
            continue;
        }
        first = false;
        let connected = unsafe { ConnectNamedPipe(h, null_mut()) } != 0
            || win::last_error() == ERROR_PIPE_CONNECTED;
        if !connected {
            unsafe { CloseHandle(h) };
            continue;
        }
        let file = unsafe { File::from_raw_handle(h as _) };
        let handler = handler.clone();
        std::thread::spawn(move || handle_client(file, &*handler));
    }
}

fn handle_client(file: File, handler: &(dyn Fn(Request) -> Response + Send + Sync)) {
    let mut line = String::new();
    {
        let mut reader = BufReader::new(&file).take(64 * 1024);
        if reader.read_line(&mut line).is_err() {
            return;
        }
    }
    let resp = match serde_json::from_str::<Request>(line.trim()) {
        Ok(req) => handler(req),
        Err(e) => Response::err(format!("petición inválida: {e}")),
    };
    let mut out = serde_json::to_string(&resp).unwrap_or_else(|_| "{}".into());
    out.push('\n');
    let mut w = &file;
    let _ = w.write_all(out.as_bytes());
    // Espera a que el cliente lea todo antes de cerrar.
    let _ = file.sync_all();
}

// ─── Cliente (CLI) ───────────────────────────────────────────────────────────

fn open_pipe(timeout: Duration) -> Result<File, ClientError> {
    let name = pipe_name();
    let wname = wide(&name);
    let start = Instant::now();
    loop {
        match std::fs::OpenOptions::new().read(true).write(true).open(&name) {
            Ok(f) => return Ok(f),
            Err(e) => match e.raw_os_error().map(|c| c as u32) {
                Some(2) | Some(3) => return Err(ClientError::NotRunning),
                Some(ERROR_PIPE_BUSY) if start.elapsed() < timeout => unsafe {
                    WaitNamedPipeW(wname.as_ptr(), 1000);
                },
                _ => return Err(ClientError::Io(e.to_string())),
            },
        }
    }
}

/// PID del agente al otro lado del pipe.
pub fn server_pid(file: &File) -> Option<u32> {
    let mut pid = 0u32;
    let ok = unsafe { GetNamedPipeServerProcessId(file.as_raw_handle() as _, &mut pid) };
    (ok != 0).then_some(pid)
}

/// Envía una petición y espera la respuesta (máx. `timeout`).
/// `before_send` recibe el PID del agente (p. ej. para cederle el foco).
pub fn call_with(
    req: &Request,
    timeout: Duration,
    before_send: impl FnOnce(Option<u32>),
) -> Result<Response, ClientError> {
    let file = open_pipe(Duration::from_secs(3))?;
    before_send(server_pid(&file));
    let mut msg = serde_json::to_string(req).map_err(|e| ClientError::Io(e.to_string()))?;
    msg.push('\n');
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let r = (|| {
            let mut w = &file;
            w.write_all(msg.as_bytes())?;
            let mut line = String::new();
            BufReader::new(&file).read_line(&mut line)?;
            Ok::<_, std::io::Error>(line)
        })();
        let _ = tx.send(r);
    });
    match rx.recv_timeout(timeout) {
        Ok(Ok(line)) if !line.trim().is_empty() => serde_json::from_str(line.trim())
            .map_err(|e| ClientError::Io(format!("respuesta inválida: {e}"))),
        Ok(Ok(_)) => Err(ClientError::Io("el agente cerró la conexión sin responder".into())),
        Ok(Err(e)) => Err(ClientError::Io(e.to_string())),
        Err(_) => Err(ClientError::Io("el agente no respondió a tiempo".into())),
    }
}

pub fn call(req: &Request) -> Result<Response, ClientError> {
    call_with(req, Duration::from_secs(15), |_| {})
}

pub fn agent_running() -> bool {
    matches!(call_with(&Request::Ping, Duration::from_secs(3), |_| {}), Ok(r) if r.ok)
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
}
