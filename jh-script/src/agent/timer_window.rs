//! Ventanita nativa (Win32) para programar el apagado. Existe solo mientras
//! está abierta; respeta el modo oscuro de Windows y el DPI.

use std::cell::{Cell, RefCell};
use std::ffi::c_void;
use std::mem::{size_of, zeroed};
use std::ptr::{null, null_mut};
use std::time::{Duration, Instant};

use chrono::Local;
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Dwm::{DWMWA_USE_IMMERSIVE_DARK_MODE, DwmSetWindowAttribute};
use windows_sys::Win32::Graphics::Gdi::{
    COLOR_WINDOW, COLOR_WINDOWTEXT, CreateFontIndirectW, CreateSolidBrush, DeleteObject, FillRect,
    GetSysColor, HBRUSH, HDC, HFONT, InvalidateRect, SetBkColor, SetTextColor,
};
use windows_sys::Win32::UI::Controls::{EM_SETLIMITTEXT, EM_SETSEL, SetWindowTheme};
use windows_sys::Win32::UI::HiDpi::{AdjustWindowRectExForDpi, GetDpiForSystem, GetDpiForWindow, SystemParametersInfoForDpi};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{EnableWindow, SetFocus};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    BS_DEFPUSHBUTTON, BS_PUSHBUTTON, CreateWindowExW, DefWindowProcW, DestroyWindow,
    ES_AUTOHSCROLL, ES_CENTER, GetClientRect, GetDlgItem, GetWindowTextW, IDC_ARROW,
    LoadCursorW, NONCLIENTMETRICSW, RegisterClassExW, SPI_GETNONCLIENTMETRICS, SPI_GETWORKAREA,
    SW_SHOW, SW_SHOWNORMAL, SWP_NOACTIVATE, SWP_NOZORDER, SendMessageW, SetForegroundWindow,
    SetWindowPos, SetWindowTextW, ShowWindow, SystemParametersInfoW, WM_CLOSE, WM_COMMAND,
    WM_CREATE, WM_CTLCOLORBTN, WM_CTLCOLOREDIT, WM_CTLCOLORSTATIC, WM_DESTROY, WM_DPICHANGED,
    WM_ERASEBKGND, WM_SETFONT, WNDCLASSEXW, WS_CAPTION, WS_CHILD, WS_EX_CLIENTEDGE, WS_MINIMIZEBOX,
    WS_OVERLAPPED, WS_SYSMENU, WS_TABSTOP, WS_VISIBLE,
};

use super::tray;
use crate::timeparse;
use crate::win::{self, wide};

const CLASS: &str = "JhScriptShutdownWindow";
const IDOK: i32 = 1;
const IDCANCEL: i32 = 2;
const ID_CANCEL_SHUTDOWN: i32 = 3;
const ID_EDIT: i32 = 100;
const ID_STATUS: i32 = 101;

const CLIENT_W: i32 = 380;
const CLIENT_H: i32 = 196;

thread_local! {
    static WND: Cell<HWND> = const { Cell::new(null_mut()) };
    static UI: RefCell<Option<Ui>> = const { RefCell::new(None) };
}

#[derive(Clone, Copy)]
struct Palette {
    bg: u32,
    fg: u32,
    edit_bg: u32,
    muted: u32,
    amber: u32,
    red: u32,
    green: u32,
}

const fn rgb(r: u8, g: u8, b: u8) -> u32 {
    r as u32 | (g as u32) << 8 | (b as u32) << 16
}

fn palette(dark: bool) -> Palette {
    if dark {
        Palette {
            bg: rgb(32, 32, 32),
            fg: rgb(240, 240, 240),
            edit_bg: rgb(45, 45, 45),
            muted: rgb(160, 160, 160),
            amber: rgb(245, 158, 11),
            red: rgb(248, 113, 113),
            green: rgb(74, 222, 128),
        }
    } else {
        unsafe {
            Palette {
                bg: GetSysColor(COLOR_WINDOW),
                fg: GetSysColor(COLOR_WINDOWTEXT),
                edit_bg: GetSysColor(COLOR_WINDOW),
                muted: rgb(107, 114, 128),
                amber: rgb(180, 83, 9),
                red: rgb(220, 38, 38),
                green: rgb(22, 163, 74),
            }
        }
    }
}

