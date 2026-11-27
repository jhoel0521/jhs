//! Núcleo portable del agente: estado, casos de uso y bucle de eventos.
//!
//! No sabe nada del SO: recibe eventos por un canal (IPC, bandeja, diálogos)
//! y actúa sobre los puertos de salida. Lo usan los adaptadores de Linux; el
//! de Windows mantiene por ahora su bucle Win32 propio.

use std::sync::Arc;
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
use std::time::{Duration, Instant};

use chrono::{DateTime, Local};
use serde_json::{Value, json};

use crate::domain::combo::Combo;
use crate::domain::config::{Config, config_path};
use crate::domain::doctor::{self, Check};
use crate::domain::protocol::{Request, Response};
use crate::domain::shutdown::Scheduler;
use crate::domain::state;
use crate::domain::timeparse;
use crate::ports::{
    AgentService, AutostartPort, DialogPort, HotkeyPort, NotifierPort, PowerPort, PrivacyEnd, PrivacyPort,
    TrayAction, TrayModel, TrayPort,
};

/// Eventos que mueven el agente.
pub enum Event {
    /// Petición del CLI; la respuesta vuelve por el canal.
    Ipc(Request, Sender<Response>),
    /// Clic en el menú de la bandeja.
    Tray(TrayAction),
    /// Resultado de la ventanita de apagado.
    Schedule(u64),
    Cancel,
    /// Aviso que mostrar (p. ej. un error en un diálogo).
    Notify(String, bool),
}

/// Adaptadores que necesita el núcleo.
pub struct Ports {
    pub privacy: Box<dyn PrivacyPort>,
    pub power: Box<dyn PowerPort>,
    pub notifier: Box<dyn NotifierPort>,
    pub hotkey: Box<dyn HotkeyPort>,
    pub tray: Option<Box<dyn TrayPort>>,
    pub autostart: Box<dyn AutostartPort>,
    pub dialog: Arc<dyn DialogPort>,
    /// Abre un archivo con la app predeterminada.
    pub open_file: fn(&std::path::Path),
    pub ram_mb: fn() -> f64,
    pub doctor: fn(Option<&Value>, &Config, Option<&str>) -> Vec<Check>,
}

impl<T: PowerPort + ?Sized> PowerPort for Box<T> {
    fn schedule(&mut self, seconds: u64, force: bool) -> Result<(), String> {
        (**self).schedule(seconds, force)
    }
    fn cancel(&mut self) -> Result<bool, String> {
        (**self).cancel()
    }
    fn pending(&self) -> Option<i64> {
        (**self).pending()
    }
    fn deadline_reached(&mut self) {
        (**self).deadline_reached()
    }
    fn describe(&self, seconds: u64, force: bool) -> String {
        (**self).describe(seconds, force)
    }
}

struct HotkeyStatus {
    combo: Option<Combo>,
    registered: bool,
    error: Option<String>,
}

struct PrivacySession {
    since: DateTime<Local>,
    until: Option<DateTime<Local>>,
}

pub struct Agent {
    privacy_port: Box<dyn PrivacyPort>,
    notifier: Box<dyn NotifierPort>,
    hotkey_port: Box<dyn HotkeyPort>,
    tray: Option<Box<dyn TrayPort>>,
    tray_model: Option<TrayModel>,
    autostart: Box<dyn AutostartPort>,
    dialog: Arc<dyn DialogPort>,
    open_file: fn(&std::path::Path),
    ram_mb: fn() -> f64,
    doctor: fn(Option<&Value>, &Config, Option<&str>) -> Vec<Check>,
    events: Sender<Event>,

    cfg: Config,
    cfg_error: Option<String>,
    hotkey: HotkeyStatus,
    privacy: Option<PrivacySession>,
    last_end: Option<(PrivacyEnd, DateTime<Local>)>,
    shutdown: Scheduler<Box<dyn PowerPort>>,
    started: DateTime<Local>,
    stop: bool,
}

fn fmt_time(t: &DateTime<Local>) -> String {
    t.format("%Y-%m-%d %H:%M:%S").to_string()
}

