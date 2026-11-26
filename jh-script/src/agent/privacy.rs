//! Modo privacidad.
//!
//! Crea un escritorio Win32 privado (`CreateDesktopW` + `SwitchDesktop`, el
//! mismo mecanismo que usan UAC y la pantalla de bloqueo). Tus programas siguen
//! corriendo en el escritorio normal, pero en pantalla solo existe este:
//! - una ventana negra por monitor (todas las pantallas),
//! - sin barra de tareas, tecla Windows, Alt+Tab ni notificaciones,
//! - teclado y mouse tragados por hooks de bajo nivel (el combo de desbloqueo
//!   se detecta dentro del hook, sin depender del foco),
//! - un vigilante que detecta si alguien sale (p. ej. Ctrl+Alt+Supr).

use std::cell::{Cell, RefCell};
use std::ffi::c_void;
use std::mem::{size_of, zeroed};
use std::ptr::{null, null_mut};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use chrono::{DateTime, Local};
use windows_sys::Win32::Foundation::{GENERIC_ALL, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{
    BLACK_BRUSH, EnumDisplayMonitors, GetStockObject, HBRUSH, HDC, HMONITOR,
};
use windows_sys::Win32::System::Power::{
    ES_CONTINUOUS, ES_DISPLAY_REQUIRED, ES_SYSTEM_REQUIRED, SetThreadExecutionState,
};
use windows_sys::Win32::System::Shutdown::LockWorkStation;
use windows_sys::Win32::System::StationsAndDesktops::{
    CloseDesktop, CreateDesktopW, DESKTOP_READOBJECTS, DESKTOP_SWITCHDESKTOP, EnumDesktopWindows,
    GetThreadDesktop, GetUserObjectInformationW, HDESK, OpenDesktopW, OpenInputDesktop,
    SetThreadDesktop, SwitchDesktop, UOI_NAME,
};
use windows_sys::Win32::System::Threading::{GetCurrentProcessId, GetCurrentThreadId};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::GetKeyState;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetMessageW,
    GetSystemMetrics, GetWindowThreadProcessId, HC_ACTION, IsWindowVisible, KBDLLHOOKSTRUCT,
    KillTimer, MSG, PostMessageW, PostQuitMessage, PostThreadMessageW, RegisterClassExW,
    SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN, SW_HIDE,
    SetCursor, SetForegroundWindow, SetTimer, SetWindowsHookExW, ShowWindow, TranslateMessage,
    UnhookWindowsHookEx, WH_KEYBOARD_LL, WH_MOUSE_LL, WM_APP, WM_CLOSE, WM_DISPLAYCHANGE,
    WM_KEYDOWN, WM_QUIT, WM_SETCURSOR, WM_SYSCOMMAND, WM_SYSKEYDOWN, WM_TIMER, WNDCLASSEXW,
    WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP, WS_VISIBLE,
};

use super::WM_APP_PRIVACY_ENDED;
use crate::config::EscapeAction;
use crate::hotkey::Combo;
use crate::state;
use crate::win::{self, from_wide, wide};

const CLASS: &str = "JhScriptPrivacyBlack";
const DESKTOP_PREFIX: &str = "jh-privacidad-";
const WM_APP_REBUILD: u32 = WM_APP + 20;

type Bool = i32;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EndReason {
    Combo = 1,
    Cli = 2,
    Timeout = 3,
    Escape = 4,
    Error = 5,
}

impl EndReason {
    pub fn from_code(c: usize) -> EndReason {
        match c {
            1 => EndReason::Combo,
            2 => EndReason::Cli,
            3 => EndReason::Timeout,
            4 => EndReason::Escape,
            _ => EndReason::Error,
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            EndReason::Combo => "combo",
            EndReason::Cli => "cli",
            EndReason::Timeout => "tiempo",
            EndReason::Escape => "escape",
            EndReason::Error => "error",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            EndReason::Combo => "combo de desbloqueo",
            EndReason::Cli => "comando (CLI)",
            EndReason::Timeout => "tiempo cumplido",
            EndReason::Escape => "intento de salida detectado (Windows bloqueado)",
            EndReason::Error => "error",
        }
    }
}

