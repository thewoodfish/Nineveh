import type { NextConfig } from "next";

const config: NextConfig = {
  reactStrictMode: true,
  // Plain HTML, CSS and JS in `out/`, so the site deploys to anything that serves
  // files: Vercel, Netlify, Cloudflare, a bucket.
  output: "export",
  images: { unoptimized: true },
};

export default config;
