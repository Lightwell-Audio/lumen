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

## Assets

- Fonts: the app's own Inter, Unbounded and Martian Mono, cut to Latin as
  WOFF2. Their licences sit beside them (`*-OFL.txt`).
- Videos: screen recordings of the current build at 150% scale, 1600 px wide
  (H.264 MP4, with a WebP still as the poster).
- Keep the copy product-neutral (see `CLAUDE.md`) and keep the VST and ASIO
  trademark line in the footer.