pub struct Params {
    pub unlock: Combo,
    pub escape: EscapeAction,
    pub keep_display: bool,
    pub for_secs: Option<u64>,
    pub notify: HWND,
}

/// Parámetros que viajan al hilo del escritorio privado (solo tipos `Send`).
struct ThreadParams {
    id: u32,
    name: String,
    desk: isize,
    orig: isize,
    notify: isize,
    unlock: Combo,
    escape: EscapeAction,
    keep_display: bool,
    for_secs: Option<u64>,
}

static NEXT_ID: AtomicU32 = AtomicU32::new(1);

thread_local! {
    static WINDOWS: RefCell<Vec<HWND>> = const { RefCell::new(Vec::new()) };
    static KEYS: RefCell<[bool; 256]> = const { RefCell::new([false; 256]) };
    static UNLOCK: Cell<Option<Combo>> = const { Cell::new(None) };
}

pub struct Session {
    pub id: u32,
    desk: isize,
    thread: Option<JoinHandle<()>>,
    thread_id: u32,
    pub since: DateTime<Local>,
    pub until: Option<DateTime<Local>>,
}

impl Session {
    fn request_stop(&self, reason: EndReason) {
        unsafe {
            PostThreadMessageW(self.thread_id, WM_QUIT, reason as usize, 0);
        }
    }

    /// Termina el modo privacidad y espera a que el hilo salga.
    pub fn stop(self, reason: EndReason) {
        self.request_stop(reason);
        drop(self);
    }

    pub fn is_finished(&self) -> bool {
        self.thread.as_ref().is_none_or(|t| t.is_finished())
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        if let Some(t) = self.thread.take() {
            if !t.is_finished() {
                self.request_stop(EndReason::Cli);
            }
            let _ = t.join();
        }
        if self.desk != 0 {
            unsafe { CloseDesktop(self.desk as HDESK) };
            self.desk = 0;
        }
    }
}

pub fn register_class() {
    let class = wide(CLASS);
    let wc = WNDCLASSEXW {
        cbSize: size_of::<WNDCLASSEXW>() as u32,
        lpfnWndProc: Some(black_proc),
        hInstance: win::hinstance(),
        hbrBackground: unsafe { GetStockObject(BLACK_BRUSH) } as HBRUSH,
        lpszClassName: class.as_ptr(),
        ..unsafe { zeroed() }
    };
    unsafe { RegisterClassExW(&wc) };
}

pub fn start(p: Params) -> Result<Session, String> {
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    let name = format!("{DESKTOP_PREFIX}{}-{id}", std::process::id());
    let wname = wide(&name);
    let desk = unsafe { CreateDesktopW(wname.as_ptr(), null(), null(), 0, GENERIC_ALL, null()) };
    if desk.is_null() {
        return Err(format!("CreateDesktop falló: {}", win::os_error(win::last_error())));
    }
    let orig = unsafe { GetThreadDesktop(GetCurrentThreadId()) };
    let tp = ThreadParams {
        id,
        name,
        desk: desk as isize,
        orig: orig as isize,
        notify: p.notify as isize,
        unlock: p.unlock,
        escape: p.escape,
        keep_display: p.keep_display,
        for_secs: p.for_secs,
    };
    let since = Local::now();
    let until = p.for_secs.map(|s| since + chrono::Duration::seconds(s as i64));
    let (tx, rx) = mpsc::channel::<Result<u32, String>>();
    let thread = std::thread::Builder::new()
        .name("privacidad".into())
        .spawn(move || {
            let notify = tp.notify;
            let id = tp.id;
            let reason = thread_main(&tp, tx);
            unsafe {
                PostMessageW(notify as HWND, WM_APP_PRIVACY_ENDED, reason as usize, id as isize);
            }
        })
        .map_err(|e| e.to_string())?;
    match rx.recv_timeout(Duration::from_secs(5)) {
        Ok(Ok(thread_id)) => Ok(Session { id, desk: desk as isize, thread: Some(thread), thread_id, since, until }),
        Ok(Err(e)) => {
            let _ = thread.join();
            unsafe { CloseDesktop(desk) };
            Err(e)
        }
        Err(_) => Err("el escritorio privado no respondió a tiempo".into()),
    }
}

