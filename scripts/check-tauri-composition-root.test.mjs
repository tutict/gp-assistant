import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { readFileSync, readdirSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { join, relative } from "node:path";
import test from "node:test";

const root = fileURLToPath(new URL("../", import.meta.url));
const src = join(root, "desktop/src-tauri/src");
const read = (path) => readFileSync(join(root, path), "utf8");
const lib = read("desktop/src-tauri/src/lib.rs");
const registry = read("desktop/src-tauri/src/app/command_registry.rs");
const workflow = read(".github/workflows/rag-evaluation.yml");
// Reviewed pre-integration main (e1bf42a2); new owners do not inherit these ceilings.
const legacyCeilings = {
  "agent_harness.rs": 2727,
  "hk_market.rs": 8401,
  "news_rag.rs": 4835,
  "observe.rs": 3525,
  "research_tests.rs": 2951,
  "rig_runtime.rs": 4864,
  "screening.rs": 5110,
  "tests.rs": 5574
};
const appOwners = [
  "app/mod.rs", "app/bootstrap.rs", "app/command_registry.rs", "app/commands.rs",
  "app/desktop_window.rs", "app/lifecycle.rs", "app/state.rs",
];
// The reviewed seven-owner fingerprint expansion adds exactly 22 metadata lines.
const metadataGrowth = { "screening.rs": 22 };

function rustFiles(directory) {
  return readdirSync(directory, { withFileTypes: true }).flatMap((entry) => {
    const path = join(directory, entry.name);
    return entry.isDirectory() ? rustFiles(path) : entry.isFile() && path.endsWith(".rs") ? [path] : [];
  });
}

test("crate root declares modules and delegates startup", () => {
  assert.ok(lib.trimEnd().split(/\r?\n/).length <= 80);
  assert.doesNotMatch(lib, /generate_handler!|invoke_handler\s*\(|fn setup_desktop|fn setup_reliability/);
  assert.doesNotMatch(lib, /static\s+(?:CLOSE_HANDLER_READY|CLOSE_SAVED|STARTUP_WARNINGS)/);
  assert.match(lib, /pub fn run\(\)\s*\{\s*app::run\(\);/);
  assert.match(lib, /cfg_attr\(mobile, tauri::mobile_entry_point\)/);
});

test("IPC registration keeps all 153 frozen commands", () => {
  const handler = registry.match(/generate_handler!\s*\[([\s\S]*?)\]\s*\)/)?.[1];
  assert.ok(handler);
  const commands = [...handler.matchAll(/^\s*([\w:]+)\s*,?\s*$/gm)]
    .map((match) => match[1].split("::").at(-1)).sort();
  assert.equal(commands.length, 153);
  assert.equal(createHash("sha256").update(commands.join("\n")).digest("hex"),
    "23d97d0efdadf12ea42c1797522acbb28cab1ed586d4173c3a31660000fb9e52");
  assert.equal([...registry.matchAll(/generate_handler!/g)].length, 1);
  assert.deepEqual([...registry.matchAll(/^\s*(?:pub(?:\([^)]*\))?\s+)?fn\s+(\w+)/gm)]
    .map((match) => match[1]), ["register"]);
});

test("app owners participate in the existing desktop fingerprint", () => {
  const screening = read("desktop/src-tauri/src/screening.rs");
  const descriptor = JSON.parse(read("scripts/rust-architecture/governance-policy.json"))
    .fingerprints.find((spec) => spec.entries.some((entry) => entry.source === "desktop/src-tauri/src/lib.rs"));
  const labels = descriptor.entries.map((entry) => entry.label);
  const start = labels.indexOf("lib.rs") + 1;
  assert.deepEqual(labels.slice(start, start + appOwners.length), appOwners);
  for (const owner of appOwners) {
    assert.ok(screening.includes('include_bytes!("' + owner + '")'), "missing fingerprint owner: " + owner);
  }
});

test("HK commands move behind the API seam with original root reexports", () => {
  const hk = read("desktop/src-tauri/src/hk_market.rs");
  const api = read("desktop/src-tauri/src/hk_market/api.rs");
  assert.match(hk, /pub\(crate\) mod api;/);
  assert.doesNotMatch(hk, /pub\(crate\) (?:async )?fn api_hk_market_status/);
  assert.match(api, /pub\(crate\) fn api_hk_market_status/);
  assert.match(api, /pub\(crate\) async fn api_hk_market_refresh/);
  assert.match(registry, /crate::hk_market::api::api_hk_market_status/);
  assert.match(read("desktop/src-tauri/src/rig_runtime.rs"), /crate::hk_market::api::api_hk_observe/);
});

test("CI retains the original exact retrieval gate and adds the composition guard", () => {
  assert.match(workflow, /node scripts\/check-tauri-composition-root\.test\.mjs/);
  assert.match(workflow, /--expected research::tests::fixed_retrieval_eval_suite_enforces_governance_invariants/);
  assert.doesNotMatch(workflow, /research::tests::retrieval::fixed_retrieval/);
});

test("new owners stay bounded and existing large files do not grow", () => {
  for (const path of rustFiles(src)) {
    const name = relative(src, path).replaceAll("\\", "/");
    const lines = readFileSync(path, "utf8").trimEnd().split(/\r?\n/).length;
    const ceiling = legacyCeilings[name] === undefined
      ? 2500 : legacyCeilings[name] + (metadataGrowth[name] ?? 0);
    assert.ok(lines <= ceiling, name + " exceeded its source-size ceiling");
  }
});