impl Agent {
    pub fn new(ports: Ports, dry_run: bool, events: Sender<Event>) -> Agent {
        let (cfg, cfg_error) = Config::load();
        Agent {
            privacy_port: ports.privacy,
            notifier: ports.notifier,
            hotkey_port: ports.hotkey,
            tray: ports.tray,
            tray_model: None,
            autostart: ports.autostart,
            dialog: ports.dialog,
            open_file: ports.open_file,
            ram_mb: ports.ram_mb,
            doctor: ports.doctor,
            events,
            cfg,
            cfg_error,
            hotkey: HotkeyStatus { combo: None, registered: false, error: None },
            privacy: None,
            last_end: None,
            shutdown: Scheduler::with_port(ports.power, dry_run),
            started: Local::now(),
            stop: false,
        }
    }

    fn startup(&mut self) {
        self.privacy_port.configure(&self.cfg);
        if let Some(e) = self.cfg_error.clone() {
            self.notifier.notify("jhs: revisa config.toml", &e, true);
        }
        self.register_hotkey();
        self.update_tray();
        state::log(&format!(
            "agente iniciado (v{}, pid {}{})",
            crate::VERSION,
            std::process::id(),
            if self.shutdown.dry_run { ", modo prueba" } else { "" }
        ));
    }

    fn register_hotkey(&mut self) {
        self.hotkey = HotkeyStatus { combo: None, registered: false, error: None };
        let text = self.cfg.privacidad.atajo_activar.trim().to_string();
        if text.is_empty() {
            self.hotkey_port.unbind();
            return;
        }
        match Combo::parse(&text) {
            Err(e) => self.hotkey.error = Some(e),
            Ok(c) => {
                self.hotkey.combo = Some(c);
                match self.hotkey_port.bind(&c) {
                    Ok(()) => self.hotkey.registered = true,
                    Err(why) => {
                        state::log(&format!("no se pudo registrar el atajo {c}: {why}"));
                        self.notifier.notify(
                            "jhs: atajo no disponible",
                            &format!("{c}: {why}. Cambia atajo_activar en config.toml."),
                            true,
                        );
                        self.hotkey.error = Some(why);
                    }
                }
            }
        }
    }

    // ─── Modo privacidad ─────────────────────────────────────────────────────

    fn privacy_on(&mut self, for_secs: Option<u64>) -> Response {
        if self.privacy.is_some() {
            return Response::ok("El modo privacidad ya estaba activo").with_data(self.privacy_json());
        }
        match self.privacy_port.activate(self.cfg.privacidad.mantener_pantalla_encendida) {
            Ok(()) => {
                let since = Local::now();
                let until = for_secs.map(|s| since + chrono::Duration::seconds(s as i64));
                self.privacy = Some(PrivacySession { since, until });
                // Suelta el atajo de activación mientras dura: así el escritorio
                // no se queda con esas teclas y el combo llega a la pantalla de bloqueo.
                self.hotkey_port.unbind();
                state::log("modo privacidad activado");
                self.update_tray();
                let extra = for_secs.map(|s| format!(" Se desactiva solo en {s} s.")).unwrap_or_default();
                Response::ok(format!(
                    "Modo privacidad activado. Desbloqueo: {}.{extra}",
                    self.privacy_port.unlock_hint(&self.cfg)
                ))
                .with_data(self.privacy_json())
            }
            Err(e) => {
                state::log(&format!("no se pudo activar el modo privacidad: {e}"));
                Response::err(format!("No se pudo activar el modo privacidad: {e}"))
            }
        }
    }

    fn privacy_off(&mut self) -> Response {
        if self.privacy.is_none() {
            return Response::ok("El modo privacidad no estaba activo").with_data(self.privacy_json());
        }
        match self.privacy_port.deactivate() {
            Ok(()) => {
                self.end_privacy(PrivacyEnd::Cli);
                Response::ok("Modo privacidad desactivado").with_data(self.privacy_json())
            }
            Err(e) => Response::err(format!("No se pudo desactivar el modo privacidad: {e}")),
        }
    }

