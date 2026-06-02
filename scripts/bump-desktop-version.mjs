#!/usr/bin/env node
import { readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const version = process.argv[2];

if (!version || !/^\d+\.\d+\.\d+$/.test(version)) {
  console.error("Usage: scripts/bump-desktop-version.mjs <x.y.z>");
  process.exit(2);
}

const root = dirname(dirname(fileURLToPath(import.meta.url)));

function filePath(relativePath) {
  return join(root, relativePath);
}

function writeJson(relativePath, update) {
  const absolutePath = filePath(relativePath);
  const data = JSON.parse(readFileSync(absolutePath, "utf8"));
  update(data);
  writeFileSync(absolutePath, `${JSON.stringify(data, null, 2)}\n`);
}

function replaceOrFail(relativePath, pattern, replacement, description) {
  const absolutePath = filePath(relativePath);
  const before = readFileSync(absolutePath, "utf8");
  if (!pattern.test(before)) {
    console.error(`Could not update ${description} in ${relativePath}`);
    process.exit(1);
  }
  const after = before.replace(pattern, replacement);
  writeFileSync(absolutePath, after);
}

writeJson("apps/desktop/package.json", (data) => {
  data.version = version;
});

writeJson("apps/desktop/package-lock.json", (data) => {
  data.version = version;
  if (data.packages?.[""]) {
    data.packages[""].version = version;
  }
});

replaceOrFail(
  "apps/desktop/src-tauri/tauri.conf.json",
  /("version": ")[^"]+(")/,
  `$1${version}$2`,
  "tauri.conf.json version",
);

replaceOrFail(
  "apps/desktop/src-tauri/Cargo.toml",
  /(\[package\][\s\S]*?^name = "daytrail-desktop"[\s\S]*?^version = ")[^"]+(")/m,
  `$1${version}$2`,
  "Cargo.toml package version",
);

replaceOrFail(
  "apps/desktop/src-tauri/Cargo.lock",
  /(\[\[package\]\]\nname = "daytrail-desktop"\nversion = ")[^"]+(")/,
  `$1${version}$2`,
  "Cargo.lock package version",
);

console.log(`Bumped desktop version metadata to ${version}`);
