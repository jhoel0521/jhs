<p align="center">
  <img src="assets/branding/logo.svg" alt="jhs" width="128" height="128">
</p>

<h1 align="center">jhs</h1>

<p align="center">
  <b>Utilidades de escritorio para Windows y Linux, controlables por CLI y por agentes de IA.</b><br>
  Modo privacidad sin cerrar nada · Apagado programado · Pensado para Claude Code y Remote Control.
</p>

<p align="center">
  <a href="https://github.com/jhoel0521/jhs/actions/workflows/ci.yml"><img alt="CI" src="https://github.com/jhoel0521/jhs/actions/workflows/ci.yml/badge.svg"></a>
  <a href="https://github.com/jhoel0521/jhs/releases/latest"><img alt="Release" src="https://img.shields.io/github/v/release/jhoel0521/jhs"></a>
  <a href="LICENSE"><img alt="Licencia Apache-2.0" src="https://img.shields.io/badge/licencia-Apache--2.0-blue"></a>
  <a href="https://jhoel0521.github.io/jhs/"><img alt="Web" src="https://img.shields.io/badge/web-jhoel0521.github.io%2Fjhs-informational"></a>
</p>

---

`jhs` (de *jh scripts*) es un agente ligero que corre en tu sesión y un CLI corto para hablarle:

- **Modo privacidad**: oculta la pantalla y bloquea teclado y mouse mientras tus programas (servidores de
  desarrollo, pruebas e2e, descargas…) **siguen corriendo**. Ideal para irte del PC y controlarlo a distancia.
- **Apagado programado**: en N minutos o a una hora exacta, con avisos y cancelación.
- **Para agentes de IA**: salida `--json`, códigos de salida estables y una [skill](skills/jhs/SKILL.md)
  versionada junto con el programa.

```bash
jhs --off-screen          # modo privacidad
jhs --on-screen           # volver
jhs shutdown in 90        # apagar en 90 minutos
jhs cancelar-apagado
jhs status --json
```

## Instalación

### Desde una release (recomendado)

