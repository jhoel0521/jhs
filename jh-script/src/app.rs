//! Lógica común de `jh-script.exe` (consola) y `jh-scriptw.exe` (sin consola).

use std::ffi::OsString;
use std::time::Duration;

use clap::Parser;
use serde_json::{Value, json};
use windows_sys::Win32::UI::WindowsAndMessaging::AllowSetForegroundWindow;

use crate::agent::{self, privacy};
use crate::cli::{AgentAction, Cli, Cmd, HotkeyAction, PrivacyAction, ShutdownAction};
use crate::config::Config;
use crate::doctor::{self, Level};
use crate::hotkey::{self, Combo};
use crate::install;
use crate::ipc::{self, ClientError, Request, Response};
use crate::timeparse;
use crate::win;

pub const EXIT_OK: i32 = 0;
pub const EXIT_ERROR: i32 = 1;
pub const EXIT_NO_AGENT: i32 = 2;
pub const EXIT_USAGE: i32 = 3;

/// Punto de entrada. `gui = true` para `jh-scriptw.exe`.
pub fn main_entry(gui: bool) -> i32 {
    let args: Vec<OsString> = std::env::args_os().collect();
    // `jh-scriptw.exe` sin argumentos = arrancar el agente.
    if gui && args.len() <= 1 {
        return agent::run(dry_run_env());
    }
    let cli = match Cli::try_parse_from(&args) {
        Ok(c) => c,
        Err(e) => {
            let code = if e.use_stderr() { EXIT_USAGE } else { EXIT_OK };
            if gui {
                win::message_box(&e.to_string(), "jh-script", code != EXIT_OK);
            } else {
                let _ = e.print();
            }
            return code;
        }
    };
    let out = Out { json: cli.json, gui };
    match cli.cmd {
        Some(cmd) => run(cmd, &out),
        None => EXIT_OK,
    }
}

fn dry_run_env() -> bool {
    std::env::var("JH_SCRIPT_DRY_RUN").is_ok_and(|v| v == "1" || v.eq_ignore_ascii_case("true"))
}

// ─── Salida ──────────────────────────────────────────────────────────────────

struct Out {
    json: bool,
    gui: bool,
}

impl Out {
    /// Muestra una respuesta del agente. En modo ventana solo se muestran
    /// errores, salvo que `always` sea true.
    fn response(&self, r: &Response, text: Option<String>, always: bool) -> i32 {
        let code = if r.ok { EXIT_OK } else { EXIT_ERROR };
        if self.json {
            println!("{}", win::to_ascii_json(r, false));
        } else {
            let body = text.unwrap_or_else(|| r.message.clone());
            if self.gui {
                if always || !r.ok {
                    win::message_box(&body, "jh-script", !r.ok);
                }
            } else if r.ok {
                println!("{body}");
            } else {
                eprintln!("{body}");
            }
        }
        code
    }

    fn client_error(&self, e: &ClientError) -> i32 {
        let (code, kind) = match e {
            ClientError::NotRunning => (EXIT_NO_AGENT, "agente_no_disponible"),
            ClientError::Io(_) => (EXIT_ERROR, "comunicacion"),
        };
        let mut msg = e.to_string();
        if matches!(e, ClientError::NotRunning) {
            msg.push_str(". Inícialo con `jh-script agent` (o `jh-script install` para que arranque solo).");
        }
        if self.json {
            println!("{}", win::to_ascii_json(&json!({"ok": false, "message": msg, "error": kind}), false));
        } else if self.gui {
            win::message_box(&msg, "jh-script", true);
        } else {
            eprintln!("{msg}");
        }
        code
    }

    fn usage(&self, msg: &str) -> i32 {
        if self.json {
            println!("{}", win::to_ascii_json(&json!({"ok": false, "message": msg, "error": "argumentos"}), false));
        } else if self.gui {
            win::message_box(msg, "jh-script", true);
        } else {
            eprintln!("{msg}");
        }
        EXIT_USAGE
    }

    fn result(&self, r: Result<Response, ClientError>, always: bool) -> i32 {
        match r {
            Ok(r) => self.response(&r, None, always),
            Err(e) => self.client_error(&e),
        }
    }
}

// ─── Llamadas al agente ──────────────────────────────────────────────────────

/// Envía `req` al agente. Con `autostart`, lo lanza si no está corriendo.
fn call_agent(req: Request, autostart: bool, give_focus: bool) -> Result<Response, ClientError> {
    let attempt = |req: &Request| {
        ipc::call_with(req, Duration::from_secs(15), |pid| {
            if give_focus && let Some(pid) = pid {
                unsafe { AllowSetForegroundWindow(pid) };
            }
        })
    };
    match attempt(&req) {
        Err(ClientError::NotRunning) if autostart => {
            install::spawn_agent(&install::windowless_exe())
                .map_err(|e| ClientError::Io(format!("no se pudo iniciar el agente: {e}")))?;
            if !install::wait_agent(Duration::from_secs(5)) {
                return Err(ClientError::NotRunning);
            }
            attempt(&req)
        }
        other => other,
    }
}

