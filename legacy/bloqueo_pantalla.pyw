"""
Bloqueo de pantalla negra conmutado por hotkey global (Ctrl+Shift+Q por defecto).
Se ejecuta en segundo plano (bandeja del sistema).
- Ctrl+Shift+Q activa/desactiva la pantalla negra fullscreen.
- Mientras está activa, evita que el monitor se apague.
- Escape de emergencia: pulsa Esc 5 veces rápidas para cerrar el bloqueo.
"""

import sys
import os
import ctypes
import threading
import time
import tkinter as tk
import pystray
from PIL import Image, ImageDraw
from ctypes import wintypes

# ═══════════════════════════════════════════════════════════════════════════
# CONFIGURACIÓN DEL HOTKEY (personalizable)
# ═══════════════════════════════════════════════════════════════════════════
# Módulos: MOD_ALT = 0x0001, MOD_CONTROL = 0x0002, MOD_SHIFT = 0x0004, MOD_WIN = 0x0008
# Códigos de teclas virtuales (VK): https://docs.microsoft.com/windows/win32/inputdev/virtual-key-codes
HOTKEY_MODS = 0x0002 | 0x0004      # Ctrl + Shift
HOTKEY_VK   = 0x51                 # Tecla 'Q'

# Nota: si esta combinación tampoco funciona, prueba por ejemplo:
#   HOTKEY_MODS = 0x0001 | 0x0002 | 0x0004   # Ctrl+Alt+Shift
#   HOTKEY_VK   = 0x7B                       # F12
# o bien:
#   HOTKEY_MODS = 0x0002                     # solo Ctrl
#   HOTKEY_VK   = 0x70                       # F1
# ═══════════════════════════════════════════════════════════════════════════

# Prevención de apagado de pantalla
ES_CONTINUOUS = 0x80000000
ES_DISPLAY_REQUIRED = 0x00000002

class PowerBlocker:
    def __init__(self):
        self._stop = threading.Event()
        self._thread = threading.Thread(target=self._run, daemon=True)

    def start(self):
        if not self._thread.is_alive():
            self._stop.clear()
            self._thread = threading.Thread(target=self._run, daemon=True)
            self._thread.start()

    def stop(self):
        self._stop.set()
        # Restaurar comportamiento normal de energía
        ctypes.windll.kernel32.SetThreadExecutionState(ES_CONTINUOUS)

    def _run(self):
        while not self._stop.is_set():
            ctypes.windll.kernel32.SetThreadExecutionState(
                ES_CONTINUOUS | ES_DISPLAY_REQUIRED)
            self._stop.wait(30)

# ═══════════════════════════════════════════════════════════════════════════
# Pantalla negra con salida de emergencia (Esc × 5)
# ═══════════════════════════════════════════════════════════════════════════
class BlackOverlay:
    def __init__(self, on_close_callback=None):
        self.root = None
        self.on_close = on_close_callback
        self._esc_count = 0
        self._esc_timer = None

    def show(self):
        if self.root is not None:
            return
        self.root = tk.Tk()
        self.root.title("")
        self.root.configure(bg='black', cursor='none')
        self.root.attributes('-fullscreen', True)
        self.root.attributes('-topmost', True)
        self.root.overrideredirect(True)
        self.root.protocol("WM_DELETE_WINDOW", lambda: None)

        # Bloquear cierres normales
        self.root.bind("<Escape>", self._on_escape_pressed)
        self.root.bind("<Alt-KeyPress-F4>", lambda e: "break")
        self.root.bind("<Control-KeyPress-q>", lambda e: "break")
        self.root.bind("<Control-Alt-KeyPress-q>", lambda e: "break")

        # Mantenerse siempre encima
        def keep_on_top():
            if self.root:
                try:
                    self.root.lift()
                    self.root.attributes('-topmost', True)
                except:
                    pass
                self.root.after(300, keep_on_top)
        keep_on_top()

        self.root.protocol("WM_DELETE_WINDOW", self._close)
        self.root.mainloop()

    def _on_escape_pressed(self, event):
        """Contador de pulsaciones de Escape para salida de emergencia."""
        self._esc_count += 1
        if self._esc_timer:
            self.root.after_cancel(self._esc_timer)
        if self._esc_count >= 5:
            self._close()
        else:
            # Reiniciar contador si no se pulsa en 1 segundo
            self._esc_timer = self.root.after(1000, self._reset_esc)

    def _reset_esc(self):
        self._esc_count = 0

    def _close(self):
        if self.on_close:
            self.on_close()
        if self.root:
            self.root.destroy()
            self.root = None

    def close(self):
        if self.root:
            self.root.after(0, self._close)

# ═══════════════════════════════════════════════════════════════════════════
# Hotkey global con notificación en bandeja
# ═══════════════════════════════════════════════════════════════════════════
WM_HOTKEY = 0x0312

