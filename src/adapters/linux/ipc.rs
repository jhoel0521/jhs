//! Transporte del protocolo sobre un socket Unix en
//! `$XDG_RUNTIME_DIR/jhs.sock` (carpeta privada del usuario, 0600).

use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::mpsc::{self, Sender};
use std::time::Duration;

use crate::application::agent::Event;
use crate::domain::paths::runtime_dir;
use crate::domain::protocol::{ClientError, Request, Response};
use crate::domain::state;

pub fn socket_path() -> PathBuf {
    runtime_dir().join("jhs.sock")
}

pub fn lock_path() -> PathBuf {
    runtime_dir().join("jhs.lock")
}

/// Crea el socket. Si queda uno viejo de un agente muerto, lo reemplaza.
pub fn bind() -> Result<UnixListener, String> {
    let path = socket_path();
    if path.exists() {
        if UnixStream::connect(&path).is_ok() {
            return Err("ya hay un agente escuchando".into());
        }
        let _ = std::fs::remove_file(&path);
    }
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let l = UnixListener::bind(&path).map_err(|e| format!("no se pudo crear {}: {e}", path.display()))?;
    let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
    Ok(l)
}

/// Atiende clientes para siempre: cada petición se entrega al bucle del
/// agente como [`Event::Ipc`] y se espera su respuesta.
pub fn serve(listener: UnixListener, events: Sender<Event>) {
    for conn in listener.incoming() {
        let Ok(stream) = conn else { continue };
        let events = events.clone();
        std::thread::spawn(move || handle_client(stream, events));
    }
}

fn handle_client(stream: UnixStream, events: Sender<Event>) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let mut line = String::new();
    if BufReader::new(&stream).read_line(&mut line).is_err() {
        return;
    }
    let resp = match serde_json::from_str::<Request>(line.trim()) {
        Err(e) => Response::err(format!("petición inválida: {e}")),
        Ok(req) => {
            let (tx, rx) = mpsc::channel();
            if events.send(Event::Ipc(req, tx)).is_err() {
                Response::err("el agente se está cerrando")
            } else {
                rx.recv_timeout(Duration::from_secs(12))
                    .unwrap_or_else(|_| Response::err("el agente no respondió a tiempo"))
            }
        }
    };
    let mut out = serde_json::to_string(&resp).unwrap_or_else(|_| "{}".into());
    out.push('\n');
    let mut w = &stream;
    if let Err(e) = w.write_all(out.as_bytes()) {
        state::log(&format!("no se pudo responder al cliente: {e}"));
    }
}

/// Envía una petición y espera la respuesta (máx. `timeout`).
pub fn call(req: &Request, timeout: Duration) -> Result<Response, ClientError> {
    let stream = match UnixStream::connect(socket_path()) {
        Ok(s) => s,
        Err(e) => {
            use std::io::ErrorKind::*;
            return Err(match e.kind() {
                NotFound | ConnectionRefused => ClientError::NotRunning,
                _ => ClientError::Io(e.to_string()),
            });
        }
    };
    let _ = stream.set_read_timeout(Some(timeout));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(3)));
    let mut msg = serde_json::to_string(req).map_err(|e| ClientError::Io(e.to_string()))?;
    msg.push('\n');
    let mut w = &stream;
    w.write_all(msg.as_bytes()).map_err(|e| ClientError::Io(e.to_string()))?;
    let mut line = String::new();
    match BufReader::new(&stream).read_line(&mut line) {
        Ok(_) if !line.trim().is_empty() => {
            serde_json::from_str(line.trim()).map_err(|e| ClientError::Io(format!("respuesta inválida: {e}")))
        }
        Ok(_) => Err(ClientError::Io("el agente cerró la conexión sin responder".into())),
        Err(e) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => {
            Err(ClientError::Io("el agente no respondió a tiempo".into()))
        }
        Err(e) => Err(ClientError::Io(e.to_string())),
    }
}
