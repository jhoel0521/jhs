//! Versiones del programa y de la skill para agentes de IA.
//!
//! La skill (`skills/jhs/SKILL.md`) declara en su frontmatter la versión del
//! programa para la que está escrita (`metadata.version`). Un test garantiza
//! que coincide con la del crate, así cada release publica una skill y un
//! binario emparejados; el CLI compara ambas y avisa si una quedó atrás.

use std::cmp::Ordering;

/// Skill empaquetada en el binario (la misma que publica el repo).
pub const SKILL: &str = include_str!("../../skills/jhs/SKILL.md");

/// Repositorio `owner/name` (del campo `repository` de Cargo.toml).
pub fn repo_slug() -> &'static str {
    env!("CARGO_PKG_REPOSITORY").trim_start_matches("https://github.com/").trim_end_matches('/')
}

/// Versión semántica simple `MAYOR.MENOR.PARCHE` (ignora sufijos `-beta`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Version(pub u32, pub u32, pub u32);

impl Version {
    pub fn parse(s: &str) -> Option<Version> {
        let s = s.trim().trim_start_matches('v');
        let core = s.split(['-', '+']).next()?;
        let mut it = core.split('.').map(|p| p.parse::<u32>().ok());
        let v = Version(it.next()??, it.next().flatten().unwrap_or(0), it.next().flatten().unwrap_or(0));
        Some(v)
    }

    pub fn current() -> Version {
        Version::parse(crate::VERSION).expect("versión del crate válida")
    }
}

impl std::fmt::Display for Version {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}.{}", self.0, self.1, self.2)
    }
}

/// `metadata.version` del frontmatter de un SKILL.md.
pub fn skill_version(skill_md: &str) -> Option<Version> {
    let body = skill_md.strip_prefix("---")?;
    let front = &body[..body.find("\n---")?];
    let mut in_metadata = false;
    for line in front.lines() {
        if line.trim_end() == "metadata:" {
            in_metadata = true;
            continue;
        }
        if in_metadata {
            if !line.starts_with(' ') {
                in_metadata = false;
            } else if let Some(v) = line.trim().strip_prefix("version:") {
                return Version::parse(v.trim().trim_matches(['"', '\'']));
            }
        }
    }
    None
}

/// Relación entre la skill instalada y el programa.
pub fn compare_skill(installed: Option<Version>) -> &'static str {
    match installed.map(|v| v.cmp(&Version::current())) {
        None => "sin-version",
        Some(Ordering::Equal) => "al-dia",
        Some(Ordering::Less) => "skill-desactualizada",
        Some(Ordering::Greater) => "programa-desactualizado",
    }
}

/// Nombre del paquete de una release para esta plataforma
/// (`jhs-0.2.0-linux-x86_64.tar.gz`, `jhs-0.2.0-windows-x86_64.zip`).
pub fn asset_name(version: Version) -> String {
    let os = std::env::consts::OS;
    let ext = if os == "windows" { "zip" } else { "tar.gz" };
    format!("jhs-{version}-{os}-{}.{ext}", std::env::consts::ARCH)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parsea_versiones() {
        assert_eq!(Version::parse("v1.2.3"), Some(Version(1, 2, 3)));
        assert_eq!(Version::parse("0.3"), Some(Version(0, 3, 0)));
        assert_eq!(Version::parse("1.0.0-beta.1"), Some(Version(1, 0, 0)));
        assert_eq!(Version::parse("x"), None);
        assert!(Version(0, 10, 0) > Version(0, 9, 9));
    }

    /// La skill del repo debe declarar exactamente la versión del programa.
    #[test]
    fn skill_y_programa_misma_version() {
        assert_eq!(skill_version(SKILL), Some(Version::current()), "actualiza metadata.version en skills/jhs/SKILL.md");
    }

    #[test]
    fn repo_correcto() {
        assert_eq!(repo_slug(), "jhoel0521/jhs");
    }

    #[test]
    fn frontmatter_sin_metadata() {
        assert_eq!(skill_version("---\nname: x\n---\n"), None);
        assert_eq!(skill_version("sin frontmatter"), None);
    }
}
