//! Notificaciones con `notify-send` y diálogos con `zenity` (si están).

use std::process::{Command, Stdio};

use crate::adapters::linux::sys;
use crate::domain::state;
use crate::ports::{DialogPort, NotifierPort};

pub struct NotifySend;

impl NotifierPort for NotifySend {
    fn notify(&self, title: &str, text: &str, warning: bool) {
        state::log(&format!("aviso: {title}: {text}"));
        if !sys::have("notify-send") {
            return;
        }
        let icon = if warning { "dialog-warning" } else { "dialog-information" };
        let _ = Command::new("notify-send")
            .args(["--app-name=jhs", "-i", icon, title, text])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map(|mut c| std::thread::spawn(move || c.wait()));
    }
}

pub struct Zenity;

impl DialogPort for Zenity {
    fn message(&self, text: &str, title: &str, error: bool) {
        if sys::have("zenity") {
            let kind = if error { "--error" } else { "--info" };
            let ok = Command::new("zenity")
                .args([kind, "--no-markup", "--title", title, "--text", text])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .is_ok();
            if ok {
                return;
            }
        }
        if std::io::IsTerminal::is_terminal(&std::io::stderr()) {
            eprintln!("{title}: {text}");
        } else {
            NotifySend.notify(title, text, error);
        }
    }

    fn ask(&self, title: &str, prompt: &str) -> Option<String> {
        if !sys::have("zenity") {
            NotifySend.notify(title, "Instala zenity para usar esta ventana (sudo apt install zenity)", true);
            return None;
        }
        let out = Command::new("zenity")
            .args(["--entry", "--title", title, "--text", prompt])
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output()
            .ok()?;
        out.status.success().then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
    }
}