    /// Combo secreto desde la pantalla de bloqueo: solo vale si el bloqueo es
    /// de jhs; un bloqueo normal (Super+L) sigue pidiendo la contraseña.
    fn privacy_combo(&mut self) -> Response {
        if self.privacy.is_none() {
            return Response::err("El modo privacidad no está activo: el combo no desbloquea un bloqueo normal");
        }
        match self.privacy_port.deactivate() {
            Ok(()) => {
                self.end_privacy(PrivacyEnd::Combo);
                Response::ok("Modo privacidad desactivado con el combo").with_data(self.privacy_json())
            }
            Err(e) => Response::err(format!("No se pudo desactivar el modo privacidad: {e}")),
        }
    }

    fn end_privacy(&mut self, reason: PrivacyEnd) {
        self.privacy = None;
        self.register_hotkey();
        self.last_end = Some((reason, Local::now()));
        state::log(&format!("modo privacidad terminado: {}", reason.label()));
        if reason == PrivacyEnd::Error {
            self.notifier.notify("jhs", "El modo privacidad terminó por un error (ver agent.log).", true);
        }
        self.update_tray();
    }

    fn privacy_tick(&mut self) {
        let Some(s) = &self.privacy else { return };
        if s.until.is_some_and(|u| Local::now() >= u) {
            match self.privacy_port.deactivate() {
                Ok(()) => self.end_privacy(PrivacyEnd::Timeout),
                Err(e) => state::log(&format!("no se pudo desactivar al terminar el tiempo: {e}")),
            }
            return;
        }
        if let Some(reason) = self.privacy_port.poll() {
            self.end_privacy(reason);
        }
    }

    fn privacy_json(&self) -> Value {
        let s = self.privacy.as_ref();
        json!({
            "activa": s.is_some(),
            "desde": s.map(|s| fmt_time(&s.since)),
            "hasta": s.and_then(|s| s.until.as_ref().map(fmt_time)),
            "mecanismo": self.privacy_port.backend(),
            "combo_desbloqueo": self.privacy_port.unlock_hint(&self.cfg),
            "ultimo_fin": self.last_end.as_ref().map(|(r, t)| json!({
                "motivo": r.key(), "detalle": r.label(), "hora": fmt_time(t)
            })),
        })
    }

    // ─── Apagado ─────────────────────────────────────────────────────────────

    fn schedule(&mut self, seconds: u64) -> Response {
        match self.shutdown.schedule(seconds, self.cfg.apagado.forzar) {
            Ok(at) => {
                self.update_tray();
                let prueba = if self.shutdown.dry_run { " (modo prueba: no se apagará)" } else { "" };
                Response::ok(format!(
                    "Apagado programado a las {} (en {}){prueba}",
                    at.format("%H:%M"),
                    timeparse::human_duration(seconds as i64)
                ))
                .with_data(self.shutdown_json())
            }
            Err(e) => Response::err(format!("No se pudo programar el apagado: {e}")),
        }
    }

    fn cancel(&mut self) -> Response {
        match self.shutdown.cancel() {
            Ok(true) => {
                self.update_tray();
                Response::ok("Apagado cancelado").with_data(self.shutdown_json())
            }
            Ok(false) => Response::ok("No había ningún apagado programado").with_data(self.shutdown_json()),
            Err(e) => Response::err(format!("No se pudo cancelar el apagado: {e}")),
        }
    }

    fn shutdown_json(&self) -> Value {
        json!({
            "programado": self.shutdown.remaining().is_some(),
            "hora": self.shutdown.deadline_local().map(|t| fmt_time(&t)),
            "en_segundos": self.shutdown.remaining(),
            "modo_prueba": self.shutdown.dry_run,
        })
    }