fn thread_main(tp: &ThreadParams, tx: mpsc::Sender<Result<u32, String>>) -> EndReason {
    unsafe {
        let desk = tp.desk as HDESK;
        if SetThreadDesktop(desk) == 0 {
            let _ = tx.send(Err(format!("SetThreadDesktop falló: {}", win::os_error(win::last_error()))));
            return EndReason::Error;
        }
        UNLOCK.with(|u| u.set(Some(tp.unlock)));
        KEYS.with(|k| *k.borrow_mut() = [false; 256]);
        create_windows();
        if WINDOWS.with(|w| w.borrow().is_empty()) {
            let _ = tx.send(Err("no se pudieron crear las ventanas negras".into()));
            return EndReason::Error;
        }
        let hmod = win::hinstance();
        let kb = SetWindowsHookExW(WH_KEYBOARD_LL, Some(kb_proc), hmod, 0);
        let ms = SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_proc), hmod, 0);
        if kb.is_null() || ms.is_null() {
            state::log("aviso: no se pudo instalar un hook de teclado/mouse");
        }
        let mut es = ES_CONTINUOUS | ES_SYSTEM_REQUIRED;
        if tp.keep_display {
            es |= ES_DISPLAY_REQUIRED;
        }
        SetThreadExecutionState(es);

        if SwitchDesktop(desk) == 0 {
            let e = win::last_error();
            cleanup(kb, ms, None);
            let _ = tx.send(Err(format!(
                "no se pudo mostrar el escritorio privado ({}). ¿Windows está bloqueado?",
                win::os_error(e)
            )));
            return EndReason::Error;
        }
        if let Some(&first) = WINDOWS.with(|w| w.borrow().first().copied()).as_ref() {
            SetForegroundWindow(first);
        }
        let _ = tx.send(Ok(GetCurrentThreadId()));

        let timer = SetTimer(null_mut(), 0, 250, None);
        let deadline = tp.for_secs.map(|s| Instant::now() + Duration::from_secs(s));
        let mut watch = WatchState::default();
        let mut msg: MSG = zeroed();
        let reason = loop {
            let r = GetMessageW(&mut msg, null_mut(), 0, 0);
            if r == 0 {
                break EndReason::from_code(msg.wParam);
            }
            if r == -1 {
                break EndReason::Error;
            }
            if msg.hwnd.is_null() {
                if msg.message == WM_TIMER && msg.wParam == timer {
                    if deadline.is_some_and(|d| Instant::now() >= d) {
                        break EndReason::Timeout;
                    }
                    if let Some(r) = watchdog(tp, &mut watch) {
                        break r;
                    }
                    continue;
                }
                if msg.message == WM_APP_REBUILD {
                    destroy_windows();
                    create_windows();
                    continue;
                }
            }
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        };
        KillTimer(null_mut(), timer);
        cleanup(kb, ms, Some(tp.orig as HDESK));
        state::log(&format!("modo privacidad desactivado: {}", reason.label()));
        reason
    }
}

unsafe fn cleanup(kb: *mut c_void, ms: *mut c_void, back_to: Option<HDESK>) {
    unsafe {
        if !kb.is_null() {
            UnhookWindowsHookEx(kb);
        }
        if !ms.is_null() {
            UnhookWindowsHookEx(ms);
        }
        if let Some(orig) = back_to {
            SwitchDesktop(orig);
        }
        destroy_windows();
        SetThreadExecutionState(ES_CONTINUOUS);
    }
}

// ─── Ventanas negras (una por monitor) ───────────────────────────────────────

