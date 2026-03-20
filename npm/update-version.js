#!/usr/bin/env node
/**
 * Bumps the version in npm/package.json and all optionalDependencies
 * Usage: node update-version.js <version>
 */
const fs = require("fs");
const path = require("path");

const version = process.argv[2];
if (!version) {
  console.error("Usage: node update-version.js <version>");
  process.exit(1);
}

const pkgPath = path.join(__dirname, "package.json");
const pkg = JSON.parse(fs.readFileSync(pkgPath, "utf-8"));

pkg.version = version;
for (const dep of Object.keys(pkg.optionalDependencies || {})) {
  pkg.optionalDependencies[dep] = version;
}

fs.writeFileSync(pkgPath, JSON.stringify(pkg, null, 2) + "\n");
console.log(`Updated npm/package.json to version ${version}`);
