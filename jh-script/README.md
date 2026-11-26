# jh-script

Agente ligero para Windows con dos funciones:

- **Modo privacidad**: oculta *todas* las pantallas y bloquea teclado y mouse mientras tus programas
  (servidores de desarrollo, pruebas e2e largas, descargas…) siguen corriendo. Pensado para irte del PC
  sin que nadie pueda mirar ni tocar, y controlarlo a distancia (por ejemplo con Claude `/rc`).
- **Apagado programado**: en N minutos o a una hora exacta, con avisos y una ventanita nativa.

Todo lo controla **un solo agente** en la bandeja del sistema (~2 MB de memoria privada, 0 % CPU en reposo),
al que se le habla por **CLI** (`jh-script`), por atajo de teclado, por el menú de la bandeja o con accesos directos.

## Instalación

Requisitos para compilar: Rust (toolchain MSVC) y Visual Studio Build Tools con el Windows SDK.

```powershell
cd jh-script
cargo build --release
.\target\release\jh-script.exe install
```

`install` (sin permisos de administrador):

- copia `jh-script.exe` y `jh-scriptw.exe` a `%LOCALAPPDATA%\Programs\jh-script\` y los añade al PATH del usuario,
- configura el autoarranque al iniciar sesión (`HKCU\...\Run`),
- crea accesos directos: **Modo privacidad** (Escritorio y Menú Inicio) y **Programar apagado** (Menú Inicio),
- crea `%APPDATA%\jh-script\config.toml`,
- instala la skill de Claude Code en `~\.claude\skills\jh-script\` (si existe `~\.claude`),
- inicia el agente.

`jh-script uninstall` revierte todo, salvo la configuración y el log.

## Uso

| Qué | Comando | Alias |
|---|---|---|
| Activar modo privacidad | `jh-script privacy on` | `--off-screen`, `off-screen`, `privacidad on`, `apagar-pantalla` |
| …solo por N segundos | `jh-script privacy on --for 600` | |
| Desactivar modo privacidad | `jh-script privacy off` | `--on-screen`, `on-screen`, `privacidad off`, `encender-pantalla` |
| Alternar | `jh-script privacy toggle` | |
| Apagar en N minutos | `jh-script shutdown in 90` | `programar-apagado 90` |
| Apagar a una hora | `jh-script shutdown at 23:30` | `programar-apagado 23:30` |
| Cancelar apagado | `jh-script shutdown cancel` | `cancelar-apagado` |
| Ventanita de apagado | `jh-script shutdown ui` | |
| Estado | `jh-script status` | `estado` |
| Diagnóstico | `jh-script doctor` | `diagnostico` |
| ¿Está libre un atajo? | `jh-script hotkey test "Ctrl+Alt+F12"` | |
| Agente | `jh-script agent` · `agent stop` · `agent reload` | |

Opción global `--json` para scripts y agentes de IA (los acentos salen escapados, `\u00e1`, para que ninguna
consola los rompa). Códigos de salida: `0` ok · `1` error · `2` agente no disponible · `3` argumentos inválidos.

Si el agente no está corriendo, `privacy on`, `shutdown …` y `cancelar-apagado` lo inician solos.

### Bandeja del sistema

- **Doble clic**: activa el modo privacidad.
- **Clic derecho**: modo privacidad · Programar apagado ▸ 15/30/60/90/120 min / Personalizado… · Cancelar
  apagado · Diagnóstico · Iniciar con Windows · Abrir/Recargar configuración · Salir.
- El tooltip muestra la cuenta regresiva del apagado.

## Modo privacidad: cómo funciona

Usa un **escritorio privado de Win32** (`CreateDesktopW` + `SwitchDesktop`), el mismo mecanismo que usan UAC y la
pantalla de bloqueo. Tus programas siguen en el escritorio normal; en pantalla solo existe el privado:

- una ventana negra por monitor (se recalcula si conectas o desconectas pantallas),
- no hay barra de tareas, tecla Windows, Alt+Tab ni notificaciones: Explorer no vive en ese escritorio,
- hooks de teclado y mouse de bajo nivel tragan toda la entrada; el **combo de desbloqueo** se detecta dentro
  del hook, sin depender del foco (Alt+Tab ya no puede "perder el evento"),
- ningún otro programa (AutoHotkey, PowerToys…) puede escuchar el teclado ahí, así que el combo es exclusivo,
- el PC y el monitor no se suspenden mientras está activo (sin ahorro de energía),
- se frena el cierre de sesión mientras está activo (salvo el apagado que tú programaste).

**Vigilante:** `Ctrl+Alt+Supr` no se puede bloquear (es una garantía de Windows). Si alguien lo usa para salir
(por ejemplo, abriendo el Administrador de tareas) o aparece una ventana ajena, el agente lo detecta en ≤250 ms y,
por defecto, **bloquea Windows** con tu contraseña. Tus programas siguen corriendo. `jh-script status` muestra el
motivo (`ultimo_fin.motivo = "escape"`).

**Si el agente se cae** con el modo activo, Windows destruye el escritorio privado y vuelve solo al normal.
Además, `jh-script privacy off` intenta un rescate directo si el agente no responde.

**Limitación:** las herramientas que mueven el mouse o el teclado *reales* (`SendInput`, AutoHotkey, pywinauto)
o capturan la pantalla completa no funcionan mientras está activo, igual que con el PC bloqueado.
Playwright, Cypress, Selenium y tauri-driver usan CDP/WebDriver y siguen funcionando.

## Configuración

`%APPDATA%\jh-script\config.toml` (se crea con comentarios la primera vez):

```toml
[privacidad]
atajo_activar = "Ctrl+Shift+Q"        # "" = sin atajo global
combo_desbloqueo = "Ctrl+Shift+Q"     # combo secreto dentro del modo privacidad
al_detectar_escape = "bloquear-windows"  # o "volver" / "nada"
mantener_pantalla_encendida = true

