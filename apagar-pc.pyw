"""
Temporizador de apagado con UI Moderna (CustomTkinter), cuenta regresiva,
minimizado a la bandeja del sistema (System Tray) y sin consola CMD.
"""

import sys
import os
import ctypes
import subprocess
import threading
from tkinter import messagebox
import pystray
from PIL import Image, ImageDraw
import customtkinter as ctk

# Configuración inicial del tema moderno
ctk.set_appearance_mode("System")  # Se adapta al modo oscuro/claro de Windows
ctk.set_default_color_theme("blue")

# Constante para evitar el parpadeo del comando subprocess
CREATE_NO_WINDOW = 0x08000000

remaining_seconds = 0
timer_id = None
tray_icon = None

# ─── Funciones de administración ─────────────────────────────────────────────

def is_admin():
    """Devuelve True si el script se ejecuta con privilegios de administrador."""
    try:
        return ctypes.windll.shell32.IsUserAnAdmin() != 0
    except Exception:
        return False

def run_as_admin():
    """Reinicia el script con permisos de administrador, forzando pythonw.exe."""
    script = os.path.abspath(sys.argv[0])
    params = ' '.join([f'"{arg}"' for arg in sys.argv[1:]])
    
    # Forzar pythonw.exe para evitar que se abra la consola en modo administrador
    executable = sys.executable
    if "python.exe" in executable.lower():
        executable = executable.lower().replace("python.exe", "pythonw.exe")
        
    try:
        ctypes.windll.shell32.ShellExecuteW(
            None, "runas", executable, f'"{script}" {params}', None, 1
        )
    except Exception as e:
        messagebox.showerror("Error", f"No se pudo solicitar elevación:\n{e}")
        sys.exit(1)

# ─── Lógica de Bandeja del Sistema (System Tray) ─────────────────────────────

def crear_imagen_icono():
    image = Image.new('RGB', (64, 64), color=(0, 122, 204))
    draw = ImageDraw.Draw(image)
    draw.rectangle((16, 16, 48, 48), fill="white")
    return image

def mostrar_ventana(icon, item):
    icon.stop()
    root.after(0, root.deiconify)

def ocultar_en_bandeja():
    root.withdraw()
    menu = pystray.Menu(
        pystray.MenuItem('Abrir Temporizador', mostrar_ventana),
        pystray.MenuItem('Salir', lambda icon, item: sys.exit(0))
    )
    
    global tray_icon
    tray_icon = pystray.Icon("Apagado", crear_imagen_icono(), "Apagado Programado", menu)
    threading.Thread(target=tray_icon.run, daemon=True).start()

# ─── Lógica de cuenta regresiva y apagado ────────────────────────────────────

def update_timer():
    global remaining_seconds, timer_id
    
    if remaining_seconds > 0:
        mins, secs = divmod(remaining_seconds, 60)
        time_str = f"{mins:02d}:{secs:02d}"
        status_label.configure(text=f"Apagando en: {time_str}", text_color="#f59e0b") # Color Ámbar
        remaining_seconds -= 1
        timer_id = root.after(1000, update_timer)
    else:
        status_label.configure(text="Apagando equipo...", text_color="#ef4444") # Color Rojo

def schedule_shutdown(minutes_entry):
    global remaining_seconds, timer_id

    try:
        minutes = int(minutes_entry.get())
        if minutes <= 0:
            status_label.configure(text="Ingrese un número positivo.", text_color="#ef4444")
            return
    except ValueError:
        status_label.configure(text="Ingrese un número entero válido.", text_color="#ef4444")
        return

    if timer_id:
        root.after_cancel(timer_id)

    remaining_seconds = minutes * 60

    subprocess.run(["shutdown", "/a"], capture_output=True, creationflags=CREATE_NO_WINDOW)
    subprocess.run(["shutdown", "/s", "/f", "/t", str(remaining_seconds)], capture_output=True, creationflags=CREATE_NO_WINDOW)

    update_timer()
    
    respuesta = messagebox.askyesno(
        "Programado",
        f"El equipo se apagará en {minutes} minutos.\n\n¿Deseas minimizar la app a la barra de iconos ocultos?"
    )
    
    if respuesta:
        ocultar_en_bandeja()

def cancel_shutdown():
    global timer_id
    if timer_id:
        root.after_cancel(timer_id)
        timer_id = None
        
    subprocess.run(["shutdown", "/a"], capture_output=True, creationflags=CREATE_NO_WINDOW)
    status_label.configure(text="Apagado cancelado.", text_color="#3b82f6") # Color Azul

# ─── Interfaz gráfica Moderna (CustomTkinter) ────────────────────────────────

def create_gui():
    global root, status_label
    
    root = ctk.CTk()
    root.title("Temporizador de Apagado")
    root.geometry("400x260")
    root.resizable(False, False)
    root.protocol("WM_DELETE_WINDOW", lambda: sys.exit(0))

    # Título
    titulo_label = ctk.CTkLabel(root, text="Minutos hasta el apagado:", font=("Roboto", 16, "bold"))
    titulo_label.pack(pady=(20, 5))

    # Input (Entry)
    minutes_entry = ctk.CTkEntry(root, font=("Roboto", 18), justify="center", width=120, height=40, corner_radius=8)
    minutes_entry.pack(pady=10)
    minutes_entry.insert(0, "60")

    # Estado
    status_label = ctk.CTkLabel(root, text="Listo para programar.", font=("Roboto", 14), text_color="gray")
    status_label.pack(pady=5)

    # Frame para botones
    btn_frame = ctk.CTkFrame(root, fg_color="transparent")
    btn_frame.pack(pady=10)

    # Botones estilo Tailwind
    btn_start = ctk.CTkButton(
        btn_frame, text="Iniciar apagado", font=("Roboto", 14, "bold"),
        command=lambda: schedule_shutdown(minutes_entry),
        fg_color="#22c55e", hover_color="#16a34a", text_color="white", # Verde Tailwind
        width=140, height=35, corner_radius=8
    )
    btn_start.pack(side="left", padx=10)

    btn_cancel = ctk.CTkButton(
        btn_frame, text="Cancelar", font=("Roboto", 14, "bold"),
        command=cancel_shutdown,
        fg_color="#ef4444", hover_color="#dc2626", text_color="white", # Rojo Tailwind
        width=140, height=35, corner_radius=8
    )
    btn_cancel.pack(side="left", padx=10)
    
    # Enlace para ocultar
    btn_hide = ctk.CTkButton(
        root, text="Ocultar en la bandeja 👇", font=("Roboto", 12),
        command=ocultar_en_bandeja, 
        fg_color="transparent", hover_color="#e5e7eb", text_color="gray", 
        width=200, height=25
    )
    btn_hide.pack(pady=5)

    root.mainloop()

# ─── Punto de entrada principal ──────────────────────────────────────────────

if __name__ == "__main__":
    if not is_admin():
        run_as_admin()
        sys.exit(0)
    else:
        create_gui()