fn monitor_rects() -> Vec<RECT> {
    unsafe extern "system" fn cb(_m: HMONITOR, _dc: HDC, rc: *mut RECT, lp: LPARAM) -> Bool {
        unsafe {
            let v = &mut *(lp as *mut Vec<RECT>);
            v.push(*rc);
        }
        1
    }
    let mut v: Vec<RECT> = Vec::new();
    unsafe {
        EnumDisplayMonitors(null_mut(), null(), Some(cb), &mut v as *mut Vec<RECT> as LPARAM);
        if v.is_empty() {
            let x = GetSystemMetrics(SM_XVIRTUALSCREEN);
            let y = GetSystemMetrics(SM_YVIRTUALSCREEN);
            v.push(RECT {
                left: x,
                top: y,
                right: x + GetSystemMetrics(SM_CXVIRTUALSCREEN),
                bottom: y + GetSystemMetrics(SM_CYVIRTUALSCREEN),
            });
        }
    }
    v
}

fn create_windows() {
    let class = wide(CLASS);
    let title = wide("jh-script privacidad");
    let mut created = Vec::new();
    for r in monitor_rects() {
        let h = unsafe {
            CreateWindowExW(
                WS_EX_TOPMOST | WS_EX_TOOLWINDOW,
                class.as_ptr(),
                title.as_ptr(),
                WS_POPUP | WS_VISIBLE,
                r.left,
                r.top,
                r.right - r.left,
                r.bottom - r.top,
                null_mut(),
                null_mut(),
                win::hinstance(),
                null(),
            )
        };
        if !h.is_null() {
            created.push(h);
        }
    }
    WINDOWS.with(|w| w.borrow_mut().extend(created));
}

fn destroy_windows() {
    let list = WINDOWS.with(|w| std::mem::take(&mut *w.borrow_mut()));
    for h in list {
        unsafe { DestroyWindow(h) };
    }
}

unsafe extern "system" fn black_proc(h: HWND, m: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    unsafe {
        match m {
            WM_SETCURSOR => {
                SetCursor(null_mut());
                1
            }
            // Ignora cierres, menú de sistema, salvapantallas y apagado de monitor.
            WM_CLOSE | WM_SYSCOMMAND => 0,
            WM_DISPLAYCHANGE => {
                PostThreadMessageW(GetCurrentThreadId(), WM_APP_REBUILD, 0, 0);
                0
            }
            // Respaldo por si Windows retirara el hook de teclado.
            WM_KEYDOWN | WM_SYSKEYDOWN => {
                if let Some(c) = UNLOCK.with(|u| u.get()) {
                    let mut down = [false; 256];
                    for vk in [0x10usize, 0x11, 0x12, 0x5B, 0x5C] {
                        down[vk] = GetKeyState(vk as i32) < 0;
                    }
                    if c.matches(wp as u16, &down) {
                        PostQuitMessage(EndReason::Combo as i32);
                    }
                }
                0
            }
            _ => DefWindowProcW(h, m, wp, lp),
        }
    }
}

// ─── Hooks de bajo nivel ─────────────────────────────────────────────────────

unsafe extern "system" fn kb_proc(code: i32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    unsafe {
        if code == HC_ACTION as i32 {
            let k = &*(lp as *const KBDLLHOOKSTRUCT);
            let vk = (k.vkCode & 0xFF) as usize;
            let down = matches!(wp as u32, WM_KEYDOWN | WM_SYSKEYDOWN);
            let hit = KEYS.with(|ks| {
                let mut ks = ks.borrow_mut();
                let was = ks[vk];
                ks[vk] = down;
                down && !was && UNLOCK.with(|u| u.get()).is_some_and(|c| c.matches(vk as u16, &ks))
            });
            if hit {
                PostQuitMessage(EndReason::Combo as i32);
            }
            return 1; // tragar la tecla
        }
        CallNextHookEx(null_mut(), code, wp, lp)
    }
}

unsafe extern "system" fn mouse_proc(code: i32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    unsafe {
        if code == HC_ACTION as i32 {
            return 1; // tragar movimiento, clics y rueda
        }
        CallNextHookEx(null_mut(), code, wp, lp)
    }
}

// ─── Vigilante ───────────────────────────────────────────────────────────────

#[derive(Default)]
struct WatchState {
    ticks: u32,
    escape_logged: bool,
}

