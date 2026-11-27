//! Atajo global de Windows (`RegisterHotKey`) para un [`Combo`] del dominio.

use std::ptr::null_mut;

use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    MOD_ALT, MOD_CONTROL, MOD_NOREPEAT, MOD_SHIFT, MOD_WIN, RegisterHotKey, UnregisterHotKey,
};

use crate::adapters::windows::win;
use crate::domain::combo::Combo;

/// Modificadores para `RegisterHotKey`.
pub fn mods(combo: &Combo) -> u32 {
    let mut m = MOD_NOREPEAT;
    if combo.ctrl {
        m |= MOD_CONTROL;
    }
    if combo.shift {
        m |= MOD_SHIFT;
    }
    if combo.alt {
        m |= MOD_ALT;
    }
    if combo.win {
        m |= MOD_WIN;
    }
    m
}

/// Registra el atajo global en `hwnd`. Devuelve el código de error Win32 si falla
/// (1409 = ERROR_HOTKEY_ALREADY_REGISTERED: ya lo usa otra app).
pub(crate) fn register(hwnd: HWND, id: i32, combo: &Combo) -> Result<(), u32> {
    let ok = unsafe { RegisterHotKey(hwnd, id, mods(combo), combo.vk as u32) };
    if ok != 0 { Ok(()) } else { Err(win::last_error()) }
}

pub(crate) fn unregister(hwnd: HWND, id: i32) {
    unsafe {
        UnregisterHotKey(hwnd, id);
    }
}

/// Prueba si el combo está libre en Windows: lo registra y lo libera al instante.
pub fn test_free(combo: &Combo) -> Result<(), u32> {
    const TEST_ID: i32 = 0xBEEF;
    register(null_mut(), TEST_ID, combo)?;
    unregister(null_mut(), TEST_ID);
    Ok(())
}

pub fn explain_error(code: u32) -> String {
    if code == 1409 {
        "ya lo está usando otra aplicación".into()
    } else {
        win::os_error(code)
    }
}
