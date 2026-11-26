# Viabilidad: jh-script en Linux

**Estado:** propuesta · **Veredicto:** viable, con una limitación en GNOME Wayland.

Hoy jh-script es solo para Windows (`compile_error!` en `src/lib.rs` fuera de Windows). Este documento analiza
qué haría falta para que el mismo CLI y el mismo agente funcionen en Linux.

## Qué se reutiliza tal cual

| Pieza | Archivo | Comentario |
|---|---|---|
| CLI (clap), alias, `--json`, códigos de salida | `src/cli.rs`, `src/app.rs` | Portable. |
| Protocolo JSON (`Request`/`Response`) | `src/ipc.rs` | Portable; solo cambia el transporte. |
| Config y estado | `src/config.rs`, `src/state.rs` | Cambiar rutas a XDG (`~/.config`, `~/.local/state`). |
| Parser de horas y combos | `src/timeparse.rs`, `src/hotkey.rs` | Portable (los códigos VK se mapearían a keysyms). |
| Skill para agentes de IA | `agent-skill/SKILL.md` | Igual. |

## Qué cambia por plataforma

| Función | Windows (hoy) | Linux (propuesta) | Dificultad |
|---|---|---|---|
| Transporte IPC | Named pipe | Socket Unix en `$XDG_RUNTIME_DIR/jh-script.sock` (permisos 0600) | Baja |
| "Servicio" | Agente en `HKCU\...\Run` | **Servicio de usuario systemd** (`systemctl --user`) o `.desktop` en `~/.config/autostart` | Baja |
| Apagado programado | `shutdown.exe /s /t N` | `org.freedesktop.login1.Manager.ScheduleShutdown` vía D-Bus (crate `zbus`), o `shutdown -h +N` / `shutdown -c` | Baja |
| No suspender | `SetThreadExecutionState` | `Inhibit("idle:sleep")` de logind (D-Bus) | Baja |
| Bandeja | `Shell_NotifyIconW` | StatusNotifierItem (crate `ksni`). GNOME requiere la extensión AppIndicator | Media |
| Accesos directos | `.lnk` | Archivos `.desktop` | Baja |
| Atajo global | `RegisterHotKey` | X11: `XGrabKey`. Wayland: portal `GlobalShortcuts` o atajo del compositor que ejecute `jh-script privacy toggle` | Media |
| **Modo privacidad** | Escritorio Win32 privado + hooks | Ver abajo | **Alta** |

## Modo privacidad en Linux (la parte difícil)

### X11

Viable, con la técnica de los bloqueadores de pantalla (i3lock, xsecurelock):

- una ventana `override-redirect` negra por monitor (XRandR), siempre encima,
- `XGrabKeyboard` + `XGrabPointer` exclusivos: ninguna otra app recibe entrada,
- el combo de desbloqueo se lee directamente de los eventos de la ventana.

Riesgos: cambio de terminal virtual (`Ctrl+Alt+F1…`) y `Ctrl+Alt+Backspace`. El vigilante podría detectar el
cambio de VT y ejecutar `loginctl lock-session` (equivalente a nuestro "bloquear-windows").

### Wayland

Un cliente normal **no puede** tapar la pantalla ni capturar la entrada global; es una decisión de seguridad de
Wayland. La vía correcta es el protocolo **`ext-session-lock-v1`**: el cliente se convierte en la pantalla de
bloqueo, el compositor le da una superficie por monitor y le envía toda la entrada. Si el cliente muere, la sesión
**sigue bloqueada**, que es más seguro que lo que hace Windows hoy.

- Compatible: Sway, Hyprland y otros wlroots; KDE Plasma 6 (a verificar por versión).
- Crates: `smithay-client-toolkit` (módulo `session_lock`) o `wayland-protocols`.

### GNOME (Wayland)

Mutter no implementa `ext-session-lock-v1`. Opciones, en orden de preferencia:

1. Usar el bloqueo real: `loginctl lock-session` para activar y `loginctl unlock-session` para desactivar desde
   `/rc`. **A verificar**: que GNOME Shell respete `unlock-session` sin contraseña para la misma sesión y qué
   pide polkit.
2. Una extensión de GNOME Shell que pinte negro y capture la entrada (más trabajo y mantenimiento por versión).

## Propuesta de código

```
src/platform/mod.rs        trait Platform { ipc, privacy, shutdown, tray, autostart, hotkey }
src/platform/windows/      lo actual, movido sin cambios de comportamiento
src/platform/linux/
    ipc.rs                 socket Unix
    shutdown.rs            logind (zbus)
    tray.rs                ksni
    privacy_x11.rs         x11rb + grabs
    privacy_wayland.rs     ext-session-lock-v1
    privacy_gnome.rs       loginctl lock/unlock
    autostart.rs           systemd --user
```

CI con matriz `windows-latest` + `ubuntu-latest` (`cargo build`, `cargo test`, `cargo clippy`).

## Fases sugeridas (cada una, una issue/PR)

1. **Refactor sin cambios visibles**: extraer el trait `Platform` y mover el código Win32 a `platform/windows`.
2. **Núcleo Linux**: socket Unix, apagado por logind, servicio systemd de usuario, `status` y `doctor`.
   Con esto ya sirve en servidores sin escritorio.
3. **Privacidad X11**.
4. **Privacidad Wayland** (`ext-session-lock-v1`: Sway/Hyprland/KDE).
5. **GNOME**: `loginctl lock/unlock-session` como alternativa.
6. **Bandeja** con `ksni` y atajo global (X11 / portal GlobalShortcuts).

## Riesgos y preguntas abiertas

- Distribución y entorno objetivo (¿GNOME o KDE? ¿X11 o Wayland?) para priorizar las fases 3–5.
- Probar en hardware real: varios monitores y conexión en caliente de pantallas.
- Permisos de polkit para `ScheduleShutdown` y `unlock-session` según la distribución.
