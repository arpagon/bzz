# Manual breve de marca de bzz

<img src="../../site/public/assets/logo.svg" alt="Logo de bzz: una celda hexagonal con líneas de terminal" width="96">

**bzz** es un cliente de terminal *human-first* para Buzz. La marca debe sentirse
precisa, cálida y nativa de terminal: tecnología sin perder el lado humano.

## 1. Marca principal

El símbolo combina:

- una **celda hexagonal**, referencia a Buzz y al trabajo compartido;
- tres **líneas de terminal** y un cursor;
- el nombre **`bzz`**, siempre en minúsculas y con tipografía monoespaciada.

La versión final del símbolo no se sustituye por un hexágono genérico. Mantener
sus proporciones, el orden de las líneas y el cursor.

### Espacio y tamaño

- Espacio libre mínimo: **¼ del ancho del símbolo** alrededor.
- Tamaño recomendado en interfaz: **24 px o más**.
- Para 16 px usar [`favicon.svg`](../../site/public/assets/favicon.svg), que ya
  está ajustado a ese contexto.

## 2. Animación terminal

La animación construye el hexágono con glyphs de Nerd Fonts v3 y termina en la
marca original:

```text
nf-md-hexagon_outline  U+F02D9
nf-md-hexagon_slice_1  U+F0AC3
nf-md-hexagon_slice_2  U+F0AC4
nf-md-hexagon_slice_3  U+F0AC5
nf-md-hexagon_slice_4  U+F0AC6
nf-md-hexagon_slice_5  U+F0AC7
nf-md-hexagon_slice_6  U+F0AC8
```

Secuencia copiable:

```text
󰋙 󰫃 󰫄 󰫅 󰫆 󰫇 󰫈
```

- Cada frame dura **170 ms**.
- La marca original permanece **5 s** antes de repetir.
- Animar solamente en superficies protagonistas, como el hero o una pantalla
  de arranque. Navegación, favicon y documentación usan la marca estática.
- Con `prefers-reduced-motion` se muestra únicamente la marca final.
- En web se usan paths SVG equivalentes; no se exige Nerd Fonts al visitante.

## 3. Color

| Token | Hex | Uso |
|---|---|---|
| Honey amber | `#FFB000` | Logo, acción principal y foco |
| Near black | `#0E0C08` | Fondo principal |
| Warm white | `#ECE5D8` | Texto principal |
| Muted | `#8A8272` | Texto secundario |
| Cyan | `#5EC8D8` | Enlaces y acento informativo |

Sobre `#0E0C08`, el ámbar tiene contraste **10.66:1**. No aplicar degradados al
símbolo ni usarlo sobre fondos con poco contraste. Los temas pueden cambiar la
paleta de la interfaz; la identidad base sigue siendo ámbar sobre casi negro.

## 4. Tipografía y voz

- Familia: `ui-monospace`, `SF Mono`, `JetBrains Mono`, Menlo, Consolas o una
  alternativa monoespaciada.
- Nombre: **`bzz`**, nunca `BZZ` como marca principal.
- Tagline: **“a human-first terminal client for Buzz”**.
- Voz: directa, técnica y honesta. Preferir hechos verificables a superlativos.
- No afirmar cifrado E2E para Workspace DMs ni publicar métricas inventadas.

## 5. Capturas y demos

Toda imagen del producto debe ser una captura sanitizada del TUI real mediante
`bzz-visual-capture`, con fixtures generados. No mostrar secretos, datos
personales, escritorios del operador ni estadísticas ficticias.

## 6. Usos incorrectos

No:

- deformar, rotar o redibujar libremente el símbolo;
- cambiar las líneas interiores o eliminar el cursor;
- usar otro hexágono como logo final;
- reproducir la animación continuamente sin la pausa estable;
- usar la marca como si bzz estuviera afiliado o respaldado por Block, Inc.

## Archivos canónicos

- [`logo.svg`](../../site/public/assets/logo.svg) — símbolo estático.
- [`favicon.svg`](../../site/public/assets/favicon.svg) — icono cuadrado.
- [`logo-terminal.txt`](../../site/public/assets/logo-terminal.txt) — secuencia
  Nerd Fonts y codepoints.
- [`app.js`](../../site/public/app.js) — tiempos y frames de la animación web.
- [`mdi-attribution.txt`](../../site/public/assets/mdi-attribution.txt) — origen y
  licencia Apache-2.0 de los frames hexagonales.
