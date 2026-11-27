//! Casos de uso del CLI, comunes a `jhs` (consola) y `jhsw` (sin
//! consola). Todo lo dependiente del SO pasa por el puerto [`Platform`].

use std::ffi::OsString;
use std::time::{Duration, Instant};

use clap::Parser;
use serde_json::{Value, json};

use super::cli::{AgentAction, Cli, Cmd, HotkeyAction, PrivacyAction, ShutdownAction};
use super::update;
use crate::adapters;
use crate::domain::combo::Combo;
use crate::domain::config::Config;
use crate::domain::doctor::{self, Level};
use crate::domain::protocol::{ClientError, Request, Response, to_ascii_json};
use crate::domain::timeparse;
use crate::domain::version::{self, Version};
use crate::ports::Platform;

fn platform() -> &'static dyn Platform {
    adapters::platform()
}

pub const EXIT_OK: i32 = 0;
pub const EXIT_ERROR: i32 = 1;
pub const EXIT_NO_AGENT: i32 = 2;
pub const EXIT_USAGE: i32 = 3;

/// Punto de entrada. `gui = true` para `jhsw.exe`.
pub fn main_entry(gui: bool) -> i32 {
    let args: Vec<OsString> = std::env::args_os().collect();
    // `jhsw.exe` sin argumentos = arrancar el agente.
    if gui && args.len() <= 1 {
        return platform().run_agent(dry_run_env());
    }
    let cli = match Cli::try_parse_from(&args) {
        Ok(c) => c,
        Err(e) => {
            let code = if e.use_stderr() { EXIT_USAGE } else { EXIT_OK };
            if gui {
                platform().dialog().message(&e.to_string(), "jhs", code != EXIT_OK);
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
    std::env::var("JHS_DRY_RUN").is_ok_and(|v| v == "1" || v.eq_ignore_ascii_case("true"))
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
            println!("{}", to_ascii_json(r, false));
        } else {
            let body = text.unwrap_or_else(|| r.message.clone());
            if self.gui {
                if always || !r.ok {
                    platform().dialog().message(&body, "jhs", !r.ok);
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
            msg.push_str(". Inícialo con `jhs agent` (o `jhs install` para que arranque solo).");
        }
        if self.json {
            println!("{}", to_ascii_json(&json!({"ok": false, "message": msg, "error": kind}), false));
        } else if self.gui {
            platform().dialog().message(&msg, "jhs", true);
        } else {
            eprintln!("{msg}");
        }
        code
    }

    fn usage(&self, msg: &str) -> i32 {
        if self.json {
            println!("{}", to_ascii_json(&json!({"ok": false, "message": msg, "error": "argumentos"}), false));
        } else if self.gui {
            platform().dialog().message(msg, "jhs", true);
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
    let attempt = |req: &Request| platform().call(req, Duration::from_secs(15), give_focus);
    match attempt(&req) {
        Err(ClientError::NotRunning) if autostart => {
            platform()
                .spawn_agent()
                .map_err(|e| ClientError::Io(format!("no se pudo iniciar el agente: {e}")))?;
            if !wait_agent(Duration::from_secs(5)) {
                return Err(ClientError::NotRunning);
            }
            attempt(&req)
        }
        other => other,
    }
}

/// ¿Responde el agente a un `ping`?
pub fn agent_running() -> bool {
    matches!(platform().call(&Request::Ping, Duration::from_secs(3), false), Ok(r) if r.ok)
}

/// Espera a que el agente responda (máx. `timeout`).
pub fn wait_agent(timeout: Duration) -> bool {
    let t = Instant::now();
    while t.elapsed() < timeout {
        if agent_running() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(120));
    }
    false
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
            if agent_running() {
                return out.response(&Response::ok("Ya hay un agente de jhs corriendo"), None, true);
            }
            if !out.gui && !out.json {
                println!("Agente de jhs corriendo (cierra esta ventana o usa `jhs agent stop`)…");
            }
            platform().run_agent(dry_run || dry_run_env())
        }
        Cmd::Agent { action: Some(AgentAction::Stop), .. } => out.result(call_agent(Request::Stop, false, false), true),
        Cmd::Agent { action: Some(AgentAction::Reload), .. } => {
            out.result(call_agent(Request::ReloadConfig, false, false), true)
        }

        Cmd::Install => log_result(out, platform().install(), "Instalación completada"),
        Cmd::Uninstall => log_result(out, platform().uninstall(), "Desinstalación completada"),
        Cmd::Update { check } => run_update(out, check),
    }
}

fn schedule(out: &Out, seconds: u64) -> i32 {
    out.result(call_agent(Request::ShutdownSchedule { seconds }, true, false), true)
}

fn privacy_off(out: &Out) -> i32 {
    match call_agent(Request::PrivacyOff, false, false) {
        Ok(r) => out.response(&r, None, false),
        Err(e) => {
            // Rescate: el agente no responde pero el modo privacidad sigue activo.
            if let Some(rescue) = platform().rescue_privacy() {
                let r = match rescue {
                    Ok(()) => Response::ok("Rescate: el agente no respondía; se salió del modo privacidad"),
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
    let mut checks = platform().doctor(status.as_ref(), &cfg, cfg_error.as_deref());
    checks.push(skill_check());
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
        println!("{}", to_ascii_json(&resp, false));
    } else if out.gui {
        platform().dialog().message(&text, "jhs: diagnóstico", false);
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
        (Response::ok(format!("{combo} está en uso por jhs (es el atajo de activación)")), false, Some("jhs"))
    } else {
        match platform().hotkey_free(&combo) {
            Ok(()) => (Response::ok(format!("{combo} está libre en {}", platform().os_name())), true, None),
            Err(why) => (Response::err(format!("{combo} {why}")), false, Some("otra-app")),
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

// ─── Versiones y actualización ───────────────────────────────────────────────

/// Chequeo local (sin red) de la skill instalada frente al programa.
fn skill_check() -> doctor::Check {
    let path = platform().skill_path();
    let installed = update::installed_skill_version(path.clone());
    let current = Version::current();
    let (nivel, detalle) = match (version::compare_skill(installed), installed) {
        ("al-dia", _) => (Level::Ok, format!("v{current}, igual que el programa")),
        ("skill-desactualizada", Some(v)) => (
            Level::Aviso,
            format!("v{v} es anterior al programa (v{current}): `jhs install` o `npx skills update jhs -g -y`"),
        ),
        ("programa-desactualizado", Some(v)) => {
            (Level::Aviso, format!("v{v} es más nueva que el programa (v{current}): ejecuta `jhs update`"))
        }
        _ if path.as_ref().is_some_and(|p| p.exists()) => {
            (Level::Aviso, "instalada sin versión (antigua): `jhs install` la actualiza".to_string())
        }
        _ => (Level::Aviso, "no instalada (`jhs install` o `npx skills add jhoel0521/jhs -g -a claude-code`)".to_string()),
    };
    doctor::check("Skill de IA", nivel, detalle)
}

fn run_update(out: &Out, check_only: bool) -> i32 {
    let port = adapters::releases();
    let skill = update::installed_skill_version(platform().skill_path());
    let release = match crate::ports::ReleasesPort::latest(&port) {
        Ok(r) => r,
        Err(e) => {
            let info = update::version_info(None, skill);
            return out.response(&Response::err(format!("No se pudo buscar actualizaciones: {e}")).with_data(info), None, true);
        }
    };
    let info = update::version_info(Some(&release), skill);
    let current = Version::current();
    if release.version <= current {
        let msg = format!("Ya tienes la última versión (v{current})");
        return out.response(&Response::ok(msg).with_data(info), None, true);
    }
    if check_only {
        let msg = format!("Hay una versión nueva: v{} (tienes v{current}). Instálala con `jhs update`", release.version);
        return out.response(&Response::ok(msg).with_data(info), None, true);
    }
    match update::apply(&port, &release) {
        Ok(mut pasos) => {
            pasos.push("Si instalaste la skill con npx: `npx skills update jhs -g -y`".into());
            let title = format!("Actualizado a v{}", release.version);
            let text = format!("{title}:\n  - {}", pasos.join("\n  - "));
            let resp = Response::ok(title).with_data(json!({ "pasos": pasos, "version": info }));
            out.response(&resp, Some(text), true)
        }
        Err(e) => out.response(&Response::err(e).with_data(info), None, true),
    }
}
