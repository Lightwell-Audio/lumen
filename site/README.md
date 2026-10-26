# Lumen website

The public page at https://gummycookie.github.io/lumen/: one static
`index.html` with its fonts, videos and images under `assets/`. There is no
build step.

## Preview

```sh
python3 -m http.server 8765 --directory site
```

Then open http://localhost:8765/.

## Publish

`.github/workflows/pages.yml` deploys this folder when a change to `site/`
reaches `main`, or when the workflow is run by hand from the Actions tab. The
repository's Pages source must be "GitHub Actions" (Settings > Pages), set
once.

## Search and sharing

- `index.html` carries the title, description, canonical address, Open
  Graph and Twitter card tags, and JSON-LD structured data (the website, the
  application and its source code).
- `og.jpg` is the 1200 × 630 card shown when the link is shared; keep it under
  300 KB so every messaging app shows it.
- `sitemap.xml` lists the page; update its `<lastmod>` when the content
  changes. `robots.txt` points to it, but crawlers read robots.txt only at a
  host's root, so it takes effect once the site has its own domain. Until
  then, submit the sitemap in Google Search Console and Bing Webmaster Tools.
- `favicon.svg`, `apple-touch-icon.png`, `icon-192.png`, `icon-512.png` and
  `site.webmanifest` are the icons for browsers, phones and search results.
- `404.html` is served for any missing path. Its links start with `/lumen/`;
  change them if the site moves to its own domain.

## Assets

- Fonts: the app's own Inter, Unbounded and Martian Mono, cut to Latin as
  WOFF2. Their licences sit beside them (`*-OFL.txt`).
- Videos: screen recordings of the current build at 150% scale, 1600 px wide
  (H.264 MP4, with a WebP still as the poster).
- Keep the copy product-neutral (see `CLAUDE.md`) and keep the VST and ASIO
  trademark line in the footer.
