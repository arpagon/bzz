# Planning

Store new product and release plans in `docs/planning/YYYY-MM-DD/`.

# Website (getbzz.dev)

Static site in `site/public/`; see `site/README.md`. Deploy to Cloudflare
Pages with `CLOUDFLARE_ACCOUNT_ID=c2a86d15709455f1a4c6be8b3ef3300e npx
wrangler pages deploy public --project-name getbzz-dev --branch main` from
`site/`. When editing `styles.css`/`app.js`, bump their `?v=N` query in
`index.html` (edge caches 4h). Theme tokens mirror
`src/ui/theme/builtin.rs`; update both together. Media must be sanitized
captures of the real TUI (`bzz-visual-capture`); never invent stats.
