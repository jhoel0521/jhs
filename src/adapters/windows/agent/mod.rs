//! Agente en segundo plano: una ventana oculta con su bucle de mensajes, el
//! ícono de la bandeja, el atajo global y el servidor IPC para el CLI.
//!
//! Todo el estado vive en el hilo principal (`AGENT`); el hilo del pipe le
//! pasa las peticiones con `PostMessageW` y espera la respuesta por un canal.

pub mod privacy;
pub mod shutdown;
pub mod timer_window;
pub mod tray;

use std::cell::RefCell;
use std::ffi::c_void;
use std::mem::{size_of, zeroed};
use std::ptr::{null, null_mut};
use std::sync::atomic::{AtomicPtr, AtomicU32, Ordering};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use chrono::{DateTime, Local};
use serde_json::{Value, json};
use windows_sys::Win32::Foundation::{
    CloseHandle, ERROR_ALREADY_EXISTS, HWND, LPARAM, LRESULT, POINT, WPARAM,
};
use windows_sys::Win32::System::ProcessStatus::{K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS};
use windows_sys::Win32::System::Shutdown::{ShutdownBlockReasonCreate, ShutdownBlockReasonDestroy};
use windows_sys::Win32::System::Threading::{CreateMutexW, GetCurrentProcess};
use windows_sys::Win32::UI::Controls::{ICC_STANDARD_CLASSES, INITCOMMONCONTROLSEX, InitCommonControlsEx};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyMenu, DestroyWindow,
    DispatchMessageW, GetCursorPos, GetMessageW, IsDialogMessageW, KillTimer, MF_CHECKED, MF_GRAYED,
    MF_POPUP, MF_SEPARATOR, MF_STRING, MSG, MessageBoxW, MB_ICONINFORMATION, MB_OK, PostMessageW,
    PostQuitMessage, RegisterClassExW, RegisterWindowMessageW, SetForegroundWindow,
    SetMenuDefaultItem, SetTimer, TPM_NONOTIFY, TPM_RETURNCMD, TPM_RIGHTBUTTON, TrackPopupMenu,
    TranslateMessage, WM_APP, WM_CLOSE, WM_DESTROY, WM_ENDSESSION, WM_HOTKEY, WM_LBUTTONDBLCLK,
    WM_NULL, WM_QUERYENDSESSION, WM_RBUTTONUP, WM_TIMER, WNDCLASSEXW, WS_EX_TOOLWINDOW,
    WS_OVERLAPPED,
};

use crate::domain::config::{Config, config_path};
use crate::adapters::windows::hotkey;
use crate::domain::combo::Combo;
use crate::adapters::windows::ipc;
use crate::domain::protocol::{Request, Response};
use crate::domain::state;
use crate::domain::timeparse;
use crate::adapters::windows::win::{self, wide};

pub const WM_APP_IPC: u32 = WM_APP + 1;
pub const WM_APP_TRAY: u32 = WM_APP + 2;
pub const WM_APP_PRIVACY_ENDED: u32 = WM_APP + 3;

const CLASS: &str = "JhsAgent";
const HOTKEY_ID: i32 = 1;
const TIMER_TICK: usize = 1;

const ID_PRIVACY: usize = 1001;
const ID_CANCEL: usize = 1002;
const ID_DOCTOR: usize = 1003;
const ID_AUTOSTART: usize = 1004;
const ID_CONFIG: usize = 1005;
const ID_RELOAD: usize = 1006;
const ID_EXIT: usize = 1009;
const ID_CUSTOM: usize = 1099;
const ID_PRESET: usize = 1100;

static MAIN_HWND: AtomicPtr<c_void> = AtomicPtr::new(null_mut());
static TASKBAR_CREATED: AtomicU32 = AtomicU32::new(0);

thread_local! {
    static AGENT: RefCell<Option<Agent>> = const { RefCell::new(None) };
}

