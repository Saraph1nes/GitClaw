#!/usr/bin/env node
/**
 * CI helper: publishes each platform-specific npm package.
 * Run from the repo root after downloading build artifacts to ./dist/
 */
const { execSync } = require("child_process");
const fs = require("fs");
const path = require("path");

const VERSION = process.env.VERSION;
if (!VERSION) {
  console.error("VERSION env var is required");
  process.exit(1);
}

const PACKAGES = [
  { name: "gitclaw-linux-x64",   artifact: "gitclaw-linux-x64",   os: "linux",  cpu: "x64"   },
  { name: "gitclaw-linux-arm64",  artifact: "gitclaw-linux-arm64",  os: "linux",  cpu: "arm64" },
  { name: "gitclaw-darwin-x64",   artifact: "gitclaw-darwin-x64",   os: "darwin", cpu: "x64"   },
  { name: "gitclaw-darwin-arm64", artifact: "gitclaw-darwin-arm64", os: "darwin", cpu: "arm64" },
  { name: "gitclaw-win32-x64",    artifact: "gitclaw-win32-x64",    os: "win32",  cpu: "x64",  ext: ".exe" },
];

const distDir = path.resolve(__dirname, "..", "dist");

for (const pkg of PACKAGES) {
  const ext = pkg.ext || "";
  const src = path.join(distDir, `${pkg.artifact}${ext}`);

  if (!fs.existsSync(src)) {
    console.warn(`[skip] artifact not found: ${src}`);
    continue;
  }

  const pkgDir = path.join(distDir, "npm", pkg.name);
  const binDir = path.join(pkgDir, "bin");
  fs.mkdirSync(binDir, { recursive: true });

  // Copy binary
  const binDest = path.join(binDir, `gitclaw${ext}`);
  fs.copyFileSync(src, binDest);
  if (!ext) fs.chmodSync(binDest, 0o755);

  // Write package.json
  const pkgJson = {
    name: pkg.name,
    version: VERSION,
    description: `gitclaw binary for ${pkg.os}-${pkg.cpu}`,
    license: "MIT",
    os: [pkg.os],
    cpu: [pkg.cpu],
    main: `bin/gitclaw${ext}`,
  };
  fs.writeFileSync(path.join(pkgDir, "package.json"), JSON.stringify(pkgJson, null, 2));

  // Publish
  console.log(`Publishing ${pkg.name}@${VERSION} …`);
  execSync("npm publish --access public", { cwd: pkgDir, stdio: "inherit" });
}

console.log("All platform packages published.");
