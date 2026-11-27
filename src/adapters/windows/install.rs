//! Instalación por usuario (sin admin): copia a `%LOCALAPPDATA%\Programs\jhs`,
//! PATH de usuario, autoarranque (clave Run), accesos directos y skill de Claude.

use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use windows_sys::Win32::UI::WindowsAndMessaging::{
    HWND_BROADCAST, SMTO_ABORTIFHUNG, SendMessageTimeoutW, WM_SETTINGCHANGE,
};

use crate::domain::config::Config;
use crate::adapters::windows::ipc;
use crate::domain::protocol::Request;
use crate::adapters::windows::win::{self, wide};

const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const RUN_VALUE: &str = "jhs";
const ENV_KEY: &str = "Environment";
const CREATE_NO_WINDOW: u32 = 0x0800_0000;
use crate::domain::version::SKILL;

pub fn install_dir() -> PathBuf {
    std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("Programs")
        .join("jhs")
}

fn exe_dir() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(Path::to_path_buf))
        .unwrap_or_else(|| PathBuf::from("."))
}

/// `jhsw.exe` junto al ejecutable actual (o el actual si no existe).
pub fn windowless_exe() -> PathBuf {
    let w = exe_dir().join("jhsw.exe");
    if w.exists() { w } else { std::env::current_exe().unwrap_or(w) }
}

// ─── Autoarranque ────────────────────────────────────────────────────────────

pub fn autostart_command() -> Option<String> {
    win::reg_get_string(RUN_KEY, RUN_VALUE)
}

pub fn autostart_enabled() -> bool {
    autostart_command().is_some()
}

pub fn autostart_target_exists(cmd: &str) -> bool {
    let exe = cmd.trim().strip_prefix('"').and_then(|r| r.split('"').next()).unwrap_or(cmd);
    Path::new(exe).exists()
}

pub fn set_autostart(enable: bool) -> Result<(), String> {
    if enable {
        let installed = install_dir().join("jhsw.exe");
        let exe = if installed.exists() { installed } else { windowless_exe() };
        win::reg_set_string(RUN_KEY, RUN_VALUE, &format!("\"{}\" agent", exe.display()), false)
    } else {
        win::reg_delete_value(RUN_KEY, RUN_VALUE)
    }
}

// ─── PATH de usuario ─────────────────────────────────────────────────────────

fn user_path() -> String {
    win::reg_get_string(ENV_KEY, "Path").unwrap_or_default()
}

fn same_dir(a: &str, b: &Path) -> bool {
    let a = a.trim().trim_end_matches('\\');
    let b = b.to_string_lossy();
    a.eq_ignore_ascii_case(b.trim_end_matches('\\'))
}

pub fn in_user_path() -> bool {
    let dir = install_dir();
    user_path().split(';').any(|p| same_dir(p, &dir))
}

fn broadcast_env_change() {
    let env = wide("Environment");
    let mut res = 0usize;
    unsafe {
        SendMessageTimeoutW(HWND_BROADCAST, WM_SETTINGCHANGE, 0, env.as_ptr() as isize, SMTO_ABORTIFHUNG, 2000, &mut res);
    }
}

fn add_to_path(dir: &Path) -> Result<bool, String> {
    let current = user_path();
    if current.split(';').any(|p| same_dir(p, dir)) {
        return Ok(false);
    }
    let new = if current.trim().is_empty() {
        dir.display().to_string()
    } else {
        format!("{};{}", current.trim_end_matches(';'), dir.display())
    };
    win::reg_set_string(ENV_KEY, "Path", &new, true)?;
    broadcast_env_change();
    Ok(true)
}

fn remove_from_path(dir: &Path) -> Result<bool, String> {
    let current = user_path();
    let parts: Vec<&str> = current.split(';').filter(|p| !p.is_empty()).collect();
    let kept: Vec<&str> = parts.iter().copied().filter(|p| !same_dir(p, dir)).collect();
    if kept.len() == parts.len() {
        return Ok(false);
    }
    win::reg_set_string(ENV_KEY, "Path", &kept.join(";"), true)?;
    broadcast_env_change();
    Ok(true)
}

// ─── Accesos directos (vía WScript.Shell) ────────────────────────────────────