struct Ui {
    edit: HWND,
    status: HWND,
    font: HFONT,
    font_big: HFONT,
    dark: bool,
    pal: Palette,
    bg: HBRUSH,
    edit_bg: HBRUSH,
    status_color: u32,
    hold_until: Option<Instant>,
}

pub fn register_class() {
    let class = wide(CLASS);
    let wc = WNDCLASSEXW {
        cbSize: size_of::<WNDCLASSEXW>() as u32,
        lpfnWndProc: Some(wndproc),
        hInstance: win::hinstance(),
        hIcon: tray::load_icon(32),
        hIconSm: tray::load_icon(16),
        hCursor: unsafe { LoadCursorW(null_mut(), IDC_ARROW) },
        lpszClassName: class.as_ptr(),
        ..unsafe { zeroed() }
    };
    unsafe { RegisterClassExW(&wc) };
}

pub fn current() -> HWND {
    WND.with(|w| w.get())
}

/// Abre la ventana (o la trae al frente si ya está abierta).
pub fn open() {
    let h = current();
    unsafe {
        if !h.is_null() {
            ShowWindow(h, SW_SHOWNORMAL);
            SetForegroundWindow(h);
            return;
        }
        let style = WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX;
        let dpi = GetDpiForSystem();
        let mut rc = RECT { left: 0, top: 0, right: scale(CLIENT_W, dpi), bottom: scale(CLIENT_H, dpi) };
        AdjustWindowRectExForDpi(&mut rc, style, 0, 0, dpi);
        let (w, h) = (rc.right - rc.left, rc.bottom - rc.top);
        let mut work: RECT = zeroed();
        SystemParametersInfoW(SPI_GETWORKAREA, 0, &mut work as *mut RECT as *mut c_void, 0);
        let x = work.left + (work.right - work.left - w) / 2;
        let y = work.top + (work.bottom - work.top - h) / 2;
        let class = wide(CLASS);
        let title = wide("Programar apagado — jh-script");
        let hwnd = CreateWindowExW(
            0,
            class.as_ptr(),
            title.as_ptr(),
            style,
            x,
            y,
            w,
            h,
            null_mut(),
            null_mut(),
            win::hinstance(),
            null(),
        );
        if hwnd.is_null() {
            return;
        }
        WND.with(|c| c.set(hwnd));
        ShowWindow(hwnd, SW_SHOW);
        SetForegroundWindow(hwnd);
    }
    super::ui_window_opened();
    refresh();
}

fn scale(v: i32, dpi: u32) -> i32 {
    (v as i64 * dpi as i64 / 96) as i32
}

/// Actualiza el texto de estado (llamado cada segundo por el agente).
pub fn refresh() {
    let hwnd = current();
    if hwnd.is_null() {
        return;
    }
    let held = UI.with(|u| u.borrow().as_ref().is_some_and(|ui| ui.hold_until.is_some_and(|t| Instant::now() < t)));
    let info = super::ui_shutdown_info();
    unsafe {
        EnableWindow(GetDlgItem(hwnd, ID_CANCEL_SHUTDOWN), info.is_some() as i32);
    }
    if held {
        return;
    }
    let Some(pal) = UI.with(|u| u.borrow().as_ref().map(|ui| ui.pal)) else { return };
    let (text, color) = match info {
        Some((rem, at, dry)) => (
            format!(
                "Apagado a las {} — faltan {}{}",
                at.format("%H:%M"),
                timeparse::clock(rem),
                if dry { " (prueba)" } else { "" }
            ),
            pal.amber,
        ),
        None => ("Sin apagado programado.".to_string(), pal.muted),
    };
    set_status(&text, color, None);
}

