#!/usr/bin/env node
/**
 * Postinstall: locate the correct platform binary and symlink/copy it to ./bin/gitclaw
 */
const { platform, arch } = process;
const path = require("path");
const fs = require("fs");

const PLATFORMS = {
  "linux-x64": "gitclaw-linux-x64",
  "linux-arm64": "gitclaw-linux-arm64",
  "darwin-x64": "gitclaw-darwin-x64",
  "darwin-arm64": "gitclaw-darwin-arm64",
  "win32-x64": "gitclaw-win32-x64",
};

const key = `${platform}-${arch}`;
const pkg = PLATFORMS[key];

if (!pkg) {
  console.error(`[gitclaw] Unsupported platform: ${key}`);
  process.exit(1);
}

let binPath;
try {
  // Resolve from the optional dependency package
  binPath = require.resolve(`${pkg}/bin/gitclaw${platform === "win32" ? ".exe" : ""}`);
} catch {
  console.error(
    `[gitclaw] Could not find binary for ${key}.\n` +
      `  Make sure the optional dependency "${pkg}" was installed.`
  );
  process.exit(1);
}

const dest = path.join(__dirname, "bin", `gitclaw${platform === "win32" ? ".exe" : ""}`);
fs.mkdirSync(path.dirname(dest), { recursive: true });

// Copy instead of symlink for Windows compatibility
fs.copyFileSync(binPath, dest);
if (platform !== "win32") {
  fs.chmodSync(dest, 0o755);
}

// Write a thin JS wrapper as the "bin" entry so npm can handle Windows .cmd
const wrapper = path.join(__dirname, "bin", "gitclaw");
if (platform !== "win32" && !fs.existsSync(wrapper)) {
  fs.writeFileSync(
    wrapper,
    `#!/usr/bin/env node\nrequire("child_process").spawnSync(${JSON.stringify(dest)}, process.argv.slice(2), { stdio: "inherit" });\n`
  );
  fs.chmodSync(wrapper, 0o755);
}

console.log(`[gitclaw] Installed binary for ${key}`);
