# Lumen documentation site

The user documentation, built with [Zola](https://www.getzola.org/) 0.22.1
and searched with Pagefind 1.6.0. The strategy is `docs/USER_DOCS.md`
(ADR-0119). Published at `/docs/` beside the landing page.

## Build and look

```bash
site/docs/build.sh /tmp/lumen-site http://localhost:8123/docs
```

```bash
python3 -m http.server 8123 --directory /tmp/lumen-site
```

The landing page's fonts and pictures are read from `../assets`, so serve
the folder that holds `docs/` with `site/assets` beside it (link it in).
`?theme=dark|light` in the address picks the theme, which helps for
screenshots.

## Where things come from

| Folder | What | Written by |
|---|---|---|
| `content/` | The pages, in Markdown | hand |
| `templates/` | Page layouts and shortcodes (`keys`, `shot`, `clip`, `tip`, `tryit`) | hand |
| `static/docs.css`, `static/docs.js` | The look and behaviour; colours and sizes are design tokens only | hand |
| `static/media/` | Screenshots and clips | `cargo test -p lumen-app --test docs_media -- --ignored` with `DOCS_MEDIA=site/docs/static/media` |
| `static/img/` | Lumi, from `docs/brand/` | `cwebp -resize 256 0` |
| `data/commands.json`, `preferences.json`, `controls.json` | Every command (keys on both systems, area, about), Preferences row and bar or menu control | `build.sh`, through `cargo test -p lumen-app --test docs_data -- --ignored` with `DOCS_DATA=site/docs/data` (not committed) |
| `data/tokens.json` | The design tokens | `build.sh`, from `docs/design-system/lumen-tokens.json` (not committed) |
| `data/nav.toml` | The sidebar | hand |