    /// Ventanita de apagado: un diálogo en otro hilo que responde con eventos.
    fn open_shutdown_dialog(&self) {
        let dialog = self.dialog.clone();
        let tx = self.events.clone();
        let prompt = match (self.shutdown.remaining(), self.shutdown.deadline_local()) {
            (Some(rem), Some(at)) => format!(
                "Hay un apagado a las {} (faltan {}).\nEscribe minutos (90) u hora (23:30) para cambiarlo, o «cancelar».",
                at.format("%H:%M"),
                timeparse::clock(rem)
            ),
            _ => "¿Cuándo apagar? Minutos (90) u hora (23:30):".into(),
        };
        std::thread::spawn(move || {
            let Some(text) = dialog.ask("jhs: programar apagado", &prompt) else { return };
            let text = text.trim().to_lowercase();
            let ev = if text.is_empty() {
                return;
            } else if matches!(text.as_str(), "cancelar" | "c" | "0") {
                Event::Cancel
            } else {
                match timeparse::parse_when(&text, Local::now().naive_local()) {
                    Ok(s) => Event::Schedule(s),
                    Err(e) => Event::Notify(e, true),
                }
            };
            let _ = tx.send(ev);
        });
    }

    // ─── Bandeja ─────────────────────────────────────────────────────────────

    fn update_tray(&mut self) {
        if self.tray.is_none() {
            return;
        }
        let mut tip = String::from("jhs");
        if self.privacy.is_some() {
            tip.push_str("\nModo privacidad ACTIVO");
        }
        if let (Some(rem), Some(at)) = (self.shutdown.remaining(), self.shutdown.deadline_local()) {
            tip.push_str(&format!("\nApagado {} (faltan {})", at.format("%H:%M"), timeparse::clock(rem)));
        } else if let Some(c) = self.hotkey.combo.filter(|_| self.hotkey.registered) {
            tip.push_str(&format!("\nModo privacidad: {c}"));
        }
        if self.shutdown.dry_run {
            tip.push_str("\nModo prueba");
        }
        let model = TrayModel {
            tooltip: tip,
            hotkey: self.hotkey.combo.filter(|_| self.hotkey.registered).map(|c| c.to_string()),
            presets_min: self.cfg.apagado.presets_min.clone(),
            remaining: self.shutdown.remaining(),
            autostart: self.autostart.enabled(),
            privacy_active: self.privacy.is_some(),
        };
        if self.tray_model.as_ref() != Some(&model) {
            if let Some(t) = self.tray.as_mut() {
                t.update(&model);
            }
            self.tray_model = Some(model);
        }
    }

    fn notify_response(&self, r: &Response) {
        self.notifier.notify("jhs", &r.message, !r.ok);
    }

    fn on_tray(&mut self, action: TrayAction) {
        match action {
            TrayAction::PrivacyOn => {
                let r = self.privacy_on(None);
                if !r.ok {
                    self.notify_response(&r);
                }
            }
            TrayAction::ScheduleMinutes(m) => {
                let r = self.schedule(m as u64 * 60);
                self.notify_response(&r);
            }
            TrayAction::ScheduleCustom => self.open_shutdown_dialog(),
            TrayAction::CancelShutdown => {
                let r = self.cancel();
                self.notify_response(&r);
            }
            TrayAction::Doctor => {
                let status = self.status_json();
                let text = doctor::render_text(&(self.doctor)(Some(&status), &self.cfg, self.cfg_error.as_deref()));
                let dialog = self.dialog.clone();
                std::thread::spawn(move || dialog.message(&text, "jhs: diagnóstico", false));
            }
            TrayAction::ToggleAutostart => {
                let was = self.autostart.enabled();
                let r = match self.autostart.set(!was) {
                    Ok(()) if was => Response::ok("jhs ya no se iniciará con la sesión"),
                    Ok(()) => Response::ok("jhs se iniciará con la sesión"),
                    Err(e) => Response::err(e),
                };
                self.notify_response(&r);
                self.update_tray();
            }
            TrayAction::OpenConfig => {
                Config::ensure_file();
                (self.open_file)(&config_path());
            }
            TrayAction::ReloadConfig => {
                let r = self.reload_config();
                self.notify_response(&r);
            }
            TrayAction::Exit => self.stop = true,
        }
    }

    // ─── Varios ──────────────────────────────────────────────────────────────

