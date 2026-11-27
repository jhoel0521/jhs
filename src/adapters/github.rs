//! Releases de GitHub con `curl` y `tar` (vienen con Ubuntu y con Windows 10+),
//! para no meter un cliente HTTP/TLS en el binario.

use std::path::Path;
use std::process::{Command, Stdio};

use serde_json::Value;

use crate::domain::version::Version;
use crate::ports::{Release, ReleasesPort};

pub struct GitHub {
    pub repo: &'static str,
}

fn curl() -> &'static str {
    if cfg!(windows) { "curl.exe" } else { "curl" }
}

fn tar() -> &'static str {
    if cfg!(windows) { "tar.exe" } else { "tar" }
}

fn run(cmd: &mut Command, what: &str) -> Result<Vec<u8>, String> {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let out = cmd
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("no se pudo ejecutar {what}: {e}"))?;
    if out.status.success() {
        Ok(out.stdout)
    } else {
        Err(format!("{what} falló: {}", String::from_utf8_lossy(&out.stderr).trim()))
    }
}

impl ReleasesPort for GitHub {
    fn latest(&self) -> Result<Release, String> {
        let url = format!("https://api.github.com/repos/{}/releases/latest", self.repo);
        let body = run(
            Command::new(curl()).args([
                "-fsSL",
                "--max-time",
                "20",
                "-H",
                "Accept: application/vnd.github+json",
                "-H",
                concat!("User-Agent: jhs/", env!("CARGO_PKG_VERSION")),
                &url,
            ]),
            "curl",
        )
        .map_err(|e| {
            if e.contains("404") {
                format!("{} no tiene releases publicadas todavía (o el repositorio no existe)", self.repo)
            } else {
                format!("no se pudo consultar {url}: {e}")
            }
        })?;
        let v: Value = serde_json::from_slice(&body).map_err(|e| format!("respuesta inválida de GitHub: {e}"))?;
        let tag = v["tag_name"].as_str().ok_or("la release no tiene tag_name")?;
        let version = Version::parse(tag).ok_or_else(|| format!("tag de versión inválido: {tag}"))?;
        let assets = v["assets"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|x| Some((x["name"].as_str()?.to_string(), x["browser_download_url"].as_str()?.to_string())))
                    .collect()
            })
            .unwrap_or_default();
        Ok(Release { version, url: v["html_url"].as_str().unwrap_or_default().to_string(), assets })
    }

    fn download(&self, url: &str, dest: &Path) -> Result<(), String> {
        run(
            Command::new(curl()).args(["-fsSL", "--max-time", "300", "-o"]).arg(dest).arg(url),
            "curl",
        )
        .map(|_| ())
    }

    fn extract(&self, archive: &Path, dest: &Path) -> Result<(), String> {
        std::fs::create_dir_all(dest).map_err(|e| e.to_string())?;
        run(Command::new(tar()).arg("-xf").arg(archive).arg("-C").arg(dest), "tar").map(|_| ())
    }
}