fn now_naive() -> chrono::NaiveDateTime {
    chrono::Local::now().naive_local()
}

// ─── Comandos ────────────────────────────────────────────────────────────────

fn run(cmd: Cmd, out: &Out) -> i32 {
    match cmd {
        Cmd::Privacy { action: PrivacyAction::On { for_secs } } | Cmd::OffScreen { for_secs } => {
            out.result(call_agent(Request::PrivacyOn { for_secs }, true, false), false)
        }
        Cmd::Privacy { action: PrivacyAction::Off } | Cmd::OnScreen => privacy_off(out),
        Cmd::Privacy { action: PrivacyAction::Toggle } => {
            out.result(call_agent(Request::PrivacyToggle, true, false), false)
        }

        Cmd::Shutdown { action: ShutdownAction::In { minutos } } => match timeparse::minutes_to_seconds(minutos) {
            Ok(s) => schedule(out, s),
            Err(e) => out.usage(&e),
        },
        Cmd::Shutdown { action: ShutdownAction::At { hora } } => match timeparse::seconds_until(&hora, now_naive()) {
            Ok(s) => schedule(out, s),
            Err(e) => out.usage(&e),
        },
        Cmd::ProgramarApagado { cuando } => match timeparse::parse_when(&cuando, now_naive()) {
            Ok(s) => schedule(out, s),
            Err(e) => out.usage(&e),
        },
        Cmd::Shutdown { action: ShutdownAction::Cancel } | Cmd::CancelarApagado => {
            out.result(call_agent(Request::ShutdownCancel, true, false), true)
        }
        Cmd::Shutdown { action: ShutdownAction::Status } => match call_agent(Request::Status, false, false) {
            Ok(r) => {
                let data = r.data.clone().unwrap_or(Value::Null);
                let s = &data["apagado"];
                let resp = Response::ok(shutdown_line(s)).with_data(s.clone());
                out.response(&resp, None, true)
            }
            Err(e) => out.client_error(&e),
        },
        Cmd::Shutdown { action: ShutdownAction::Ui } => {
            out.result(call_agent(Request::OpenShutdownWindow, true, true), false)
        }

        Cmd::Status => match call_agent(Request::Status, false, false) {
            Ok(r) => {
                let text = r.data.as_ref().map(status_text);
                out.response(&r, text, true)
            }
            Err(e) => out.client_error(&e),
        },
        Cmd::Doctor => run_doctor(out),
        Cmd::Hotkey { action: HotkeyAction::Test { combo } } => hotkey_test(out, &combo),

        Cmd::Agent { action: None, dry_run } => {
            if ipc::agent_running() {
                return out.response(&Response::ok("Ya hay un agente de jh-script corriendo"), None, true);
            }
            if !out.gui && !out.json {
                println!("Agente de jh-script corriendo (cierra esta ventana o usa `jh-script agent stop`)…");
            }
            agent::run(dry_run || dry_run_env())
        }
        Cmd::Agent { action: Some(AgentAction::Stop), .. } => out.result(call_agent(Request::Stop, false, false), true),
        Cmd::Agent { action: Some(AgentAction::Reload), .. } => {
            out.result(call_agent(Request::ReloadConfig, false, false), true)
        }

        Cmd::Install => log_result(out, install::install(), "Instalación completada"),
        Cmd::Uninstall => log_result(out, install::uninstall(), "Desinstalación completada"),
    }
}

fn schedule(out: &Out, seconds: u64) -> i32 {
    out.result(call_agent(Request::ShutdownSchedule { seconds }, true, false), true)
}

fn privacy_off(out: &Out) -> i32 {
    match call_agent(Request::PrivacyOff, false, false) {
        Ok(r) => out.response(&r, None, false),
        Err(e) => {
            // Rescate: el agente no responde pero la pantalla sigue en el escritorio privado.
            if privacy::private_desktop_visible() {
                let r = match privacy::rescue_to_default() {
                    Ok(()) => Response::ok("Rescate: el agente no respondía; se volvió al escritorio normal"),
                    Err(err) => Response::err(format!("El agente no responde y el rescate falló: {err}")),
                };
                out.response(&r, None, true)
            } else if matches!(e, ClientError::NotRunning) {
                out.response(&Response::ok("El modo privacidad no estaba activo (el agente no está corriendo)"), None, false)
            } else {
                out.client_error(&e)
            }
        }
    }
}

fn log_result(out: &Out, r: Result<Vec<String>, String>, title: &str) -> i32 {
    match r {
        Ok(lines) => {
            let resp = Response::ok(title).with_data(json!({ "pasos": lines }));
            let text = format!("{title}:\n  - {}", lines.join("\n  - "));
            out.response(&resp, Some(text), true)
        }
        Err(e) => out.response(&Response::err(e), None, true),
    }
}

