//! Apagado programado con `shutdown.exe` (nivel sistema: se cumple aunque el
//! agente muera; no requiere administrador). La lógica de plazos y avisos vive
//! en [`crate::domain::shutdown`].

use std::os::windows::process::CommandExt;
use std::process::Command;

use crate::ports::PowerPort;

const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const COMMENT: &str = "jhs: apagado programado. Para cancelar: jhs cancelar-apagado";

/// Adaptador de [`PowerPort`] sobre `shutdown.exe`.
pub struct ShutdownExe;

pub type Scheduler = crate::domain::shutdown::Scheduler<ShutdownExe>;

impl Scheduler {
    pub fn new(dry_run: bool) -> Scheduler {
        Scheduler::with_port(ShutdownExe, dry_run)
    }
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

fn schedule_args(seconds: u64, force: bool) -> Vec<String> {
    let mut args = vec!["/s".to_string()];
    if force {
        args.push("/f".into());
    }
    args.extend(["/t".into(), seconds.to_string(), "/c".into(), COMMENT.into()]);
    args
}

impl PowerPort for ShutdownExe {
    fn schedule(&mut self, seconds: u64, force: bool) -> Result<(), String> {
        let args = schedule_args(seconds, force);
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        // Si ya había uno pendiente, `shutdown /s` falla: se cancela primero.
        let _ = run_shutdown(&["/a"]);
        run_shutdown(&args)
    }

    fn cancel(&mut self) -> Result<bool, String> {
        // 1116 = no había apagado pendiente: no es un error.
        match run_shutdown(&["/a"]) {
            Ok(()) => Ok(true),
            Err(e) if e.contains("1116") => Ok(false),
            Err(e) => Err(e),
        }
    }

    fn describe(&self, seconds: u64, force: bool) -> String {
        format!("shutdown {}", schedule_args(seconds, force).join(" "))
    }
}
