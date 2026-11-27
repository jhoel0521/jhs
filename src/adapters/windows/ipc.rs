//! Transporte del protocolo ([`crate::domain::protocol`]): JSON por línea sobre
//! el named pipe `\\.\pipe\jhs-<sesión>` (solo conexiones locales).

use std::fs::File;
use std::io::{BufRead, BufReader, Read, Write};
use std::os::windows::io::{AsRawHandle, FromRawHandle};
use std::ptr::{null, null_mut};
use std::sync::mpsc;
use std::time::{Duration, Instant};

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

use crate::adapters::windows::win::{self, wide};
pub use crate::domain::protocol::{ClientError, Request, Response};

pub fn session_id() -> u32 {
    let mut sid = 0u32;
    unsafe {
        ProcessIdToSessionId(GetCurrentProcessId(), &mut sid);
    }
    sid
}

pub fn pipe_name() -> String {
    format!(r"\\.\pipe\jhs-{}", session_id())
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