Descarga el paquete de tu sistema en [Releases](https://github.com/jhoel0521/jhs/releases/latest), descomprímelo
y ejecuta `install`:

```bash
# Linux
tar -xzf jhs-*-linux-x86_64.tar.gz && ./jhs-*/jhs install
```

```powershell
# Windows (PowerShell)
Expand-Archive jhs-*-windows-x86_64.zip .; .\jhs-*\jhs.exe install
```

Después, para estar siempre al día: `jhs update` (comprueba con `jhs update --check`).

### Compilando

Requiere Rust 1.89 o superior (en Windows, toolchain MSVC y Visual Studio Build Tools).

```bash
git clone https://github.com/jhoel0521/jhs && cd jhs
cargo build --release
./target/release/jhs install        # Windows: .\target\release\jhs.exe install
```

### Skill para agentes de IA

`jhs install` ya instala la skill de Claude Code con la misma versión del programa. También puedes
instalarla o actualizarla con [`npx skills`](https://skills.sh):

```bash
npx skills add jhoel0521/jhs --skill jhs -g -a claude-code -y   # instalar
npx skills update jhs -g -y                                     # actualizar
```

La skill declara para qué versión del programa está escrita (`metadata.version`); `jhs doctor` y
`jhs update --check` avisan si la skill o el programa quedaron atrás.

## Qué hace `install`

| | Windows | Linux (Ubuntu/GNOME) |
|---|---|---|
| Binarios | `%LOCALAPPDATA%\Programs\jhs\` + PATH del usuario | `~/.local/bin/` |
| Autoarranque | `HKCU\...\Run` | `~/.config/autostart/jhs.desktop` |
| Accesos directos | Menú Inicio y Escritorio | Menú de aplicaciones (`~/.local/share/applications`) |
| Atajo global | `RegisterHotKey` | Atajo personalizado de GNOME (gsettings) |
| Configuración | `%APPDATA%\jhs\config.toml` | `~/.config/jhs/config.toml` |
| Datos y log | `%LOCALAPPDATA%\jhs\` | `~/.local/state/jhs/` |
| Skill | `~\.claude\skills\jhs\` | `~/.claude/skills/jhs/` |

Nada requiere administrador ni `sudo`. `jhs uninstall` lo revierte todo, salvo la configuración y el log.
Si venías de la versión anterior (`jh-script`), `install` migra la configuración y limpia lo viejo.

## Uso

| Qué | Comando | Alias |
|---|---|---|
| Activar modo privacidad | `jhs privacy on` | `--off-screen`, `privacidad on`, `apagar-pantalla` |
| …solo por N segundos | `jhs privacy on --for 600` | |
| Desactivar modo privacidad | `jhs privacy off` | `--on-screen`, `privacidad off`, `encender-pantalla` |
| Alternar | `jhs privacy toggle` | |
| Apagar en N minutos | `jhs shutdown in 90` | `programar-apagado 90` |
| Apagar a una hora | `jhs shutdown at 23:30` | `programar-apagado 23:30` |
| Cancelar apagado | `jhs shutdown cancel` | `cancelar-apagado` |
| Ventanita de apagado | `jhs shutdown ui` | |
| Estado | `jhs status` | `estado` |
| Diagnóstico | `jhs doctor` | `diagnostico` |
| ¿Está libre un atajo? | `jhs hotkey test "Ctrl+Alt+F12"` | |
| Actualizar | `jhs update` · `jhs update --check` | `actualizar` |
| Agente | `jhs agent` · `agent stop` · `agent reload` | |

Opción global `--json`. Códigos de salida: `0` ok · `1` error · `2` agente no disponible · `3` argumentos
inválidos. Si el agente no está corriendo, `privacy on`, `shutdown …` y `cancelar-apagado` lo inician solos.

La **bandeja** (Windows, y Linux con la extensión AppIndicator que Ubuntu trae activada) ofrece: modo privacidad,
Programar apagado ▸ 15/30/60/90/120 min / Personalizado…, Cancelar apagado, Diagnóstico, Iniciar con la sesión,
Abrir/Recargar configuración y Salir.

## Modo privacidad: cómo funciona

**Windows**: un escritorio privado de Win32 (`CreateDesktopW` + `SwitchDesktop`), el mismo mecanismo de UAC.
Ventana negra por monitor, sin barra de tareas ni Alt+Tab, hooks que tragan toda la entrada y un **combo
secreto** de desbloqueo que ningún otro programa puede escuchar. Si alguien escapa con `Ctrl+Alt+Supr`, el
vigilante lo detecta y **bloquea Windows**.

**Linux**: en Wayland ningún programa normal puede tapar la pantalla ni capturar el teclado (es una garantía de
seguridad), así que `jhs` usa la **pantalla de bloqueo real** de la sesión vía logind (`loginctl lock-session`).
Se sale con tu contraseña o con `jhs privacy off` (desde otra terminal, SSH o un agente de IA). Mientras está
activo se impide la suspensión, y si el agente muere la sesión **sigue bloqueada**. Desarrollado en Ubuntu
26.04 con GNOME 50 (Wayland); otros escritorios que atiendan a logind (KDE, o `xss-lock`) también sirven.

**Limitación común**: las herramientas que mueven el mouse o teclado *reales* o capturan la pantalla no funcionan
mientras está activo. Playwright, Cypress, Selenium y tauri-driver (CDP/WebDriver) siguen funcionando.

## Configuración

```toml
[privacidad]
atajo_activar = "Ctrl+Shift+Q"           # "" = sin atajo global
combo_desbloqueo = "Ctrl+Shift+Q"        # Windows: combo secreto dentro del modo privacidad
al_detectar_escape = "bloquear-windows"  # Windows: o "volver" / "nada"
mantener_pantalla_encendida = true

[apagado]
forzar = true
presets_min = [15, 30, 60, 90, 120]
avisos_min = [10, 5, 1]
```

Tras editar: bandeja > *Recargar configuración* o `jhs agent reload`.

## Arquitectura

Hexagonal: el dominio y los casos de uso no conocen el sistema operativo; cada SO aporta adaptadores.

```
src/
├── domain/        reglas puras: config, combos, horas, apagado, protocolo, versiones
├── ports/         interfaces: entrada (AgentService) y salida (privacidad, apagado, bandeja,
│                  atajo, avisos, autoarranque, releases, Platform para el CLI)
├── application/   CLI (clap), núcleo del agente, actualizador
└── adapters/
    ├── windows/   named pipe, escritorio privado, Shell_NotifyIcon, RegisterHotKey, shutdown.exe
    ├── linux/     socket Unix, logind, ksni (bandeja), gsettings (GNOME), notify-send, zenity
    └── github.rs  releases (curl + tar)
```

```
jhs (CLI) ──► IPC (pipe / socket Unix, JSON por línea) ──► agente ──► puertos ──► adaptadores del SO
```

Para llevar `jhs` a otro escritorio (Sway/Hyprland con `ext-session-lock-v1`, KDE…) basta con escribir nuevos
adaptadores; el plan está en las [issues](https://github.com/jhoel0521/jhs/issues).

## Desarrollo

```bash
cargo test
cargo clippy --all-targets
cargo build --no-default-features          # Linux sin icono de bandeja
./target/debug/jhs agent --dry-run         # modo prueba: no apaga de verdad (o JHS_DRY_RUN=1)
```

Publicar una versión: sube `version` en `Cargo.toml` **y** `metadata.version` en `skills/jhs/SKILL.md` (un test
lo exige), y luego `git tag vX.Y.Z && git push origin vX.Y.Z`. El workflow *Release* compila Windows y Linux y
publica los paquetes con `SHA256SUMS`, que es lo que descarga `jhs update`.

## Otras carpetas

| Carpeta | Qué es |
|---|---|
| [skills/jhs/](skills/jhs/) | Skill para agentes de IA (Claude Code y otros vía `npx skills`) |
| [docs/](docs/) | GitHub Page (`index.html`) |
| [assets/](assets/) | Icono del programa y [branding](assets/branding/) |
| [legacy/](legacy/) | Versiones originales en Python, reemplazadas por `jhs` |

## Licencia

[Apache-2.0](LICENSE) © jhoel0521. Puedes usarlo, modificarlo y redistribuirlo (también con fines comerciales)
siempre que conserves el archivo [NOTICE](NOTICE) con la mención al autor y el enlace a este repositorio.
