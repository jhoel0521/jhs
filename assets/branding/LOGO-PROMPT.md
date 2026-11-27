# Logo de jhs: prompt y entregables

`logo.svg` es un logo **provisional** (monitor con cerradura). Este documento sirve para pedirle a un agente de IA
(o a un diseñador) el logo definitivo.

## Prompt (copiar y pegar)

> Diseña el logo de **jhs**, una herramienta de línea de comandos (CLI) de código abierto para Windows y Linux.
> Su función principal es un **"modo privacidad"**: oculta la pantalla y bloquea teclado y mouse mientras los
> programas siguen corriendo por detrás. También programa el apagado del PC, y la controlan desarrolladores y
> agentes de IA desde la terminal. El nombre viene de las iniciales del autor ("jh") + "s" de *scripts*.
>
> **Concepto**: un ícono simple y geométrico que combine *pantalla/monitor oculto* con *seguridad/bloqueo*
> (cerradura, ojo cerrado o cortina), con un guiño a la terminal (prompt `>_` o tipografía monoespaciada).
> Debe transmitir: privado, confiable, liviano, técnico. Nada recargado.
>
> **Estilo**: flat/vectorial, formas gruesas, sin degradados finos ni texto pequeño dentro del ícono. Tiene que
> reconocerse a **16×16 px** (bandeja del sistema) y verse bien en fondo claro y oscuro.
>
> **Paleta**: base índigo/violeta oscuro (#312e81 → #4f46e5) con acento claro (#a5b4fc / #e0e7ff). Puedes
> proponer otra paleta con 2–3 colores como máximo.
>
> **Entregables**: (1) símbolo solo, en un cuadrado redondeado (squircle) de 256×256 con margen interno del 10 %;
> (2) versión monocroma en una sola tinta para la bandeja de Linux (símbolo blanco sobre transparente);
> (3) wordmark horizontal "jhs" en minúsculas con tipografía monoespaciada o geométrica junto al símbolo;
> (4) todo en **SVG vectorial limpio** (sin imágenes incrustadas, sin texto sin convertir a trazos).

## Archivos y dimensiones que necesita el repo

| Archivo | Tamaño | Uso |
|---|---|---|
| `assets/branding/logo.svg` | vectorial, viewBox 256×256 | README, GitHub Page, fuente de todo lo demás |
| `assets/branding/logo-mono.svg` | vectorial, 1 color, fondo transparente | Icono de bandeja en Linux (estilo "symbolic") |
| `assets/branding/wordmark.svg` | vectorial, ~4:1 (p. ej. 1024×256) | Cabecera del README y de la web |
| `assets/branding/logo-512.png` | 512×512, PNG transparente | Lanzadores `.desktop` de Linux, tiendas |
| `assets/branding/logo-1024.png` | 1024×1024, PNG transparente | Alta resolución / prensa |
| `assets/branding/social-preview.png` | **1280×640**, PNG (< 1 MB) | Settings > Social preview de GitHub y Open Graph de la web |
| `docs/favicon.svg` + `docs/favicon-32.png` | vectorial y 32×32 | Favicon de la GitHub Page |
| `assets/icon.ico` | multi-tamaño: **16, 20, 24, 32, 40, 48, 64, 256** | Ejecutables, bandeja y accesos directos de Windows |

Para el `.ico`, partiendo del SVG (requiere ImageMagick e Inkscape o rsvg-convert):

```bash
for s in 16 20 24 32 40 48 64 256; do rsvg-convert -w $s -h $s assets/branding/logo.svg -o /tmp/jhs-$s.png; done
magick /tmp/jhs-{16,20,24,32,40,48,64,256}.png assets/icon.ico
```

A 16 y 20 px conviene un dibujo simplificado (sin pie del monitor, cerradura más gruesa) en vez del reescalado.
