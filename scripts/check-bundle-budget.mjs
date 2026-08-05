#!/usr/bin/env node

import { readFileSync, statSync } from "node:fs";
import { gzipSync } from "node:zlib";
import { fileURLToPath } from "node:url";

const outputDirectory = fileURLToPath(
  new URL("../desktop/mobile-dist/", import.meta.url),
);
const manifestPath = `${outputDirectory}.vite/manifest.json`;
const warningBudget = 180 * 1024;
const failureBudget = 220 * 1024;
const manifest = JSON.parse(readFileSync(manifestPath, "utf8"));
const entries = Object.values(manifest).filter((chunk) => chunk.isEntry);

if (entries.length === 0) {
  console.error("Bundle budget failed: Vite manifest contains no entry chunk");
  process.exit(1);
}

let failed = false;
for (const entry of entries) {
  const assetPath = `${outputDirectory}${entry.file}`;
  const source = readFileSync(assetPath);
  const gzipBytes = gzipSync(source).byteLength;
  const rawBytes = statSync(assetPath).size;
  const summary = `${entry.file}: ${(rawBytes / 1024).toFixed(1)}KB raw, `
    + `${(gzipBytes / 1024).toFixed(1)}KB gzip`;
  if (gzipBytes > failureBudget) {
    console.error(`Bundle budget failed: ${summary} exceeds 220KB`);
    failed = true;
  } else if (gzipBytes > warningBudget) {
    console.warn(`Bundle budget warning: ${summary} exceeds 180KB`);
  } else {
    console.log(`Bundle budget passed: ${summary}`);
  }
}

if (failed) process.exit(1);