fn run_doctor(out: &Out) -> i32 {
    let status = call_agent(Request::Status, false, false).ok().and_then(|r| r.data);
    let (cfg, cfg_error) = Config::load();
    let checks = doctor::collect(status.as_ref(), &cfg, cfg_error.as_deref());
    let errors = checks.iter().filter(|c| c.nivel == Level::Error).count();
    let msg = if errors == 0 {
        "Diagnóstico sin errores".to_string()
    } else {
        format!("Diagnóstico con {errors} error(es)")
    };
    let resp = Response { ok: errors == 0, message: msg, data: Some(json!({ "chequeos": checks })) };
    let text = doctor::render_text(&checks);
    // El diagnóstico se muestra siempre, aunque tenga errores.
    if out.json {
        println!("{}", win::to_ascii_json(&resp, false));
    } else if out.gui {
        win::message_box(&text, "jh-script: diagnóstico", false);
    } else {
        println!("{text}");
    }
    if errors == 0 { EXIT_OK } else { EXIT_ERROR }
}

fn hotkey_test(out: &Out, text: &str) -> i32 {
    let combo = match Combo::parse(text) {
        Ok(c) => c,
        Err(e) => return out.usage(&e),
    };
    let status = call_agent(Request::Status, false, false).ok().and_then(|r| r.data);
    let ours = status.as_ref().is_some_and(|s| {
        s["hotkey"]["registrada"].as_bool() == Some(true) && s["hotkey"]["combo"].as_str() == Some(&combo.to_string())
    });
    let (resp, libre, usado_por) = if ours {
        (Response::ok(format!("{combo} está en uso por jh-script (es el atajo de activación)")), false, Some("jh-script"))
    } else {
        match hotkey::test_free(&combo) {
            Ok(()) => (Response::ok(format!("{combo} está libre en Windows")), true, None),
            Err(code) => (Response::err(format!("{combo} {}", hotkey::explain_error(code))), false, Some("otra-app")),
        }
    };
    let resp = resp.with_data(json!({ "combo": combo.to_string(), "libre": libre, "usado_por": usado_por }));
    out.response(&resp, None, true)
}

// ─── Texto de estado ─────────────────────────────────────────────────────────

fn short_time(v: &Value) -> String {
    let s = v.as_str().unwrap_or("?");
    s.get(11..).unwrap_or(s).to_string()
}

fn shutdown_line(s: &Value) -> String {
    if s["programado"].as_bool() == Some(true) {
        let secs = s["en_segundos"].as_i64().unwrap_or(0);
        let prueba = if s["modo_prueba"].as_bool() == Some(true) { " [modo prueba]" } else { "" };
        format!(
            "programado a las {} (faltan {}){prueba}",
            short_time(&s["hora"]).get(..5).unwrap_or("?"),
            timeparse::human_duration(secs)
        )
    } else {
        "no programado".into()
    }
}

fn status_text(d: &Value) -> String {
    let a = &d["agente"];
    let p = &d["privacidad"];
    let h = &d["hotkey"];
    let prueba = if a["modo_prueba"].as_bool() == Some(true) { " [MODO PRUEBA]" } else { "" };
    let mut lines = vec![format!(
        "Agente:      activo (pid {}, {} MB, desde {}){prueba}",
        a["pid"],
        a["ram_mb"],
        short_time(&a["desde"])
    )];
    let privacidad = if p["activa"].as_bool() == Some(true) {
        let hasta = if p["hasta"].is_string() { format!(", hasta {}", short_time(&p["hasta"])) } else { String::new() };
        format!("ACTIVA desde {}{hasta}", short_time(&p["desde"]))
    } else {
        "inactiva".into()
    };
    lines.push(format!("Privacidad:  {privacidad}"));
    if let Some(fin) = p["ultimo_fin"].as_object() {
        lines.push(format!(
            "             último fin: {} a las {}",
            fin.get("detalle").and_then(Value::as_str).unwrap_or("?"),
            short_time(fin.get("hora").unwrap_or(&Value::Null))
        ));
    }
    lines.push(format!("Desbloqueo:  {}", p["combo_desbloqueo"].as_str().unwrap_or("?")));
    lines.push(format!("Apagado:     {}", shutdown_line(&d["apagado"])));
    let atajo = match (h["combo"].as_str(), h["registrada"].as_bool()) {
        (Some(c), Some(true)) => format!("{c} (registrado)"),
        (Some(c), _) => format!("{c} NO registrado: {}", h["error"].as_str().unwrap_or("?")),
        (None, _) => match h["error"].as_str() {
            Some(e) => format!("inválido: {e}"),
            None => "desactivado".into(),
        },
    };
    lines.push(format!("Atajo:       {atajo}"));
    if let Some(e) = d["config"]["error"].as_str() {
        lines.push(format!("Config:      ERROR {e}"));
    }
    lines.join("\n")
}
