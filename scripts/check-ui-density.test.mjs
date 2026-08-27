import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

const scriptPath = fileURLToPath(new URL("./check-ui-density.mjs", import.meta.url));
const shellPath = fileURLToPath(new URL("../desktop/frontend/src/styles/shell.css", import.meta.url));

function runGuard(shellSource) {
  const directory = mkdtempSync(join(tmpdir(), "gp-ui-density-"));
  const fixturePath = join(directory, "shell.css");
  writeFileSync(fixturePath, shellSource, "utf8");
  try {
    execFileSync(process.execPath, [scriptPath, "--shell", fixturePath], {
      encoding: "utf8",
      stdio: "pipe",
    });
    return null;
  } catch (error) {
    return error;
  } finally {
    rmSync(directory, { recursive: true, force: true });
  }
}

test("rejects desktop layout declarations placed under the wrong selectors", () => {
  const original = readFileSync(shellPath, "utf8");
  const misplaced = original
    .replace(/\.workbench\s*\{\s*padding-inline:\s*max\(22px, calc\(\(100vw - 1560px\) \/ 2\)\);/, ".wide-workbench-misplaced { padding-inline: max(22px, calc((100vw - 1560px) / 2));")
    .replace(/\.panel-controls\b/g, ".form-controls-misplaced")
    .replace(/\.backtest-controls\b/g, ".backtest-controls-misplaced")
    .replace(/\.notes p\b/g, ".notes-misplaced p")
    .replace(/\.evidence-list p\b/g, ".evidence-list-misplaced p")
    .replace(/\.agent-final-reply\b/g, ".agent-final-reply-misplaced");
  const result = runGuard(misplaced);
  assert.ok(result, "the density guard must reject declarations in unrelated selectors");
  const stderr = String(result.stderr ?? result.stdout ?? result.message ?? "");
  assert.match(stderr, /shell\.css:\d+ \.workbench must declare wide-screen centering/);
  assert.match(stderr, /shell\.css:\d+ \.panel-controls must declare max-width: 1200px/);
  assert.match(stderr, /shell\.css:\d+ \.notes p must declare max-width: 76ch/);
});

test("rejects desktop constraints that only exist in a mobile media block", () => {
  const original = readFileSync(shellPath, "utf8");
  const controlsRule = /\.panel-controls,\s*\.backtest-controls\s*\{[\s\S]*?\n\}/;
  const proseRule = /\.notes p,\s*\.evidence-list p,\s*\.agent-final-reply\s*\{[\s\S]*?\n\}/;
  const misplaced = original
    .replace(controlsRule, (rule) => `@media (max-width: 768px) {\n${rule}\n}`)
    .replace(proseRule, (rule) => `@media (max-width: 768px) {\n${rule}\n}`);
  const result = runGuard(misplaced);
  assert.ok(result, "the density guard must reject mobile-only desktop constraints");
  const stderr = String(result.stderr ?? result.stdout ?? result.message ?? "");
  assert.match(stderr, /\.panel-controls must declare max-width: 1200px/);
  assert.match(stderr, /\.notes p must declare max-width: 76ch/);
});
