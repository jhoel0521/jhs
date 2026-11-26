---
name: jh-script
description: Controla el PC Windows del usuario con jh-script — activar/desactivar el modo privacidad (pantallas negras y teclado/mouse bloqueados mientras sus programas y pruebas siguen corriendo) y programar o cancelar el apagado. Úsala cuando el usuario pida ocultar o mostrar la pantalla, "modo privacidad", "off-screen", bloquear el PC sin cerrar nada, o programar/cancelar un apagado, también desde Remote Control (/rc).
---

# jh-script

`jh-script` es un CLI que habla con un agente en segundo plano del usuario. Funciona desde cualquier
terminal (PowerShell o Bash), también cuando el usuario controla el PC por Remote Control.

## Reglas

1. Empieza siempre por el estado: `jh-script status --json`.
2. Usa `--json` y revisa `ok` y el código de salida: 0 ok · 1 error · 2 agente no disponible · 3 argumentos inválidos.
3. **Nunca programes un apagado si el usuario no lo pidió explícitamente**, y confirma la hora antes si hay
   ambigüedad. Si te equivocas, cancélalo de inmediato con `jh-script cancelar-apagado`.
4. Activar el modo privacidad deja al usuario sin pantalla ni teclado en el PC: hazlo solo si lo pide.
5. Si el usuario está lejos y quiere ver la pantalla, `jh-script privacy off` la devuelve (aunque el agente
   no responda, el CLI intenta un rescate directo).

## Comandos

| Qué | Comando |
|---|---|
| Estado completo | `jh-script status --json` |
| Activar modo privacidad | `jh-script privacy on --json` |
| Activar por N segundos | `jh-script privacy on --for 600 --json` |
| Desactivar modo privacidad | `jh-script privacy off --json` |
| Apagar en N minutos | `jh-script shutdown in 90 --json` |
| Apagar a una hora | `jh-script shutdown at 23:30 --json` |
| Cancelar apagado | `jh-script shutdown cancel --json` |
| Diagnóstico | `jh-script doctor --json` |
| ¿Está libre un atajo? | `jh-script hotkey test "Ctrl+Alt+F12" --json` |

Alias equivalentes: `--off-screen` / `--on-screen`, `privacidad on|off`, `programar-apagado 60|23:30`,
`cancelar-apagado`, `estado`, `diagnostico`.

## Campos útiles de `status --json`

- `data.privacidad.activa`, `desde`, `hasta`
- `data.privacidad.ultimo_fin.motivo`: `combo` · `cli` · `tiempo` · `escape` (alguien intentó salir y se
  bloqueó Windows) · `error`
- `data.apagado.programado`, `hora`, `en_segundos`, `modo_prueba`
- `data.hotkey.registrada` y `error` (si el atajo lo usa otra app)

Si `status` devuelve código 2, el agente no está corriendo: `jh-script privacy on` lo inicia solo;
también puede iniciarse con `jh-scriptw agent`.
