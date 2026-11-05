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
`?direction=studio|playbook|stage` and `?theme=dark|light` choose the look
while the specimens are being judged (USER_DOCS.md §9.5).

## Where things come from

| Folder | What | Written by |
|---|---|---|
| `content/` | The pages, in Markdown | hand |
| `templates/` | Page layouts and shortcodes (`keys`, `shot`, `clip`, `tip`, `tryit`) | hand |
| `static/docs.css`, `static/docs.js` | The look and behaviour; colours and sizes are design tokens only | hand |
| `static/media/` | Screenshots and clips | `cargo test -p lumen-app --test docs_media -- --ignored` with `DOCS_MEDIA=site/docs/static/media` |
| `static/img/` | Lumi, from `docs/brand/` | `cwebp -resize 256 0` |
| `data/commands.json` | Command names, areas, default keys | for the D0 specimens, a script reading `commands/mod.rs`; D0 replaces it with an export from the app |
| `data/tokens.json` | The design tokens | `build.sh`, from `docs/design-system/lumen-tokens.json` (not committed) |
| `data/nav.toml` | The sidebar | hand |