/// Ejecuta `f` con el agente (hilo principal). `None` si no existe o si ya
/// está en uso (reentrada desde un bucle modal): nunca entra en pánico.
pub(crate) fn with_agent<R>(f: impl FnOnce(&mut Agent) -> R) -> Option<R> {
    AGENT.with(|cell| match cell.try_borrow_mut() {
        Ok(mut g) => g.as_mut().map(f),
        Err(_) => {
            state::log("aviso: reentrada en el agente, mensaje ignorado");
            None
        }
    })
}

struct Job {
    req: Request,
    tx: mpsc::Sender<Response>,
}

struct HotkeyStatus {
    combo: Option<Combo>,
    registered: bool,
    error: Option<String>,
}

pub struct Agent {
    hwnd: HWND,
    cfg: Config,
    cfg_error: Option<String>,
    tray: tray::Tray,
    hotkey: HotkeyStatus,
    privacy: Option<privacy::Session>,
    last_end: Option<(privacy::EndReason, DateTime<Local>)>,
    ended_at: Option<Instant>,
    shutdown: shutdown::Scheduler,
    started: DateTime<Local>,
    tick_running: bool,
    blocking_end_session: bool,
}

fn fmt_time(t: &DateTime<Local>) -> String {
    t.format("%Y-%m-%d %H:%M:%S").to_string()
}

fn ram_mb() -> f64 {
    let mut pmc: PROCESS_MEMORY_COUNTERS = unsafe { zeroed() };
    pmc.cb = size_of::<PROCESS_MEMORY_COUNTERS>() as u32;
    let ok = unsafe { K32GetProcessMemoryInfo(GetCurrentProcess(), &mut pmc, pmc.cb) };
    if ok == 0 {
        return 0.0;
    }
    (pmc.WorkingSetSize as f64 / 1_048_576.0 * 10.0).round() / 10.0
}

impl Agent {
    fn new(hwnd: HWND, dry_run: bool) -> Agent {
        let (cfg, cfg_error) = Config::load();
        Agent {
            hwnd,
            cfg,
            cfg_error,
            tray: tray::Tray::new(hwnd),
            hotkey: HotkeyStatus { combo: None, registered: false, error: None },
            privacy: None,
            last_end: None,
            ended_at: None,
            shutdown: shutdown::Scheduler::new(dry_run),
            started: Local::now(),
            tick_running: false,
            blocking_end_session: false,
        }
    }

    fn startup(&mut self) {
        self.tray.add();
        if let Some(e) = self.cfg_error.clone() {
            self.tray.balloon("jhs: revisa config.toml", &e, true);
        }
        self.register_hotkey();
        if self.shutdown.remaining().is_some() {
            self.ensure_tick();
        }
        self.update_tray();
        state::log(&format!(
            "agente iniciado (v{}, pid {}{})",
            crate::VERSION,
            std::process::id(),
            if self.shutdown.dry_run { ", modo prueba" } else { "" }
        ));
    }

    fn register_hotkey(&mut self) {
        hotkey::unregister(self.hwnd, HOTKEY_ID);
        self.hotkey = HotkeyStatus { combo: None, registered: false, error: None };
        let text = self.cfg.privacidad.atajo_activar.trim().to_string();
        if text.is_empty() {
            return;
        }
        match Combo::parse(&text) {
            Err(e) => self.hotkey.error = Some(e),
            Ok(c) => {
                self.hotkey.combo = Some(c);
                match hotkey::register(self.hwnd, HOTKEY_ID, &c) {
                    Ok(()) => self.hotkey.registered = true,
                    Err(code) => {
                        let why = hotkey::explain_error(code);
                        self.hotkey.error = Some(why.clone());
                        state::log(&format!("no se pudo registrar el atajo {c}: {why}"));
                        self.tray.balloon(
                            "jhs: atajo no disponible",
                            &format!("{c} {why}. Cambia atajo_activar en config.toml."),
                            true,
                        );
                    }
                }
            }
        }
    }

