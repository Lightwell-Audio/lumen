# Lumen documentation site

The user documentation, built with [Zola](https://www.getzola.org/) 0.22.1
and searched with Pagefind 1.6.0. The strategy is `docs/USER_DOCS.md`
(ADR-0119). Published at `/docs/` beside the landing page.

## Build and look

The screenshots and clips come from the app (needs a GPU, a software
Vulkan one is enough, and ffmpeg; on a machine without a display, run it
under `xvfb-run -a`). They are not committed: render them once, and again
when a scene or what it shows changes. Unchanged clips come from
`target/docs-media-cache` (or `DOCS_MEDIA_CACHE`) and are not encoded
again.

```bash
DOCS_MEDIA=site/docs/static/media cargo test -p lumen-app --test docs_media -- --ignored --test-threads=1
```

```bash
site/docs/build.sh /tmp/lumen-site http://localhost:8123/docs
```

```bash
python3 -m http.server 8123 --directory /tmp/lumen-site
```

The landing page's stylesheet (`lumen.css`), fonts and pictures are read
from `../assets`, so serve the folder that holds `docs/` with `site/assets`
beside it (link it in).
`?theme=dark|light` in the address picks the theme, which helps for
screenshots.

## Where things come from

| Folder | What | Written by |
|---|---|---|
| `content/` | The pages, in Markdown | hand |
| `templates/` | Page layouts and shortcodes (`keys`, `shot` with its callouts, `callout`, `clip`, `video`, `tip`, `tryit`) | hand |
| `static/docs.css`, `static/docs.js` | The look and behaviour; colours and sizes are design tokens only | hand |
| `static/media/` | Screenshots, clips and walkthroughs, with callout bounds and cues | the scenes in `crates/lumen-app/tests/docs_media/`, listed in `tests/shared/scenes.rs` (not committed) |
| `static/img/` | Lumi, from `docs/brand/` | `cwebp -resize 256 0` |
| `data/commands.json`, `preferences.json`, `controls.json` | Every command (keys on both systems, area, about), Preferences row and bar or menu control | `build.sh`, through `cargo test -p lumen-app --test docs_data -- --ignored` with `DOCS_DATA=site/docs/data` (not committed) |
| `data/tokens.json` | The design tokens | `build.sh`, from `docs/design-system/lumen-tokens.json` (not committed) |
| `data/nav.toml` | The sidebar | hand |
| `data/also.toml` | The reader's words for a command ("consolidate" for Join into new file), shown and searched on its entry; `docs_coverage` checks each id | hand |

Link between pages as `@/guides/editing/split-a-clip.md#with-the-keyboard`:
the build stops when such a page or anchor does not exist. A plain
`/docs/...` address is not checked.

## Checks and deploy

- `.github/workflows/docs.yml`, on pull requests that touch the site or the
  app: `docs_coverage` and the name tests, every scene once in the dark
  theme without writing anything (no `DOCS_MEDIA`), and the build, which
  stops at a wrong shortcode, missing alt text, an unknown key or a broken
  internal link. The built pages, with the landing page beside them and
  without media, are kept as the run's `docs-site` artifact: unzip it and
  serve the folder with `python3 -m http.server 8123`, then open
  `http://localhost:8123/docs/`.
- `.github/workflows/pages.yml`, on `main`: renders every scene in both
  themes, builds the docs into `docs/` beside the landing page and
  publishes both.
