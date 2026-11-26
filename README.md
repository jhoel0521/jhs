# scripts

Utilidades personales para el PC.

| Carpeta | Qué es |
|---|---|
| [jh-script/](jh-script/) | Agente para Windows en Rust: **modo privacidad** (oculta todas las pantallas y bloquea teclado y mouse mientras todo sigue corriendo) y **apagado programado**. Se controla con CLI, atajo, bandeja o agentes de IA. |
| [legacy/](legacy/) | Versiones originales en Python (`apagar-pc.pyw`, `bloqueo_pantalla.pyw`), reemplazadas por jh-script. |
| [docs/issues/](docs/issues/) | Análisis y propuestas de trabajo futuro. |

Inicio rápido:

```powershell
cd jh-script
cargo build --release
.\target\release\jh-script.exe install
jh-script --off-screen     # modo privacidad
jh-script --on-screen      # volver
```