[apagado]
forzar = true                         # shutdown /f
presets_min = [15, 30, 60, 90, 120]
avisos_min = [10, 5, 1]
```

Tras editar: bandeja > *Recargar configuración* o `jh-script agent reload`.
Si `atajo_activar` lo usa otra app, el agente lo avisa y `jh-script doctor` lo muestra.

## Agentes de IA

No hace falta un servidor MCP: cualquier agente con terminal (Claude Code, también vía Remote Control) usa el CLI
con `--json`. `install` copia [agent-skill/SKILL.md](agent-skill/SKILL.md) como skill de Claude Code, con las reglas
de uso (empezar por `status --json`, no programar apagados sin que el usuario lo pida, etc.).

## Arquitectura

```
jh-script.exe  (consola)   ──┐
jh-scriptw.exe (sin consola) ├── \\.\pipe\jh-script-<sesión> ──► agente (jh-scriptw agent)
accesos directos ────────────┘     JSON por línea                ├─ bandeja + atajo global
                                                                 ├─ modo privacidad (hilo + escritorio privado)
                                                                 └─ apagado (shutdown.exe) + ventanita
```

- Un servicio de Windows real corre en la sesión 0 y no puede dibujar en tu pantalla ni leer tu teclado; por eso el
  "servicio" es un agente de usuario que arranca al iniciar sesión.
- El apagado usa `shutdown.exe /s /t N`: se cumple aunque el agente muera y no requiere administrador.
- Archivos: `src/agent/` (agente, bandeja, privacidad, apagado, ventanita), `src/ipc.rs` (protocolo),
  `src/cli.rs` + `src/app.rs` (CLI), `src/install.rs`, `src/doctor.rs`.

## Desarrollo

```powershell
cargo test
cargo clippy --all-targets
# Agente en modo prueba: no ejecuta shutdown.exe (solo lo registra en el log)
.\target\debug\jh-scriptw.exe agent --dry-run     # o JH_SCRIPT_DRY_RUN=1
```

Log del agente: `%LOCALAPPDATA%\jh-script\agent.log`.

Soporte para Linux: ver [docs/issues/001-soporte-linux.md](../docs/issues/001-soporte-linux.md).