    fn unlock_combo(&self) -> Combo {
        Combo::parse(&self.cfg.privacidad.combo_desbloqueo)
            .unwrap_or_else(|_| Combo::parse("Ctrl+Shift+Q").expect("combo por defecto válido"))
    }

    fn handle(&mut self, req: Request) -> Response {
        match req {
            Request::Ping => Response::ok("pong")
                .with_data(json!({"pid": std::process::id(), "version": crate::VERSION})),
            Request::Status => Response::ok("estado").with_data(self.status_json()),
            Request::PrivacyOn { for_secs } => self.privacy_on(for_secs),
            Request::PrivacyOff => self.privacy_off(),
            Request::PrivacyToggle => {
                if self.privacy.is_some() { self.privacy_off() } else { self.privacy_on(None) }
            }
            Request::ShutdownSchedule { seconds } => self.schedule(seconds),
            Request::ShutdownCancel => self.cancel(),
            Request::ReloadConfig => self.reload_config(),
            Request::OpenShutdownWindow | Request::Stop => Response::ok("ok"),
        }
    }

    // ─── Modo privacidad ─────────────────────────────────────────────────────

    fn reap_privacy(&mut self) {
        if self.privacy.as_ref().is_some_and(|s| s.is_finished()) {
            self.privacy = None;
        }
    }

