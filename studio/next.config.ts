import type { NextConfig } from "next";

const config: NextConfig = {
  reactStrictMode: true,
  // Studio is often opened as http://127.0.0.1:3000, beside the API on 127.0.0.1:4000.
  allowedDevOrigins: ["127.0.0.1"],
};

export default config;
