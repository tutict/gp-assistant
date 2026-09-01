import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

const scriptPath = fileURLToPath(new URL("./check-theme-parity.mjs", import.meta.url));

function runGuard(source) {
  const directory = mkdtempSync(join(tmpdir(), "gp-theme-parity-"));
  const fixturePath = join(directory, "tokens.css");
  writeFileSync(fixturePath, source, "utf8");
  try {
    const stdout = execFileSync(process.execPath, [scriptPath, "--tokens", fixturePath], {
      encoding: "utf8",
      stdio: "pipe",
    });
    return { status: 0, stdout, stderr: "" };
  } catch (error) {
    return {
      status: error.status,
      stdout: String(error.stdout ?? ""),
      stderr: String(error.stderr ?? error.message ?? ""),
    };
  } finally {
    rmSync(directory, { recursive: true, force: true });
  }
}

test("rejects light themes that omit direct and aliased color tokens", () => {
  const result = runGuard(`
    :root {
      --text: #f4f7fa;
      --muted: rgba(255, 255, 255, 0.5);
      --danger: var(--text);
      --touch-height: var(--touch-comfort);
      --touch-comfort: 44px;
      --agent-send-start: #ef4444;
      --chart-up: var(--danger);
    }
    [data-theme="light"] { --text: #18212a; }
  `);

  assert.equal(result.status, 1);
  assert.match(result.stderr, /--muted/);
  assert.match(result.stderr, /--danger/);
  assert.doesNotMatch(result.stderr, /--touch-height/);
  assert.doesNotMatch(result.stderr, /--agent-send-start/);
  assert.match(result.stderr, /--chart-up/);
});

test("accepts complete light color mappings and color-mix values", () => {
  const result = runGuard(`
    :root {
      --text: #f4f7fa;
      --muted: rgba(255, 255, 255, 0.5);
      --danger: var(--text);
      --wash: color-mix(in srgb, var(--text) 10%, transparent);
      --space-1: 4px;
    }
    [data-theme="light"] {
      --text: #18212a;
      --muted: rgba(21, 31, 42, 0.5);
      --danger: var(--text);
      --wash: color-mix(in srgb, var(--text) 8%, transparent);
    }
  `);

  assert.equal(result.status, 0, result.stderr);
  assert.match(result.stdout, /4 color tokens/);
});