fn set_status(text: &str, color: u32, hold: Option<Duration>) {
    let Some(status) = UI.with(|u| {
        u.borrow_mut().as_mut().map(|ui| {
            ui.status_color = color;
            if hold.is_some() {
                ui.hold_until = hold.map(|d| Instant::now() + d);
            }
            ui.status
        })
    }) else {
        return;
    };
    unsafe {
        SetWindowTextW(status, wide(text).as_ptr());
        InvalidateRect(status, null(), 1);
    }
}

fn read_edit(edit: HWND) -> String {
    let mut buf = [0u16; 64];
    let n = unsafe { GetWindowTextW(edit, buf.as_mut_ptr(), buf.len() as i32) };
    String::from_utf16_lossy(&buf[..n.max(0) as usize])
}

fn on_create(hwnd: HWND) {
    unsafe {
        let dpi = GetDpiForWindow(hwnd);
        let s = |v: i32| scale(v, dpi);
        let dark = win::system_uses_dark_mode();
        let pal = palette(dark);

        let mut ncm: NONCLIENTMETRICSW = zeroed();
        ncm.cbSize = size_of::<NONCLIENTMETRICSW>() as u32;
        SystemParametersInfoForDpi(SPI_GETNONCLIENTMETRICS, ncm.cbSize, &mut ncm as *mut _ as *mut c_void, 0, dpi);
        let mut lf = ncm.lfMessageFont;
        lf.lfHeight = lf.lfHeight * 11 / 10;
        let font = CreateFontIndirectW(&lf);
        let mut lf_big = lf;
        lf_big.lfHeight = lf.lfHeight * 12 / 10;
        lf_big.lfWeight = 600;
        let font_big = CreateFontIndirectW(&lf_big);

        let child = |ex: u32, class: &str, text: &str, style: u32, x: i32, y: i32, w: i32, h: i32, id: i32| {
            let c = CreateWindowExW(
                ex,
                wide(class).as_ptr(),
                wide(text).as_ptr(),
                WS_CHILD | WS_VISIBLE | style,
                s(x),
                s(y),
                s(w),
                s(h),
                hwnd,
                id as usize as _,
                win::hinstance(),
                null(),
            );
            SendMessageW(c, WM_SETFONT, font as WPARAM, 1);
            c
        };

        child(0, "STATIC", "Minutos (ej. 60) u hora exacta (ej. 23:30):", 0, 20, 16, 340, 22, -1);
        let edit = child(
            WS_EX_CLIENTEDGE,
            "EDIT",
            "60",
            WS_TABSTOP | ES_CENTER as u32 | ES_AUTOHSCROLL as u32,
            20,
            44,
            150,
            30,
            ID_EDIT,
        );
        SendMessageW(edit, EM_SETLIMITTEXT, 8, 0);
        child(0, "STATIC", "Enter para programar", 0, 184, 50, 176, 22, -1);
        let status = child(0, "STATIC", "", 0, 20, 92, 340, 28, ID_STATUS);
        SendMessageW(status, WM_SETFONT, font_big as WPARAM, 1);
        let b1 = child(0, "BUTTON", "Programar", WS_TABSTOP | BS_DEFPUSHBUTTON as u32, 20, 142, 104, 34, IDOK);
        let b2 = child(0, "BUTTON", "Cancelar apagado", WS_TABSTOP | BS_PUSHBUTTON as u32, 134, 142, 136, 34, ID_CANCEL_SHUTDOWN);
        let b3 = child(0, "BUTTON", "Cerrar", WS_TABSTOP | BS_PUSHBUTTON as u32, 280, 142, 80, 34, IDCANCEL);

        if dark {
            let on: i32 = 1;
            DwmSetWindowAttribute(hwnd, DWMWA_USE_IMMERSIVE_DARK_MODE as u32, &on as *const i32 as *const c_void, 4);
            let explorer = wide("DarkMode_Explorer");
            for b in [b1, b2, b3] {
                SetWindowTheme(b, explorer.as_ptr(), null());
            }
            SetWindowTheme(edit, wide("DarkMode_CFD").as_ptr(), null());
        }

        UI.with(|u| {
            *u.borrow_mut() = Some(Ui {
                edit,
                status,
                font,
                font_big,
                dark,
                pal,
                bg: CreateSolidBrush(pal.bg),
                edit_bg: CreateSolidBrush(pal.edit_bg),
                status_color: pal.muted,
                hold_until: None,
            })
        });
        SetFocus(edit);
        SendMessageW(edit, EM_SETSEL, 0, -1);
    }
}

