//! Caso de uso `jhs update`: busca la última release, la descarga, verifica su
//! SHA-256 y ejecuta su `install` (que reemplaza los binarios, reinicia el
//! agente e instala la skill emparejada con esa versión).

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::domain::version::{self, Version};
use crate::ports::{Release, ReleasesPort};

pub const SUMS_FILE: &str = "SHA256SUMS";

/// Información de versión para `update --check` y `doctor`.
pub fn version_info(release: Option<&Release>, skill_installed: Option<Version>) -> Value {
    let current = Version::current();
    json!({
        "programa": current.to_string(),
        "ultima": release.map(|r| r.version.to_string()),
        "hay_actualizacion": release.is_some_and(|r| r.version > current),
        "release": release.map(|r| r.url.clone()),
        "skill_instalada": skill_installed.map(|v| v.to_string()),
        "skill_estado": version::compare_skill(skill_installed),
        "repo": version::repo_slug(),
    })
}

/// Versión de la skill instalada en `path`, si existe.
pub fn installed_skill_version(path: Option<PathBuf>) -> Option<Version> {
    path.and_then(|p| std::fs::read_to_string(p).ok()).and_then(|t| version::skill_version(&t))
}

fn sha256_hex(path: &Path) -> Result<String, String> {
    let data = std::fs::read(path).map_err(|e| format!("no se pudo leer {}: {e}", path.display()))?;
    Ok(Sha256::digest(&data).iter().map(|b| format!("{b:02x}")).collect())
}

/// Busca `nombre` en un archivo `sha256sum` (`<hex>  <nombre>`).
fn expected_sum(sums: &str, name: &str) -> Option<String> {
    sums.lines().find_map(|l| {
        let mut it = l.split_whitespace();
        let hex = it.next()?;
        let file = it.next()?.trim_start_matches('*');
        (file == name).then(|| hex.to_lowercase())
    })
}

fn find_binary(dir: &Path, name: &str) -> Option<PathBuf> {
    let direct = dir.join(name);
    if direct.is_file() {
        return Some(direct);
    }
    std::fs::read_dir(dir).ok()?.flatten().map(|e| e.path()).filter(|p| p.is_dir()).find_map(|d| find_binary(&d, name))
}

/// Descarga e instala `release`. Devuelve los pasos realizados.
pub fn apply(port: &dyn ReleasesPort, release: &Release) -> Result<Vec<String>, String> {
    let name = version::asset_name(release.version);
    let find = |n: &str| release.assets.iter().find(|(a, _)| a == n).map(|(_, u)| u.clone());
    let asset_url = find(&name).ok_or_else(|| format!("la release {} no tiene {name}", release.version))?;
    let sums_url = find(SUMS_FILE).ok_or_else(|| format!("la release {} no publica {SUMS_FILE}", release.version))?;

    let work = std::env::temp_dir().join(format!("jhs-update-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&work);
    std::fs::create_dir_all(&work).map_err(|e| format!("no se pudo crear {}: {e}", work.display()))?;
    let result = (|| {
        let mut log = Vec::new();
        let archive = work.join(&name);
        let sums = work.join(SUMS_FILE);
        port.download(&asset_url, &archive)?;
        port.download(&sums_url, &sums)?;
        let sums_text = std::fs::read_to_string(&sums).map_err(|e| e.to_string())?;
        let want = expected_sum(&sums_text, &name).ok_or_else(|| format!("{SUMS_FILE} no incluye {name}"))?;
        let got = sha256_hex(&archive)?;
        if got != want {
            return Err(format!("el SHA-256 de {name} no coincide: se canceló la actualización"));
        }
        log.push(format!("Descargado y verificado {name}"));

        let out_dir = work.join("pkg");
        port.extract(&archive, &out_dir)?;
        let exe_name = if cfg!(windows) { "jhs.exe" } else { "jhs" };
        let exe = find_binary(&out_dir, exe_name).ok_or_else(|| format!("el paquete no trae {exe_name}"))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            for b in ["jhs", "jhsw"] {
                if let Some(p) = exe.parent().map(|d| d.join(b)) {
                    let _ = std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o755));
                }
            }
        }
        let out = Command::new(&exe)
            .args(["install", "--json"])
            .output()
            .map_err(|e| format!("no se pudo ejecutar el instalador nuevo: {e}"))?;
        let resp: Value = serde_json::from_slice(&out.stdout).unwrap_or(Value::Null);
        if resp["ok"].as_bool() != Some(true) {
            let msg = resp["message"].as_str().map(str::to_string).unwrap_or_else(|| {
                String::from_utf8_lossy(&out.stderr).trim().to_string()
            });
            return Err(format!("falló la instalación de la versión {}: {msg}", release.version));
        }
        if let Some(pasos) = resp["data"]["pasos"].as_array() {
            log.extend(pasos.iter().filter_map(|p| p.as_str().map(str::to_string)));
        }
        log.push(format!("Actualizado de {} a {}", Version::current(), release.version));
        Ok(log)
    })();
    let _ = std::fs::remove_dir_all(&work);
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lee_sha256sums() {
        let s = "abc123  jhs-0.2.0-linux-x86_64.tar.gz\nDEF456 *jhs-0.2.0-windows-x86_64.zip\n";
        assert_eq!(expected_sum(s, "jhs-0.2.0-linux-x86_64.tar.gz").as_deref(), Some("abc123"));
        assert_eq!(expected_sum(s, "jhs-0.2.0-windows-x86_64.zip").as_deref(), Some("def456"));
        assert_eq!(expected_sum(s, "otro"), None);
    }
}