/// Nombre del escritorio que recibe la entrada ahora mismo. `None` = escritorio
/// seguro (Ctrl+Alt+Supr, UAC, pantalla de bloqueo), al que no tenemos acceso.
pub fn input_desktop_name() -> Option<String> {
    unsafe {
        let h = OpenInputDesktop(0, 0, DESKTOP_READOBJECTS);
        if h.is_null() {
            return None;
        }
        let mut buf = [0u16; 256];
        let mut needed = 0u32;
        let ok = GetUserObjectInformationW(
            h as _,
            UOI_NAME,
            buf.as_mut_ptr() as *mut c_void,
            (buf.len() * 2) as u32,
            &mut needed,
        );
        CloseDesktop(h);
        (ok != 0).then(|| from_wide(&buf))
    }
}

fn foreign_windows(desk: HDESK) -> Vec<HWND> {
    unsafe extern "system" fn cb(h: HWND, lp: LPARAM) -> Bool {
        unsafe {
            if IsWindowVisible(h) != 0 {
                let mut pid = 0u32;
                GetWindowThreadProcessId(h, &mut pid);
                if pid != GetCurrentProcessId() {
                    (*(lp as *mut Vec<HWND>)).push(h);
                }
            }
        }
        1
    }
    let mut v: Vec<HWND> = Vec::new();
    unsafe { EnumDesktopWindows(desk, Some(cb), &mut v as *mut Vec<HWND> as LPARAM) };
    v
}

fn watchdog(tp: &ThreadParams, w: &mut WatchState) -> Option<EndReason> {
    w.ticks = w.ticks.wrapping_add(1);
    let desk = tp.desk as HDESK;
    let mut intruders = Vec::new();
    let escaped = match input_desktop_name() {
        None => false,
        Some(n) if n == tp.name => {
            if w.ticks.is_multiple_of(4) {
                intruders = foreign_windows(desk);
            }
            !intruders.is_empty()
        }
        Some(n) if n.eq_ignore_ascii_case("Winlogon") || n.eq_ignore_ascii_case("Screen-saver") => false,
        Some(_) => true,
    };
    if !escaped {
        w.escape_logged = false;
        return None;
    }
    match tp.escape {
        EscapeAction::BloquearWindows => {
            state::log("escape detectado: bloqueando Windows");
            unsafe { LockWorkStation() };
            Some(EndReason::Escape)
        }
        EscapeAction::Volver => {
            state::log("escape detectado: volviendo al escritorio privado");
            unsafe {
                for h in intruders {
                    ShowWindow(h, SW_HIDE);
                }
                SwitchDesktop(desk);
            }
            None
        }
        EscapeAction::Nada => {
            if !w.escape_logged {
                state::log("escape detectado (al_detectar_escape = \"nada\")");
                w.escape_logged = true;
            }
            None
        }
    }
}

// ─── Utilidades para CLI y diagnóstico ───────────────────────────────────────

/// ¿El escritorio visible ahora es uno de jh-script?
pub fn private_desktop_visible() -> bool {
    input_desktop_name().is_some_and(|n| n.starts_with(DESKTOP_PREFIX))
}

/// Rescate: vuelve al escritorio normal sin pasar por el agente
/// (por si el agente murió estando activo el modo privacidad).
pub fn rescue_to_default() -> Result<(), String> {
    unsafe {
        let h = OpenDesktopW(wide("Default").as_ptr(), 0, 0, DESKTOP_SWITCHDESKTOP);
        if h.is_null() {
            return Err(format!("OpenDesktop falló: {}", win::os_error(win::last_error())));
        }
        let ok = SwitchDesktop(h);
        let e = win::last_error();
        CloseDesktop(h);
        if ok != 0 { Ok(()) } else { Err(format!("SwitchDesktop falló: {}", win::os_error(e))) }
    }
}

/// Prueba en seco: ¿se puede crear un escritorio privado?
pub fn test_create_desktop() -> Result<(), String> {
    let name = wide(&format!("{DESKTOP_PREFIX}prueba-{}", std::process::id()));
    unsafe {
        let d = CreateDesktopW(name.as_ptr(), null(), null(), 0, GENERIC_ALL, null());
        if d.is_null() {
            return Err(win::os_error(win::last_error()));
        }
        CloseDesktop(d);
    }
    Ok(())
}