    fn reload_config(&mut self) -> Response {
        let (cfg, err) = Config::load();
        self.cfg = cfg;
        self.cfg_error = err.clone();
        self.privacy_port.configure(&self.cfg);
        self.register_hotkey();
        self.update_tray();
        match err {
            Some(e) => Response::err(e),
            None => Response::ok("Configuración recargada").with_data(self.status_json()),
        }
    }

    fn status_json(&self) -> Value {
        json!({
            "agente": {
                "pid": std::process::id(),
                "version": crate::VERSION,
                "ram_mb": (self.ram_mb)(),
                "desde": fmt_time(&self.started),
                "modo_prueba": self.shutdown.dry_run,
                "so": std::env::consts::OS,
            },
            "privacidad": self.privacy_json(),
            "apagado": self.shutdown_json(),
            "hotkey": {
                "combo": self.hotkey.combo.map(|c| c.to_string()),
                "registrada": self.hotkey.registered,
                "error": self.hotkey.error,
            },
            "config": {
                "ruta": config_path().display().to_string(),
                "error": self.cfg_error,
            },
        })
    }

    fn tick(&mut self) {
        if let Some(msg) = self.shutdown.tick(&self.cfg.apagado.avisos_min.clone()) {
            self.notifier.notify("Apagado programado", &msg, true);
        }
        self.privacy_tick();
        self.update_tray();
    }

    fn teardown(&mut self) {
        if self.privacy.is_some() {
            // Se deja la sesión bloqueada: más seguro que destaparla al salir.
            state::log("agente detenido con el modo privacidad activo (la sesión sigue bloqueada)");
        }
        state::log("agente detenido");
    }

    /// Bucle principal: atiende eventos y hace un "tick" por segundo.
    pub fn run(mut self, rx: Receiver<Event>) -> i32 {
        self.startup();
        let mut next_tick = Instant::now() + Duration::from_secs(1);
        while !self.stop {
            let wait = next_tick.saturating_duration_since(Instant::now());
            match rx.recv_timeout(wait) {
                Ok(Event::Ipc(Request::Stop, tx)) => {
                    let _ = tx.send(Response::ok("Agente detenido"));
                    self.stop = true;
                }
                Ok(Event::Ipc(Request::OpenShutdownWindow, tx)) => {
                    self.open_shutdown_dialog();
                    let _ = tx.send(Response::ok("Ventana de apagado abierta"));
                }
                Ok(Event::Ipc(req, tx)) => {
                    let _ = tx.send(self.handle(req));
                }
                Ok(Event::Tray(a)) => self.on_tray(a),
                Ok(Event::Schedule(s)) => {
                    let r = self.schedule(s);
                    self.notify_response(&r);
                }
                Ok(Event::Cancel) => {
                    let r = self.cancel();
                    self.notify_response(&r);
                }
                Ok(Event::Notify(m, warn)) => self.notifier.notify("jhs", &m, warn),
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => break,
            }
            if Instant::now() >= next_tick {
                self.tick();
                next_tick = Instant::now() + Duration::from_secs(1);
            }
        }
        self.teardown();
        0
    }
}

impl AgentService for Agent {
    fn handle(&mut self, req: Request) -> Response {
        match req {
            Request::Ping => {
                Response::ok("pong").with_data(json!({"pid": std::process::id(), "version": crate::VERSION}))
            }
            Request::Status => Response::ok("estado").with_data(self.status_json()),
            Request::PrivacyOn { for_secs } => self.privacy_on(for_secs),
            Request::PrivacyOff => self.privacy_off(),
            Request::PrivacyToggle => {
                if self.privacy.is_some() {
                    self.privacy_off()
                } else {
                    self.privacy_on(None)
                }
            }
            Request::PrivacyCombo => self.privacy_combo(),
            Request::ShutdownSchedule { seconds } => self.schedule(seconds),
            Request::ShutdownCancel => self.cancel(),
            Request::ReloadConfig => self.reload_config(),
            Request::OpenShutdownWindow | Request::Stop => Response::ok("ok"),
        }
    }
}
