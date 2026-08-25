# bzz website — getbzz.dev

**Status:** Draft 2026-08-24

**Domain:** `getbzz.dev` (registrado en Cloudflare Registrar; `.dev` fuerza HTTPS).

**Inspiración (no imitación):**

- <https://getslk.sh/> — one-pager de marketing para un TUI: hero + one-liner
  de instalación, métricas, grid de features, sección de keybindings, bloque
  de instalación multi-plataforma.
- <https://github.com/chojs23/concord> — README rico: media inline,
  tabla de keymaps, temas, FAQ de seguridad.

## Estilo propio

bzz no es "otro clon de slk". Diferenciadores de identidad que la página debe
comunicar y que definen su estética:

1. **Human-first + soberanía**: identidad Nostr nativa, llaves en el keychain
   del SO, comunidades aisladas por host. No hay cuenta de un tercero.
2. **Enjambre / colmena**: la metáfora visual es la abeja y el hexágono.
   Paleta base: ámbar/miel (`#FFB000` aprox.) sobre fondo casi negro
   (`#0E0C08`), con acentos del tema por defecto del TUI. Tipografía
   monoespaciada para todo lo técnico; una sans geométrica solo para titulares.
3. **Terminal-honesto**: nada de mockups falsos. Todas las capturas y videos
   provienen del binario real vía la skill `bzz-visual-capture` (sanitizadas).
4. **60 temas**: el sitio tiene un theme-switcher propio que re-tematiza la
   página con 4–6 temas reales de bzz (tokens exportados desde `theme.toml`).
   Es la demo interactiva distintiva: la web se comporta como el TUI.
5. **Bilingüe después**: v1 en inglés; estructura preparada para `es/`.

## Estructura del sitio (v1, one-pager + 2 subpáginas)

```
/                 hero, install one-liner, screenshot/video, features, keys
/install          guía por plataforma + verificación (sha256, SBOM, attestation)
/docs -> GitHub   (enlace, no duplicamos docs en v1)
/install.sh       script de instalación (proxy/copia del bzz-installer.sh del release)
```

### Secciones del one-pager

1. **Hero**: logo, "a human-first terminal client for Buzz",
   `curl -fsSL https://getbzz.dev/install.sh | sh`, botones GitHub / Releases.
2. **Captura viva**: video corto (WebM/MP4) del TUI navegando canales, Inbox
   y cambio de tema; fallback a PNG. Generado con `bzz-visual-capture`.
3. **Métricas honestas**: binario estático Rust, tamaño on-disk, cold start,
   100% keyboard-driven (medir antes de publicar; no inventar números).
4. **Features** (grid, tomado del README): offline-first SQLite, sends
   acknowledged con recovery, Inbox, DMs de workspace, búsqueda FTS5+NIP-50,
   media segura Blossom/imeta, imágenes inline (Kitty/Sixel/iTerm2/half-block),
   60 temas, Vim-style, identidad Nostr en keychain.
5. **Seguridad como feature de primera clase**: sección propia (a diferencia
   de slk): NIP-49 backups, media safety, sin credenciales en argv/env.
6. **Keybindings**: tabla compacta estilo cheat-sheet (Movement / Leader /
   Composer / Inbox), fuente: README + keymap efectivo.
7. **Install**: tabs quick / macOS / Linux (tar.xz) / Windows (zip) / brew
   (`bzz.rb` ya existe en el release) / cargo. Assets ya publicados por
   release: `bzz-installer.sh`, `bzz-installer.ps1`, archivos por target,
   `sha256.sum`, `bzz.cdx.xml` (SBOM), attestations.
8. **Footer**: MIT OR Apache-2.0, disclaimer "proyecto independiente,
   compatible con Buzz (`block/buzz`), no afiliado a Block".

## Stack técnico

- **Sitio estático** (Astro o HTML/CSS artesanal — decidir en implementación;
  sesgo hacia artesanal: una página, cero framework, presupuesto < 100 KB
  sin contar media).
- **Deploy**: Cloudflare Workers static assets (`wrangler deploy`) sobre la
  zona `getbzz.dev` ya presente en la cuenta.
- **`/install.sh`**: Worker route que sirve el `bzz-installer.sh` del último
  release de GitHub (redirect 302 a
  `github.com/arpagon/bzz/releases/latest/download/bzz-installer.sh`
  en v1; copia cacheada solo si medimos fricción).
- **Repo**: directorio `site/` en este repo (o repo `arpagon/getbzz.dev`
  separado — decidir; sesgo: `site/` aquí para versionar junto al producto).

## Pipeline de assets

### Capturas y video reales

- Skill `bzz-visual-capture`: capturas sanitizadas del TUI en terminal con
  gráficos (avatares/media), mismas escenas que los releases.
- Escenas mínimas: workspace completo, Inbox, switcher fuzzy, theme picker,
  imagen inline renderizada.

### Imágenes generadas (skill `codex-gpt-image-2`)

Para todo lo que no sea captura real:

- **Logo/marca**: abeja geométrica + hexágono, estilo flat/terminal, ámbar
  sobre oscuro; variantes: icono cuadrado (favicon/social), lockup horizontal.
- **OG image** (1200×630): logo + tagline + un frame del TUI.
- **Texturas hex de fondo** sutiles para secciones (opcional).

Comando base:

```bash
uv run ~/.agents/skills/codex-gpt-image-2/codex-gpt-image-2.py \
  --prompt "<prompt>" --format png
```

Los generados van a `.pi/artifacts/images/` (gitignored); los aprobados se
optimizan (SVG a mano si es posible, o PNG/WebP comprimido) y se copian a
`site/assets/`. Ninguna imagen generada reemplaza capturas reales del TUI.

## SEO / meta

- Title: `bzz — a human-first terminal client for Buzz`.
- Description ≈ README primera frase; OG/Twitter cards con la OG image.
- `robots.txt`, sitemap mínimo, canonical.

## Hitos

| Hito | Contenido |
| --- | --- |
| M1 | `site/` con hero + install + features estáticos; deploy a getbzz.dev |
| M2 | `/install.sh` worker route + sección verificación (`sha256`, SBOM) |
| M3 | Video de captura sanitizada + theme-switcher de la página |
| M4 | OG image, favicon, pulido responsive, Lighthouse ≥ 95 |

## Fuera de alcance v1

- Docs duplicadas (viven en GitHub), blog, analytics, newsletter, i18n es/.

## Riesgos

- Números de rendimiento: medir antes de publicar (nada de "blazingly fast"
  sin datos).
- Marca "Buzz": mantener disclaimer de no afiliación con Block.
- El installer de GitHub latest puede cambiar de forma; el redirect 302 evita
  mantener una copia desincronizada.