fn powershell(script: &str) -> Result<(), String> {
    let out = Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-Command", script])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|e| format!("no se pudo ejecutar PowerShell: {e}"))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

fn ps_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

fn create_shortcuts(exe: &Path) -> Result<(), String> {
    let exe = ps_quote(&exe.display().to_string());
    let script = format!(
        r#"$ErrorActionPreference = 'Stop'
$sh = New-Object -ComObject WScript.Shell
$menu = Join-Path ([Environment]::GetFolderPath('Programs')) 'jhs'
New-Item -ItemType Directory -Force -Path $menu | Out-Null
$desk = [Environment]::GetFolderPath('Desktop')
function Make($path, $arguments, $desc) {{
  $l = $sh.CreateShortcut($path)
  $l.TargetPath = {exe}
  $l.Arguments = $arguments
  $l.IconLocation = {exe} + ',0'
  $l.Description = $desc
  $l.WorkingDirectory = Split-Path {exe}
  $l.Save()
}}
Make (Join-Path $menu 'Modo privacidad.lnk') 'privacy on' 'Oculta todas las pantallas y bloquea teclado y mouse'
Make (Join-Path $menu 'Programar apagado.lnk') 'shutdown ui' 'Programar o cancelar el apagado del PC'
Make (Join-Path $desk 'Modo privacidad.lnk') 'privacy on' 'Oculta todas las pantallas y bloquea teclado y mouse'
"#
    );
    powershell(&script)
}

fn remove_shortcuts() -> Result<(), String> {
    powershell(
        r#"$menu = Join-Path ([Environment]::GetFolderPath('Programs')) 'jhs'
Remove-Item -Recurse -Force -ErrorAction SilentlyContinue $menu
Remove-Item -Force -ErrorAction SilentlyContinue (Join-Path ([Environment]::GetFolderPath('Desktop')) 'Modo privacidad.lnk')"#,
    )
}

// ─── Skill para Claude Code ──────────────────────────────────────────────────

pub fn skill_path() -> Option<PathBuf> {
    let claude = PathBuf::from(std::env::var_os("USERPROFILE")?).join(".claude");
    claude.is_dir().then(|| claude.join("skills").join("jhs").join("SKILL.md"))
}

// ─── Agente ──────────────────────────────────────────────────────────────────

fn stop_agent() {
    if ipc::call(&Request::Stop).is_ok() {
        let t = Instant::now();
        while ipc::agent_running() && t.elapsed() < Duration::from_secs(5) {
            std::thread::sleep(Duration::from_millis(150));
        }
        // Margen para que el proceso termine y libere el .exe.
        std::thread::sleep(Duration::from_millis(400));
    }
}

/// Lanza el agente desacoplado (fuera del job del proceso padre si se puede).
pub fn spawn_agent(exe: &Path) -> std::io::Result<()> {
    const DETACHED_PROCESS: u32 = 0x0000_0008;
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
    const CREATE_BREAKAWAY_FROM_JOB: u32 = 0x0100_0000;
    let base = DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP;
    let mk = |flags: u32| {
        let mut c = Command::new(exe);
        c.arg("agent")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .creation_flags(flags);
        if let Some(d) = exe.parent() {
            c.current_dir(d);
        }
        c
    };
    match mk(base | CREATE_BREAKAWAY_FROM_JOB).spawn() {
        Ok(_) => Ok(()),
        Err(_) => mk(base).spawn().map(|_| ()),
    }
}

/// Espera a que el agente responda (máx. `timeout`).
pub fn wait_agent(timeout: Duration) -> bool {
    let t = Instant::now();
    while t.elapsed() < timeout {
        if ipc::agent_running() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(120));
    }
    false
}

// ─── install / uninstall ─────────────────────────────────────────────────────

/// Restos de la versión anterior (`jh-script`): agente, autoarranque, PATH,
/// accesos directos y skill. La configuración se migra en `domain::paths`.
fn remove_legacy() -> Vec<String> {
    let mut log = Vec::new();
    let old_dir = install_dir().with_file_name(crate::domain::paths::LEGACY_NAME);
    let old_exe = old_dir.join("jh-script.exe");
    if old_exe.exists() {
        let _ = Command::new(&old_exe).args(["agent", "stop"]).creation_flags(CREATE_NO_WINDOW).output();
    }
    if win::reg_get_string(RUN_KEY, crate::domain::paths::LEGACY_NAME).is_some() {
        let _ = win::reg_delete_value(RUN_KEY, crate::domain::paths::LEGACY_NAME);
        log.push("Quitado el autoarranque de la versión anterior (jh-script)".into());
    }
    if remove_from_path(&old_dir).unwrap_or(false) {
        log.push(format!("Quitado {} del PATH", old_dir.display()));
    }
    let _ = powershell(
        r#"$menu = Join-Path ([Environment]::GetFolderPath('Programs')) 'jh-script'
Remove-Item -Recurse -Force -ErrorAction SilentlyContinue $menu"#,
    );
    if let Some(old_skill) = skill_path().and_then(|p| p.parent()?.parent().map(|d| d.join("jh-script")))
        && old_skill.is_dir()
    {
        let _ = std::fs::remove_dir_all(&old_skill);
    }
    if old_dir.is_dir() && std::fs::remove_dir_all(&old_dir).is_ok() {
        log.push(format!("Borrada la instalación anterior {}", old_dir.display()));
    }
    log
}

