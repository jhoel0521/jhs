---
name: jhs
description: Controla el PC del usuario (Windows o Linux/Ubuntu) con el CLI jhs — activar/desactivar el modo privacidad (pantalla oculta y teclado/mouse bloqueados mientras sus programas y pruebas siguen corriendo) y programar o cancelar el apagado. Úsala cuando el usuario pida ocultar o mostrar la pantalla, "modo privacidad", "off-screen", bloquear el PC sin cerrar nada, o programar/cancelar un apagado, también desde Remote Control (/rc).
metadata:
  version: "0.2.0"
  repository: https://github.com/jhoel0521/jhs
---

# jhs

`jhs` es un CLI que habla con un agente en segundo plano del usuario. Funciona desde cualquier terminal
(PowerShell, Bash), también cuando el usuario controla el PC por Remote Control.

Esta skill está escrita para **jhs v0.2.0** (`metadata.version`). Skill y programa se publican juntos.

## Antes de empezar: versión

1. `jhs update --check --json` compara el programa, la skill instalada y la última release:
   - `data.hay_actualizacion = true` → dile al usuario que hay una versión nueva y, si acepta, `jhs update --json`
     (descarga, verifica SHA-256, reinstala binarios, agente y esta skill).
   - `data.skill_estado = "skill-desactualizada"` → `npx skills update jhs -g -y` (o `jhs install`).
   - `data.skill_estado = "programa-desactualizado"` → `jhs update`.
2. Si `jhs` no existe: indícale al usuario cómo instalarlo (https://github.com/jhoel0521/jhs). No lo descargues
   ni lo ejecutes por tu cuenta sin que lo pida.

## Reglas

1. Empieza siempre por el estado: `jhs status --json`.
2. Usa `--json` y revisa `ok` y el código de salida: 0 ok · 1 error · 2 agente no disponible · 3 argumentos inválidos.
3. **Nunca programes un apagado si el usuario no lo pidió explícitamente**, y confirma la hora antes si hay
   ambigüedad. Si te equivocas, cancélalo de inmediato con `jhs cancelar-apagado`.
4. Activar el modo privacidad deja al usuario sin pantalla ni teclado: hazlo solo si lo pide.
5. Si el usuario está lejos y quiere ver la pantalla, `jhs privacy off` la devuelve (si el agente no responde,
   el CLI intenta un rescate directo).
6. No ejecutes `jhs update` sin que el usuario lo acepte: reemplaza el programa y reinicia el agente.

## Comandos

| Qué | Comando |
|---|---|
| Estado completo | `jhs status --json` |
| Activar modo privacidad | `jhs privacy on --json` |
| Activar por N segundos | `jhs privacy on --for 600 --json` |
| Desactivar modo privacidad | `jhs privacy off --json` |
| Apagar en N minutos | `jhs shutdown in 90 --json` |
| Apagar a una hora | `jhs shutdown at 23:30 --json` |
| Cancelar apagado | `jhs shutdown cancel --json` |
| Diagnóstico | `jhs doctor --json` |
| ¿Está libre un atajo? | `jhs hotkey test "Ctrl+Alt+F12" --json` |
| ¿Hay versión nueva? | `jhs update --check --json` |
| Actualizar | `jhs update --json` |

Alias: `--off-screen` / `--on-screen`, `privacidad on|off`, `programar-apagado 60|23:30`, `cancelar-apagado`,
`estado`, `diagnostico`, `actualizar`.

## Diferencias por sistema

- **Windows**: pantallas negras en un escritorio privado; se sale con el combo secreto (`combo_desbloqueo`) o
  `jhs privacy off`. Si alguien escapa con Ctrl+Alt+Supr, se bloquea Windows.
- **Linux (GNOME/Ubuntu)**: usa la pantalla de bloqueo real de la sesión; se sale con la contraseña del usuario
  o `jhs privacy off`. Si el agente muere, la sesión sigue bloqueada.

## Campos útiles de `status --json`

- `data.agente.so`: `windows` o `linux`.
- `data.privacidad.activa`, `desde`, `hasta`, `mecanismo`.
- `data.privacidad.ultimo_fin.motivo`: `combo` · `cli` · `tiempo` · `desbloqueo` (Linux: contraseña) ·
  `escape` (Windows: alguien intentó salir y se bloqueó Windows) · `error`.
- `data.apagado.programado`, `hora`, `en_segundos`, `modo_prueba`.
- `data.hotkey.registrada` y `error` (si el atajo lo usa otra app).

Si `status` devuelve código 2, el agente no está corriendo: `jhs privacy on` lo inicia solo; también
`jhsw agent`.
