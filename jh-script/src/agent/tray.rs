//! Ícono de la bandeja del sistema (uno solo para todo jh-script).

use std::mem::{size_of, zeroed};

use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::UI::Shell::{
    NIF_ICON, NIF_INFO, NIF_MESSAGE, NIF_TIP, NIIF_INFO, NIIF_WARNING, NIM_ADD, NIM_DELETE,
    NIM_MODIFY, NOTIFYICONDATAW, Shell_NotifyIconW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetSystemMetrics, HICON, IMAGE_ICON, LR_DEFAULTCOLOR, LoadImageW, SM_CXSMICON, SM_CYSMICON,
};

use super::WM_APP_TRAY;
use crate::win;

pub struct Tray {
    hwnd: HWND,
    icon: HICON,
    tip: String,
    added: bool,
}

/// Ícono incrustado en el ejecutable (recurso 1).
pub fn load_icon(size: i32) -> HICON {
    unsafe { LoadImageW(win::hinstance(), 1 as _, IMAGE_ICON, size, size, LR_DEFAULTCOLOR) as HICON }
}

impl Tray {
    pub fn new(hwnd: HWND) -> Tray {
        let icon = unsafe {
            LoadImageW(
                win::hinstance(),
                1 as _,
                IMAGE_ICON,
                GetSystemMetrics(SM_CXSMICON),
                GetSystemMetrics(SM_CYSMICON),
                LR_DEFAULTCOLOR,
            ) as HICON
        };
        Tray { hwnd, icon, tip: "jh-script".into(), added: false }
    }

    fn base(&self) -> NOTIFYICONDATAW {
        let mut d: NOTIFYICONDATAW = unsafe { zeroed() };
        d.cbSize = size_of::<NOTIFYICONDATAW>() as u32;
        d.hWnd = self.hwnd;
        d.uID = 1;
        d
    }

    pub fn add(&mut self) {
        let mut d = self.base();
        d.uFlags = NIF_ICON | NIF_MESSAGE | NIF_TIP;
        d.uCallbackMessage = WM_APP_TRAY;
        d.hIcon = self.icon;
        win::copy_to_buf(&mut d.szTip, &self.tip);
        self.added = unsafe { Shell_NotifyIconW(NIM_ADD, &d) } != 0;
    }

    /// Explorer se reinició: hay que volver a añadir el ícono.
    pub fn readd(&mut self) {
        self.added = false;
        self.add();
    }

    pub fn set_tip(&mut self, tip: &str) {
        if tip == self.tip {
            return;
        }
        self.tip = tip.to_string();
        if !self.added {
            self.add();
            return;
        }
        let mut d = self.base();
        d.uFlags = NIF_TIP;
        win::copy_to_buf(&mut d.szTip, &self.tip);
        unsafe { Shell_NotifyIconW(NIM_MODIFY, &d) };
    }

    /// Notificación tipo globo / toast.
    pub fn balloon(&self, title: &str, text: &str, warning: bool) {
        let mut d = self.base();
        d.uFlags = NIF_INFO;
        d.dwInfoFlags = if warning { NIIF_WARNING } else { NIIF_INFO };
        win::copy_to_buf(&mut d.szInfoTitle, title);
        win::copy_to_buf(&mut d.szInfo, text);
        unsafe { Shell_NotifyIconW(NIM_MODIFY, &d) };
    }

    pub fn remove(&mut self) {
        if self.added {
            let d = self.base();
            unsafe { Shell_NotifyIconW(NIM_DELETE, &d) };
            self.added = false;
        }
    }
}