class GlobalHotkey:
    def __init__(self, callback, mods, vk, tray_icon=None):
        self.callback = callback
        self.mods = mods
        self.vk = vk
        self.tray_icon = tray_icon  # para notificaciones
        self.hwnd = None
        self.msg_id = 1
        self._running = False

    def _wnd_proc(self, hwnd, msg, wparam, lparam):
        if msg == WM_HOTKEY and wparam == self.msg_id:
            # Mostrar notificación de que el hotkey fue capturado (opcional)
            if self.tray_icon:
                try:
                    self.tray_icon.notify("Hotkey detectada",
                                          "Se ha pulsado la combinación de teclas.")
                except:
                    pass
            self.callback()
        return ctypes.windll.user32.DefWindowProcW(hwnd, msg, wparam, lparam)

    def _message_loop(self):
        WNDPROC = ctypes.WINFUNCTYPE(ctypes.c_long, ctypes.c_void_p, ctypes.c_uint,
                                     wintypes.WPARAM, wintypes.LPARAM)
        wc = ctypes.WNDCLASSW()
        wc.lpfnWndProc = WNDPROC(self._wnd_proc)
        wc.hInstance = ctypes.windll.kernel32.GetModuleHandleW(None)
        wc.lpszClassName = "HotkeyOverlayClass"
        atom = ctypes.windll.user32.RegisterClassW(ctypes.byref(wc))
        if not atom:
            raise RuntimeError("No se pudo registrar la clase de ventana.")
        self.hwnd = ctypes.windll.user32.CreateWindowExW(
            0, "HotkeyOverlayClass", "HotkeyListener", 0,
            0, 0, 0, 0, None, None, wc.hInstance, None)
        if not self.hwnd:
            raise RuntimeError("No se pudo crear la ventana oculta.")
        if not ctypes.windll.user32.RegisterHotKey(self.hwnd, self.msg_id, self.mods, self.vk):
            raise RuntimeError(f"No se pudo registrar el hotkey (mods={self.mods}, vk={self.vk}). "
                               "Ya está en uso por otro programa o no es válido.")

        msg = ctypes.MSG()
        while self._running:
            # Manejar errores en el bucle para evitar que el hilo muera
            try:
                if ctypes.windll.user32.GetMessageW(ctypes.byref(msg), self.hwnd, 0, 0) != 0:
                    ctypes.windll.user32.TranslateMessage(ctypes.byref(msg))
                    ctypes.windll.user32.DispatchMessageW(ctypes.byref(msg))
            except Exception:
                break
        self._cleanup()

    def start(self):
        self._running = True
        self._thread = threading.Thread(target=self._message_loop, daemon=True)
        self._thread.start()

    def stop(self):
        self._running = False
        if self.hwnd:
            ctypes.windll.user32.PostMessageW(self.hwnd, 0, 0, 0)

    def _cleanup(self):
        if self.hwnd:
            ctypes.windll.user32.UnregisterHotKey(self.hwnd, self.msg_id)
            ctypes.windll.user32.DestroyWindow(self.hwnd)
            self.hwnd = None

# ═══════════════════════════════════════════════════════════════════════════
# Ícono en la bandeja
# ═══════════════════════════════════════════════════════════════════════════
def create_image():
    img = Image.new('RGB', (64, 64), color=(0, 0, 0))
    draw = ImageDraw.Draw(img)
    draw.rectangle((16, 16, 48, 48), fill=(200, 200, 200))
    return img

# ═══════════════════════════════════════════════════════════════════════════
# App principal
# ═══════════════════════════════════════════════════════════════════════════
class App:
    def __init__(self):
        self.overlay = None
        self.blocker = PowerBlocker()
        self.hotkey = None
        self.tray = None
        self.overlay_active = False

    def toggle_overlay(self):
        if self.overlay_active:
            self.deactivate_overlay()
        else:
            self.activate_overlay()

    def activate_overlay(self):
        if self.overlay_active:
            return
        self.blocker.start()
        self.overlay = BlackOverlay(on_close_callback=self._on_overlay_closed)
        self.overlay_active = True
        self.overlay.show()  # bloqueará hasta que se cierre

    def deactivate_overlay(self):
        if not self.overlay_active:
            return
        if self.overlay:
            self.overlay.close()
        self.overlay_active = False
        self.blocker.stop()

    def _on_overlay_closed(self):
        if self.overlay_active:
            self.overlay_active = False
            self.blocker.stop()

    def exit_app(self, icon, item):
        self.deactivate_overlay()
        if self.hotkey:
            self.hotkey.stop()
        if self.tray:
            self.tray.stop()
        os._exit(0)

    def run(self):
        # Crear bandeja primero para poder pasar la referencia a las notificaciones
        image = create_image()
        self.tray = pystray.Icon("ScreenBlocker", image, "Bloqueo de pantalla")
        # Menú contextual
        menu = pystray.Menu(
            pystray.MenuItem('Activar/Desactivar bloqueo', self.toggle_overlay,
                            default=True, visible=True),
            pystray.MenuItem('Salir', self.exit_app)
        )
        self.tray.menu = menu

        # Iniciar hotkey (la notificación se mostrará al pulsar)
        self.hotkey = GlobalHotkey(self.toggle_overlay, HOTKEY_MODS, HOTKEY_VK,
                                   tray_icon=self.tray)
        try:
            self.hotkey.start()
        except RuntimeError as e:
            # Mostrar error en un cuadro de diálogo
            import tkinter.messagebox as msg
            msg.showerror("Error de hotkey",
                          f"No se pudo registrar la combinación de teclas.\n"
                          f"Detalle: {e}\n"
                          "Cambia las teclas en el script o cierra el programa que la esté usando.")
            sys.exit(1)

        # Ejecutar bandeja en hilo aparte
        threading.Thread(target=self.tray.run, daemon=True).start()

        # Mantener vivo el script
        try:
            while True:
                threading.Event().wait(1)
        except KeyboardInterrupt:
            self.exit_app(None, None)

# ═══════════════════════════════════════════════════════════════════════════
if __name__ == "__main__":
    # Ocultar consola si se ejecuta con python.exe
    if sys.executable.lower().endswith("python.exe"):
        script = os.path.abspath(sys.argv[0])
        pythonw = sys.executable.replace("python.exe", "pythonw.exe")
        if os.path.exists(pythonw):
            os.system(f'"{pythonw}" "{script}"')
            sys.exit(0)
    App().run()