    fn privacy_on(&mut self, for_secs: Option<u64>) -> Response {
        self.reap_privacy();
        if self.privacy.is_some() {
            return Response::ok("El modo privacidad ya estaba activo").with_data(self.privacy_json());
        }
        let unlock = self.unlock_combo();
        let params = privacy::Params {
            unlock,
            escape: self.cfg.privacidad.al_detectar_escape,
            keep_display: self.cfg.privacidad.mantener_pantalla_encendida,
            for_secs,
            notify: self.hwnd,
        };
        match privacy::start(params) {
            Ok(s) => {
                self.privacy = Some(s);
                let extra = for_secs.map(|s| format!(" Se desactiva solo en {s} s.")).unwrap_or_default();
                Response::ok(format!(
                    "Modo privacidad activado. Desbloqueo: {unlock} o `jhs privacy off`.{extra}"
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
        match self.privacy.take() {
            Some(s) => {
                s.stop(privacy::EndReason::Cli);
                self.record_end(privacy::EndReason::Cli);
                Response::ok("Modo privacidad desactivado").with_data(self.privacy_json())
            }
            None => Response::ok("El modo privacidad no estaba activo").with_data(self.privacy_json()),
        }
    }

    fn record_end(&mut self, reason: privacy::EndReason) {
        self.last_end = Some((reason, Local::now()));
        self.ended_at = Some(Instant::now());
        if self.blocking_end_session {
            unsafe { ShutdownBlockReasonDestroy(self.hwnd) };
            self.blocking_end_session = false;
        }
    }

    fn on_privacy_ended(&mut self, code: usize, id: u32) {
        if self.privacy.as_ref().map(|s| s.id) != Some(id) {
            return; // ya se gestionó (p. ej. `privacy off`)
        }
        let reason = privacy::EndReason::from_code(code);
        drop(self.privacy.take());
        self.record_end(reason);
        match reason {
            privacy::EndReason::Escape => self.tray.balloon(
                "jhs: intento de salida",
                &format!(
                    "{}: alguien salió del modo privacidad y se bloqueó Windows.",
                    Local::now().format("%H:%M")
                ),
                true,
            ),
            privacy::EndReason::Error => {
                self.tray.balloon("jhs", "El modo privacidad terminó por un error (ver agent.log).", true)
            }
            _ => {}
        }
    }

    fn on_hotkey(&mut self) {
        // Evita reactivar justo después de desbloquear con el mismo combo.
        if self.ended_at.is_some_and(|t| t.elapsed() < Duration::from_millis(1500)) {
            return;
        }
        let r = if self.privacy.is_some() { self.privacy_off() } else { self.privacy_on(None) };
        if !r.ok {
            self.tray.balloon("jhs", &r.message, true);
        }
    }

    fn privacy_json(&self) -> Value {
        let s = self.privacy.as_ref();
        json!({
            "activa": s.is_some(),
            "desde": s.map(|s| fmt_time(&s.since)),
            "hasta": s.and_then(|s| s.until.as_ref().map(fmt_time)),
            "combo_desbloqueo": self.unlock_combo().to_string(),
            "al_detectar_escape": self.cfg.privacidad.al_detectar_escape,
            "ultimo_fin": self.last_end.as_ref().map(|(r, t)| json!({
                "motivo": r.key(), "detalle": r.label(), "hora": fmt_time(t)
            })),
        })
    }

    // ─── Apagado ─────────────────────────────────────────────────────────────

    fn schedule(&mut self, seconds: u64) -> Response {
        match self.shutdown.schedule(seconds, self.cfg.apagado.forzar) {
            Ok(at) => {
                self.ensure_tick();
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

    fn ensure_tick(&mut self) {
        if !self.tick_running {
            unsafe { SetTimer(self.hwnd, TIMER_TICK, 1000, None) };
            self.tick_running = true;
        }
    }

    fn tick(&mut self) {
        if let Some(msg) = self.shutdown.tick(&self.cfg.apagado.avisos_min.clone()) {
            self.tray.balloon("Apagado programado", &msg, true);
        }
        self.update_tray();
        if self.shutdown.remaining().is_none() && timer_window::current().is_null() && self.tick_running {
            unsafe { KillTimer(self.hwnd, TIMER_TICK) };
            self.tick_running = false;
        }
    }

    fn update_tray(&mut self) {
        let mut tip = String::from("jhs");
        if let (Some(rem), Some(at)) = (self.shutdown.remaining(), self.shutdown.deadline_local()) {
            tip.push_str(&format!("\nApagado {} (faltan {})", at.format("%H:%M"), timeparse::clock(rem)));
        } else if let Some(c) = &self.hotkey.combo {
            tip.push_str(&format!("\nModo privacidad: {c}"));
        }
        if self.shutdown.dry_run {
            tip.push_str("\nModo prueba");
        }
        self.tray.set_tip(&tip);
    }

    // ─── Varios ──────────────────────────────────────────────────────────────

    fn reload_config(&mut self) -> Response {
        let (cfg, err) = Config::load();
        self.cfg = cfg;
        self.cfg_error = err.clone();
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
                "ram_mb": ram_mb(),
                "desde": fmt_time(&self.started),
                "modo_prueba": self.shutdown.dry_run,
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

    fn query_end_session(&mut self) -> bool {
        self.reap_privacy();
        // Con el modo privacidad activo se frena el cierre de sesión, salvo el
        // apagado que tú mismo programaste.
        if self.privacy.is_some() && self.shutdown.remaining().is_none() {
            let reason = wide("jhs: modo privacidad activo");
            unsafe { ShutdownBlockReasonCreate(self.hwnd, reason.as_ptr()) };
            self.blocking_end_session = true;
            state::log("se bloqueó un intento de cerrar sesión/apagar (modo privacidad activo)");
            return false;
        }
        true
    }

    fn teardown(&mut self) {
        if let Some(s) = self.privacy.take() {
            s.stop(privacy::EndReason::Cli);
        }
        hotkey::unregister(self.hwnd, HOTKEY_ID);
        self.tray.remove();
        state::log("agente detenido");
    }
}

// ─── Funciones usadas por la ventanita ───────────────────────────────────────

pub(crate) fn ui_schedule(seconds: u64) -> Response {
    with_agent(|a| a.schedule(seconds)).unwrap_or_else(|| Response::err("agente ocupado"))
}

pub(crate) fn ui_cancel() -> Response {
    with_agent(|a| a.cancel()).unwrap_or_else(|| Response::err("agente ocupado"))
}

pub(crate) fn ui_shutdown_info() -> Option<(i64, DateTime<Local>, bool)> {
    with_agent(|a| {
        let rem = a.shutdown.remaining()?;
        Some((rem, a.shutdown.deadline_local()?, a.shutdown.dry_run))
    })
    .flatten()
}

pub(crate) fn ui_window_opened() {
    with_agent(|a| a.ensure_tick());
}

// ─── Arranque y bucle de mensajes ────────────────────────────────────────────

fn ipc_handler(req: Request) -> Response {
    let hwnd = MAIN_HWND.load(Ordering::Acquire);
    let (tx, rx) = mpsc::channel();
    let job = Box::into_raw(Box::new(Job { req, tx }));
    if unsafe { PostMessageW(hwnd, WM_APP_IPC, 0, job as LPARAM) } == 0 {
        drop(unsafe { Box::from_raw(job) });
        return Response::err("el agente se está cerrando");
    }
    rx.recv_timeout(Duration::from_secs(12))
        .unwrap_or_else(|_| Response::err("el agente no respondió a tiempo"))
}

/// Ejecuta el agente (bloquea hasta que se detiene). Devuelve el código de salida.
pub fn run(dry_run: bool) -> i32 {
    let mutex_name = wide("Local\\jhs-agent");
    let mutex = unsafe { CreateMutexW(null(), 0, mutex_name.as_ptr()) };
    if mutex.is_null() || win::last_error() == ERROR_ALREADY_EXISTS {
        if !mutex.is_null() {
            unsafe { CloseHandle(mutex) };
        }
        return 0; // ya hay un agente corriendo
    }

    unsafe {
        let icc = INITCOMMONCONTROLSEX {
            dwSize: size_of::<INITCOMMONCONTROLSEX>() as u32,
            dwICC: ICC_STANDARD_CLASSES,
        };
        InitCommonControlsEx(&icc);
    }
    Config::ensure_file();

    let class = wide(CLASS);
    let wc = WNDCLASSEXW {
        cbSize: size_of::<WNDCLASSEXW>() as u32,
        lpfnWndProc: Some(wndproc),
        hInstance: win::hinstance(),
        lpszClassName: class.as_ptr(),
        ..unsafe { zeroed() }
    };
    unsafe { RegisterClassExW(&wc) };
    privacy::register_class();
    timer_window::register_class();

    let title = wide("jhs agente");
    let hwnd = unsafe {
        CreateWindowExW(
            WS_EX_TOOLWINDOW,
            class.as_ptr(),
            title.as_ptr(),
            WS_OVERLAPPED,
            0,
            0,
            0,
            0,
            null_mut(),
            null_mut(),
            win::hinstance(),
            null(),
        )
    };
    if hwnd.is_null() {
        state::log(&format!("no se pudo crear la ventana del agente: {}", win::os_error(win::last_error())));
        return 1;
    }
    MAIN_HWND.store(hwnd, Ordering::Release);
    TASKBAR_CREATED.store(unsafe { RegisterWindowMessageW(wide("TaskbarCreated").as_ptr()) }, Ordering::Relaxed);

    AGENT.with(|c| *c.borrow_mut() = Some(Agent::new(hwnd, dry_run)));
    with_agent(|a| a.startup());

    std::thread::Builder::new()
        .name("ipc".into())
        .spawn(move || {
            if let Err(e) = ipc::serve(ipc_handler) {
                state::log(&format!("servidor IPC detenido: {e}"));
                unsafe { PostMessageW(MAIN_HWND.load(Ordering::Acquire), WM_CLOSE, 0, 0) };
            }
        })
        .ok();

    let mut msg: MSG = unsafe { zeroed() };
    loop {
        let r = unsafe { GetMessageW(&mut msg, null_mut(), 0, 0) };
        if r == 0 || r == -1 {
            break;
        }
        let tw = timer_window::current();
        if !tw.is_null() && unsafe { IsDialogMessageW(tw, &msg) } != 0 {
            continue;
        }
        unsafe {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }

    with_agent(|a| a.teardown());
    AGENT.with(|c| c.borrow_mut().take());
    unsafe { CloseHandle(mutex) };
    0
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    unsafe {
        match msg {
            WM_APP_IPC => {
                let Job { req, tx } = *Box::from_raw(lp as *mut Job);
                let resp = match req {
                    // Crear ventanas fuera del préstamo del agente (WM_CREATE lo usa).
                    Request::OpenShutdownWindow => {
                        timer_window::open();
                        Response::ok("Ventana de apagado abierta")
                    }
                    Request::Stop => {
                        PostMessageW(hwnd, WM_CLOSE, 0, 0);
                        Response::ok("Agente detenido")
                    }
                    other => with_agent(|a| a.handle(other))
                        .unwrap_or_else(|| Response::err("agente ocupado, reintenta")),
                };
                let _ = tx.send(resp);
                0
            }
            WM_APP_TRAY => {
                match (lp as u32) & 0xFFFF {
                    WM_LBUTTONDBLCLK => {
                        if let Some(r) = with_agent(|a| a.privacy_on(None))
                            && !r.ok
                        {
                            with_agent(|a| a.tray.balloon("jhs", &r.message, true));
                        }
                    }
                    WM_RBUTTONUP => show_menu(hwnd),
                    _ => {}
                }
                0
            }
            WM_HOTKEY => {
                if wp as i32 == HOTKEY_ID {
                    with_agent(|a| a.on_hotkey());
                }
                0
            }
            WM_TIMER => {
                if wp == TIMER_TICK {
                    with_agent(|a| a.tick());
                    timer_window::refresh();
                }
                0
            }
            WM_APP_PRIVACY_ENDED => {
                with_agent(|a| a.on_privacy_ended(wp, lp as u32));
                0
            }
            WM_QUERYENDSESSION => with_agent(|a| a.query_end_session()).unwrap_or(true) as LRESULT,
            WM_ENDSESSION => {
                if wp != 0 {
                    with_agent(|a| a.teardown());
                }
                0
            }
            WM_DESTROY => {
                PostQuitMessage(0);
                0
            }
            m if m != 0 && m == TASKBAR_CREATED.load(Ordering::Relaxed) => {
                with_agent(|a| {
                    a.tray.readd();
                    a.tray.set_tip("jhs");
                    a.update_tray();
                });
                0
            }
            _ => DefWindowProcW(hwnd, msg, wp, lp),
        }
    }
}

// ─── Menú de la bandeja ──────────────────────────────────────────────────────

struct MenuInfo {
    hotkey: Option<String>,
    presets: Vec<u32>,
    remaining: Option<i64>,
    autostart: bool,
}

fn show_menu(hwnd: HWND) {
    let Some(info) = with_agent(|a| MenuInfo {
        hotkey: a.hotkey.combo.filter(|_| a.hotkey.registered).map(|c| c.to_string()),
        presets: a.cfg.apagado.presets_min.clone(),
        remaining: a.shutdown.remaining(),
        autostart: crate::adapters::windows::install::autostart_enabled(),
    }) else {
        return;
    };
    let add = |menu, flags, id: usize, text: &str| unsafe {
        AppendMenuW(menu, flags, id, wide(text).as_ptr());
    };
    unsafe {
        let menu = CreatePopupMenu();
        let sub = CreatePopupMenu();
        for (i, m) in info.presets.iter().enumerate() {
            add(sub, MF_STRING, ID_PRESET + i, &format!("En {m} min"));
        }
        AppendMenuW(sub, MF_SEPARATOR, 0, null());
        add(sub, MF_STRING, ID_CUSTOM, "Personalizado…");

        let privacy_label = match &info.hotkey {
            Some(h) => format!("Activar modo privacidad\t{h}"),
            None => "Activar modo privacidad".into(),
        };
        add(menu, MF_STRING, ID_PRIVACY, &privacy_label);
        SetMenuDefaultItem(menu, ID_PRIVACY as u32, 0);
        AppendMenuW(menu, MF_SEPARATOR, 0, null());
        add(menu, MF_POPUP, sub as usize, "Programar apagado");
        match info.remaining {
            Some(s) => add(menu, MF_STRING, ID_CANCEL, &format!("Cancelar apagado (faltan {})", timeparse::clock(s))),
            None => add(menu, MF_STRING | MF_GRAYED, ID_CANCEL, "Cancelar apagado"),
        }
        AppendMenuW(menu, MF_SEPARATOR, 0, null());
        add(menu, MF_STRING, ID_DOCTOR, "Diagnóstico…");
        add(menu, MF_STRING | if info.autostart { MF_CHECKED } else { 0 }, ID_AUTOSTART, "Iniciar con Windows");
        add(menu, MF_STRING, ID_CONFIG, "Abrir configuración");
        add(menu, MF_STRING, ID_RELOAD, "Recargar configuración");
        AppendMenuW(menu, MF_SEPARATOR, 0, null());
        add(menu, MF_STRING, ID_EXIT, "Salir");

        let mut pt = POINT { x: 0, y: 0 };
        GetCursorPos(&mut pt);
        SetForegroundWindow(hwnd);
        let cmd = TrackPopupMenu(menu, TPM_RETURNCMD | TPM_RIGHTBUTTON | TPM_NONOTIFY, pt.x, pt.y, 0, hwnd, null());
        PostMessageW(hwnd, WM_NULL, 0, 0);
        DestroyMenu(menu);
        on_menu(hwnd, cmd as usize, &info);
    }
}

fn notify_result(r: Option<Response>) {
    if let Some(r) = r {
        with_agent(|a| a.tray.balloon("jhs", &r.message, !r.ok));
    }
}

fn on_menu(hwnd: HWND, id: usize, info: &MenuInfo) {
    match id {
        0 => {}
        ID_PRIVACY => {
            let r = with_agent(|a| a.privacy_on(None));
            if r.as_ref().is_some_and(|r| !r.ok) {
                notify_result(r);
            }
        }
        ID_CANCEL => notify_result(with_agent(|a| a.cancel())),
        ID_CUSTOM => timer_window::open(),
        ID_DOCTOR => {
            let Some(text) = with_agent(|a| {
                let status = a.status_json();
                crate::domain::doctor::render_text(&crate::adapters::windows::doctor::collect(Some(&status), &a.cfg, a.cfg_error.as_deref()))
            }) else {
                return;
            };
            unsafe {
                MessageBoxW(hwnd, wide(&text).as_ptr(), wide("jhs: diagnóstico").as_ptr(), MB_OK | MB_ICONINFORMATION);
            }
        }
        ID_AUTOSTART => {
            let r = crate::adapters::windows::install::set_autostart(!info.autostart);
            let msg = match (r, info.autostart) {
                (Ok(()), false) => Response::ok("jhs se iniciará con Windows"),
                (Ok(()), true) => Response::ok("jhs ya no se iniciará con Windows"),
                (Err(e), _) => Response::err(e),
            };
            notify_result(Some(msg));
        }
        ID_CONFIG => {
            Config::ensure_file();
            let _ = std::process::Command::new("notepad.exe").arg(config_path()).spawn();
        }
        ID_RELOAD => notify_result(with_agent(|a| a.reload_config())),
        ID_EXIT => unsafe {
            DestroyWindow(hwnd);
        },
        n if (ID_PRESET..ID_PRESET + info.presets.len()).contains(&n) => {
            let minutes = info.presets[n - ID_PRESET] as u64;
            notify_result(with_agent(|a| a.schedule(minutes * 60)));
        }
        _ => {}
    }
}