fn on_command(hwnd: HWND, id: i32) {
    let Some((edit, pal)) = UI.with(|u| u.borrow().as_ref().map(|ui| (ui.edit, ui.pal))) else { return };
    match id {
        IDOK => {
            let input = read_edit(edit);
            match timeparse::parse_when(&input, Local::now().naive_local()) {
                Err(e) => set_status(&e, pal.red, Some(Duration::from_secs(4))),
                Ok(secs) => {
                    let r = super::ui_schedule(secs);
                    let color = if r.ok { pal.green } else { pal.red };
                    set_status(&r.message, color, Some(Duration::from_secs(3)));
                }
            }
            refresh();
        }
        ID_CANCEL_SHUTDOWN => {
            let r = super::ui_cancel();
            set_status(&r.message, if r.ok { pal.green } else { pal.red }, Some(Duration::from_secs(3)));
            refresh();
        }
        IDCANCEL => unsafe {
            DestroyWindow(hwnd);
        },
        _ => {}
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    unsafe {
        match msg {
            WM_CREATE => {
                on_create(hwnd);
                0
            }
            WM_COMMAND => {
                on_command(hwnd, (wp & 0xFFFF) as i32);
                0
            }
            WM_ERASEBKGND => {
                if let Some(bg) = UI.with(|u| u.borrow().as_ref().map(|ui| ui.bg)) {
                    let mut rc: RECT = zeroed();
                    GetClientRect(hwnd, &mut rc);
                    FillRect(wp as HDC, &rc, bg);
                    return 1;
                }
                DefWindowProcW(hwnd, msg, wp, lp)
            }
            WM_CTLCOLORSTATIC | WM_CTLCOLORBTN => {
                let hdc = wp as HDC;
                let ctrl = lp as HWND;
                match UI.with(|u| u.borrow().as_ref().map(|ui| (ui.bg, ui.pal, ui.status, ui.status_color))) {
                    Some((bg, pal, status, status_color)) => {
                        SetTextColor(hdc, if ctrl == status { status_color } else { pal.fg });
                        SetBkColor(hdc, pal.bg);
                        bg as LRESULT
                    }
                    None => DefWindowProcW(hwnd, msg, wp, lp),
                }
            }
            WM_CTLCOLOREDIT => match UI.with(|u| u.borrow().as_ref().map(|ui| (ui.dark, ui.edit_bg, ui.pal))) {
                Some((true, brush, pal)) => {
                    let hdc = wp as HDC;
                    SetTextColor(hdc, pal.fg);
                    SetBkColor(hdc, pal.edit_bg);
                    brush as LRESULT
                }
                _ => DefWindowProcW(hwnd, msg, wp, lp),
            },
            WM_DPICHANGED => {
                let r = &*(lp as *const RECT);
                SetWindowPos(hwnd, null_mut(), r.left, r.top, r.right - r.left, r.bottom - r.top, SWP_NOZORDER | SWP_NOACTIVATE);
                0
            }
            WM_CLOSE => {
                DestroyWindow(hwnd);
                0
            }
            WM_DESTROY => {
                if let Some(ui) = UI.with(|u| u.borrow_mut().take()) {
                    DeleteObject(ui.font as _);
                    DeleteObject(ui.font_big as _);
                    DeleteObject(ui.bg as _);
                    DeleteObject(ui.edit_bg as _);
                }
                WND.with(|c| c.set(null_mut()));
                0
            }
            _ => DefWindowProcW(hwnd, msg, wp, lp),
        }
    }
}
