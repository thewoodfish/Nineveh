# Nineveh's landing page

A static site, separate from Studio so it deploys on its own.

```sh
npm install
npm run dev     # http://localhost:3001
npm run build   # static HTML, CSS and JS in out/
```

`next build` writes `out/`, which any static host serves: Vercel (root directory
`site`), Netlify, Cloudflare Pages, or a bucket behind a CDN. Nothing here talks to a
Nineveh instance — the page is copy and a drawing of the product, so it can't break
when the API does.

What it claims is kept honest by [`docs/landing.md`](../docs/landing.md), which also
lists what isn't built yet. Don't put those on the site.