pub fn install() -> Result<Vec<String>, String> {
    let mut log = Vec::new();
    let src = exe_dir();
    let dest = install_dir();
    for name in ["jhs.exe", "jhsw.exe"] {
        if !src.join(name).exists() {
            return Err(format!("falta {} junto a este ejecutable", name));
        }
    }

    stop_agent();
    if !same_dir(&src.display().to_string(), &dest) {
        std::fs::create_dir_all(&dest).map_err(|e| format!("no se pudo crear {}: {e}", dest.display()))?;
        for name in ["jhs.exe", "jhsw.exe"] {
            // Un .exe en ejecución (p. ej. el `jhs update` que lanzó esta
            // instalación) no se puede sobrescribir, pero sí renombrar.
            let target = dest.join(name);
            let old = target.with_extension("exe.old");
            let _ = std::fs::remove_file(&old);
            if target.exists() {
                let _ = std::fs::rename(&target, &old);
            }
            std::fs::copy(src.join(name), &target)
                .map_err(|e| format!("no se pudo copiar {name}: {e} (¿el agente sigue abierto?)"))?;
        }
        log.push(format!("Copiado a {}", dest.display()));
    } else {
        log.push(format!("Ya está en {}", dest.display()));
    }

    if add_to_path(&dest)? {
        log.push("Añadido al PATH del usuario (abre una terminal nueva)".into());
    } else {
        log.push("Ya estaba en el PATH del usuario".into());
    }

    set_autostart(true)?;
    log.push("Autoarranque al iniciar sesión configurado".into());

    log.extend(remove_legacy());

    match create_shortcuts(&dest.join("jhsw.exe")) {
        Ok(()) => log.push("Accesos directos: Menú Inicio > jhs y Escritorio > \"Modo privacidad\"".into()),
        Err(e) => log.push(format!("Aviso: no se pudieron crear los accesos directos: {e}")),
    }

    Config::ensure_file();
    log.push(format!("Configuración: {}", crate::domain::config::config_path().display()));

    if let Some(p) = skill_path() {
        if let Some(dir) = p.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        match std::fs::write(&p, SKILL) {
            Ok(()) => log.push(format!("Skill de Claude Code instalada: {}", p.display())),
            Err(e) => log.push(format!("Aviso: no se pudo instalar la skill de Claude: {e}")),
        }
    }

    spawn_agent(&dest.join("jhsw.exe")).map_err(|e| format!("no se pudo iniciar el agente: {e}"))?;
    if wait_agent(Duration::from_secs(4)) {
        log.push("Agente iniciado".into());
    } else {
        log.push("Aviso: el agente no respondió todavía; se iniciará al volver a iniciar sesión".into());
    }
    Ok(log)
}

pub fn uninstall() -> Result<Vec<String>, String> {
    let mut log = Vec::new();
    stop_agent();
    log.push("Agente detenido".into());
    set_autostart(false)?;
    log.push("Autoarranque eliminado".into());
    let dest = install_dir();
    if remove_from_path(&dest)? {
        log.push("Quitado del PATH del usuario".into());
    }
    if let Err(e) = remove_shortcuts() {
        log.push(format!("Aviso: accesos directos: {e}"));
    } else {
        log.push("Accesos directos eliminados".into());
    }
    if let Some(p) = skill_path().filter(|p| p.exists()) {
        let _ = std::fs::remove_dir_all(p.parent().unwrap_or(&p));
        log.push("Skill de Claude Code eliminada".into());
    }
    if dest.exists() {
        let running_from_dest = same_dir(&exe_dir().display().to_string(), &dest);
        if running_from_dest {
            // No se puede borrar el .exe en uso: se borra al terminar este proceso.
            let cmd = format!("ping -n 3 127.0.0.1 >nul & rmdir /s /q \"{}\"", dest.display());
            let _ = Command::new("cmd.exe").args(["/c", &cmd]).creation_flags(CREATE_NO_WINDOW | 0x8).spawn();
            log.push(format!("{} se borrará en unos segundos", dest.display()));
        } else {
            match std::fs::remove_dir_all(&dest) {
                Ok(()) => log.push(format!("Borrado {}", dest.display())),
                Err(e) => log.push(format!("Aviso: no se pudo borrar {}: {e}", dest.display())),
            }
        }
    }
    log.push(format!(
        "Se conservan la configuración ({}) y los datos ({})",
        crate::domain::config::config_dir().display(),
        crate::domain::state::data_dir().display()
    ));
    Ok(log)
}
