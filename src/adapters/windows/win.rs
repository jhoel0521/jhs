//! Utilidades Win32 compartidas: cadenas UTF-16, registro, mensajes.

use std::ffi::c_void;
use std::ptr::{null, null_mut};

use windows_sys::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS, GetLastError, HINSTANCE};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Registry::{
    HKEY_CURRENT_USER, REG_EXPAND_SZ, REG_SZ, RRF_NOEXPAND, RRF_RT_REG_DWORD, RRF_RT_REG_EXPAND_SZ,
    RRF_RT_REG_SZ, RegDeleteKeyValueW, RegGetValueW, RegSetKeyValueW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    MB_ICONERROR, MB_ICONINFORMATION, MB_OK, MB_SETFOREGROUND, MessageBoxW,
};

/// Convierte a UTF-16 terminado en NUL.
pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Lee un buffer UTF-16 hasta el primer NUL.
pub fn from_wide(buf: &[u16]) -> String {
    let n = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..n])
}

/// Copia `s` a un buffer UTF-16 fijo (truncando) y termina en NUL.
pub fn copy_to_buf(dst: &mut [u16], s: &str) {
    if dst.is_empty() {
        return;
    }
    let w: Vec<u16> = s.encode_utf16().take(dst.len() - 1).collect();
    dst[..w.len()].copy_from_slice(&w);
    dst[w.len()] = 0;
}

pub fn last_error() -> u32 {
    unsafe { GetLastError() }
}

/// Mensaje legible de un código de error Win32.
pub fn os_error(code: u32) -> String {
    format!("{} (código {code})", std::io::Error::from_raw_os_error(code as i32))
}

pub fn hinstance() -> HINSTANCE {
    unsafe { GetModuleHandleW(null()) }
}

pub fn message_box(text: &str, title: &str, error: bool) {
    let flags = MB_OK | MB_SETFOREGROUND | if error { MB_ICONERROR } else { MB_ICONINFORMATION };
    unsafe {
        MessageBoxW(null_mut(), wide(text).as_ptr(), wide(title).as_ptr(), flags);
    }
}

// ─── Registro (HKCU) ─────────────────────────────────────────────────────────

pub fn reg_get_string(subkey: &str, value: &str) -> Option<String> {
    let sk = wide(subkey);
    let v = wide(value);
    let flags = RRF_RT_REG_SZ | RRF_RT_REG_EXPAND_SZ | RRF_NOEXPAND;
    let mut size: u32 = 0;
    unsafe {
        let r = RegGetValueW(HKEY_CURRENT_USER, sk.as_ptr(), v.as_ptr(), flags, null_mut(), null_mut(), &mut size);
        if r != ERROR_SUCCESS {
            return None;
        }
        let mut buf = vec![0u16; size as usize / 2 + 2];
        let mut size2 = (buf.len() * 2) as u32;
        let r = RegGetValueW(
            HKEY_CURRENT_USER,
            sk.as_ptr(),
            v.as_ptr(),
            flags,
            null_mut(),
            buf.as_mut_ptr() as *mut c_void,
            &mut size2,
        );
        if r != ERROR_SUCCESS {
            return None;
        }
        Some(from_wide(&buf))
    }
}

pub fn reg_get_dword(subkey: &str, value: &str) -> Option<u32> {
    let sk = wide(subkey);
    let v = wide(value);
    let mut data: u32 = 0;
    let mut size = 4u32;
    let r = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            sk.as_ptr(),
            v.as_ptr(),
            RRF_RT_REG_DWORD,
            null_mut(),
            &mut data as *mut u32 as *mut c_void,
            &mut size,
        )
    };
    (r == ERROR_SUCCESS).then_some(data)
}

pub fn reg_set_string(subkey: &str, value: &str, data: &str, expand: bool) -> Result<(), String> {
    let sk = wide(subkey);
    let v = wide(value);
    let d = wide(data);
    let kind = if expand { REG_EXPAND_SZ } else { REG_SZ };
    let r = unsafe {
        RegSetKeyValueW(
            HKEY_CURRENT_USER,
            sk.as_ptr(),
            v.as_ptr(),
            kind,
            d.as_ptr() as *const c_void,
            (d.len() * 2) as u32,
        )
    };
    if r == ERROR_SUCCESS { Ok(()) } else { Err(os_error(r)) }
}

pub fn reg_delete_value(subkey: &str, value: &str) -> Result<(), String> {
    let sk = wide(subkey);
    let v = wide(value);
    let r = unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, sk.as_ptr(), v.as_ptr()) };
    if r == ERROR_SUCCESS || r == ERROR_FILE_NOT_FOUND { Ok(()) } else { Err(os_error(r)) }
}

/// ¿Windows está en modo oscuro para apps?
pub fn system_uses_dark_mode() -> bool {
    reg_get_dword(
        r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize",
        "AppsUseLightTheme",
    ) == Some(0)
}

pub use crate::domain::protocol::to_ascii_json;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copy_trunca() {
        let mut b = [1u16; 4];
        copy_to_buf(&mut b, "abcdef");
        assert_eq!(from_wide(&b), "abc");
    }
}
