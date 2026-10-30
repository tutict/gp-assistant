#!/usr/bin/env node
import assert from "node:assert/strict";
import { mkdirSync } from "node:fs";
import { createRequire } from "node:module";
import { resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { installHarnessState, startBuiltAppServer } from "./ui-screenshot.mjs";

// Browser plugin is unavailable in this workspace; use the project's installed Playwright.
const requireFrontend = createRequire(new URL("../desktop/frontend/package.json", import.meta.url));
const { chromium } = requireFrontend("playwright");
const output = resolve(fileURLToPath(new URL("../logs/dev/api-config-checks/", import.meta.url)));
mkdirSync(output, { recursive: true });
const built = await startBuiltAppServer();
let browser;
try {
  browser = await chromium.launch();
  for (const width of [390, 430, 1440]) for (const theme of ["dark", "light"]) {
    const label = width + "-" + theme;
    const context = await browser.newContext({ viewport: { width, height: width < 768 ? 844 : 900 }, colorScheme: theme });
    const page = await context.newPage();
    page.setDefaultTimeout(12000);
    const errors = [];
    page.on("pageerror", error => errors.push(error.message));
    await installHarnessState(page);
    await page.addInitScript(() => {
      localStorage.setItem("stock-optimizer-workspace-v1", JSON.stringify({ schemaVersion: 1, revision: 0, values: { "app.view": "screen" } }));
      localStorage.setItem("stock-optimizer-llm-settings", JSON.stringify({
        active_provider_id: "a", providers: [
          { id: "a", name: "当前网关", provider: "custom", base_url: "https://gateway.example/v1", model: "model-a", remember_key: false },
          { id: "b", name: "备用网关", provider: "custom", base_url: "https://backup.example/v1", model: "model-b", remember_key: false },
        ],
      }));
    });
    const requests = [];
    await page.route("**/api/llm/models", route => {
      requests.push({ kind: "models", payload: route.request().postDataJSON() });
      return route.fulfill({ json: { count: 3, models: [{ id: "alpha" }, { id: "beta-one", name: "Beta" }, { id: "beta-two", owned_by: "Beta" }] } });
    });
    await page.route("**/api/llm/test", route => {
      const payload = route.request().postDataJSON();
      requests.push({ kind: "test", payload });
      if (payload.api_key === "fixture-bad-key") return route.fulfill({ status: 401,
        json: { code: "auth", message: "密钥未通过，请检查后重试。", elapsed_ms: 8 } });
      return route.fulfill({ json: { ok: true, status: 200, elapsed_ms: 42,
        api_format: "openai_chat", endpoint: "https://backup.example/v1/chat/completions?api_key=fixture-query-secret" } });
    });
    try {
      await page.goto(built.url + "/?theme=" + theme + "#sectionScreen", { waitUntil: "networkidle" });
      await page.getByRole("button", { name: "设置", exact: true }).click();
      await page.locator(".settings-sheet .llm-settings-toggle").click();
      const dialog = page.getByRole("dialog", { name: "模型连接配置", exact: true });
      await dialog.waitFor();
      await dialog.getByRole("button", { name: "编辑 备用网关", exact: true }).click();
      await page.evaluate(() => new Promise(resolveFrame => requestAnimationFrame(() => requestAnimationFrame(resolveFrame))));
      await page.evaluate(() => document.activeElement?.blur());
      await page.keyboard.press("Tab");
      assert.equal(await dialog.evaluate(element => element.contains(document.activeElement)), true, "Tab focus escaped the API dialog");
      await page.keyboard.press("Shift+Tab");
      assert.equal(await dialog.getByRole("button", { name: "测试连接", exact: true }).evaluate(element => element === document.activeElement), true);
      await page.keyboard.press("Tab");
      assert.equal(await dialog.getByRole("button", { name: "关闭模型配置", exact: true }).evaluate(element => element === document.activeElement), true);
      const persisted = () => page.evaluate(() => JSON.parse(localStorage.getItem("stock-optimizer-llm-settings")));
      assert.equal((await persisted()).active_provider_id, "a");
      const remember = dialog.locator(".llm-remember-key");
      const checkbox = remember.getByRole("checkbox");
      const glyph = await checkbox.boundingBox();
      assert.ok(glyph.width <= 20 && glyph.height <= 20, "Remember-key checkbox was enlarged by form styles");
      const rememberBox = await remember.boundingBox();
      assert.ok(Math.abs(glyph.y + glyph.height / 2 - rememberBox.y - rememberBox.height / 2) < 1, "Remember-key checkbox is misaligned with its label");
      await remember.click();
      assert.equal(await checkbox.isChecked(), true);
      assert.equal((await persisted()).providers.find(item => item.id === "b").remember_key, false);
      await remember.click();
      assert.equal(await checkbox.isChecked(), false);
      await dialog.locator("#llmModel").fill("manual-model");
      assert.equal((await persisted()).providers.find(item => item.id === "b").model, "model-b");
      assert.equal(await dialog.getByRole("button", { name: "保存并启用", exact: true }).isEnabled(), false);
      await dialog.getByRole("button", { name: "保存", exact: true }).click();
      await page.waitForFunction(() => JSON.parse(localStorage.getItem("stock-optimizer-llm-settings")).providers.find(item => item.id === "b").model === "manual-model");
      assert.equal((await persisted()).active_provider_id, "a");
      await dialog.locator("#llmApiKey").fill("fixture-replacement-key");
      await dialog.getByRole("button", { name: "拉取供应商模型列表", exact: true }).click();
      await dialog.getByRole("button", { name: "选择供应商模型", exact: true }).click();
      await dialog.locator("#llmModelSearch").press("Escape");
      assert.equal(await dialog.isVisible(), true);
      await dialog.getByRole("button", { name: "选择供应商模型", exact: true }).click();
      await dialog.getByRole("combobox", { name: "搜索模型", exact: true }).fill("beta");
      assert.equal(await dialog.getByRole("listbox", { name: "供应商模型" }).getByRole("option").count(), 2);
      await dialog.locator("#llmModelSearch").press("ArrowDown");
      await dialog.locator("#llmModelSearch").press("Enter");
      assert.equal(await dialog.locator("#llmModel").inputValue(), "beta-two");
      await dialog.getByRole("button", { name: "测试连接", exact: true }).click();
      await dialog.locator(".llm-connection-state.success").waitFor();
      assert.equal(requests.at(-1).payload.api_key, "fixture-replacement-key");
      assert.equal(requests.find(item => item.kind === "models").payload.api_key, "fixture-replacement-key");
      assert.equal((await dialog.innerText()).includes("fixture-query-secret"), false);
      await dialog.getByRole("button", { name: "保存并启用", exact: true }).click();
      await page.waitForFunction(() => JSON.parse(localStorage.getItem("stock-optimizer-llm-settings")).active_provider_id === "b");
      assert.equal(JSON.stringify(await persisted()).includes("fixture-replacement-key"), false);
      assert.equal(await dialog.locator("#llmApiKey").inputValue(), "");
      await dialog.locator(".llm-editor-container").evaluate(element => { element.scrollTop = 0; });
      const box = await dialog.boundingBox();
      const footer = await dialog.locator(".llm-config-footer").boundingBox();
      assert.ok(box.width <= Math.min(width, 960) + 1);
      assert.ok(footer.y + footer.height <= box.y + box.height + 1, "Footer escapes the dialog");
      assert.equal(await dialog.evaluate(element => element.scrollWidth > element.clientWidth + 1), false, "Dialog has horizontal overflow");
      if (width < 768) {
        const smallControls = await dialog.locator("button:visible").evaluateAll(elements => elements.map(element => ({
          label: element.getAttribute("aria-label") || element.innerText,
          className: element.className, width: element.getBoundingClientRect().width, height: element.getBoundingClientRect().height,
        })).filter(item => Math.min(item.width, item.height) < 43.5));
        assert.deepEqual(smallControls, [], "Mobile touch controls are too small: " + JSON.stringify(smallControls));
        assert.ok(Math.abs(box.y) < 1 && Math.abs(box.height - 844) < 1, "Mobile dialog must fill the viewport: " + JSON.stringify(box));
      }
      await dialog.screenshot({ path: resolve(output, label + "-editor.png") });
      if (width < 768) {
        await dialog.getByRole("button", { name: "连接列表", exact: true }).click();
        await dialog.screenshot({ path: resolve(output, label + "-connections.png") });
        await dialog.getByRole("button", { name: "编辑 备用网关", exact: true }).click();
      }
      await dialog.locator("#llmApiKey").fill("fixture-bad-key");
      await dialog.getByRole("button", { name: "测试连接", exact: true }).click();
      const failed = dialog.locator(".llm-connection-state.error");
      await failed.waitFor();
      assert.ok((await failed.innerText()).includes("鉴权未通过"));
      assert.ok((await failed.innerText()).includes("HTTP 401"));
      assert.equal((await failed.innerText()).includes('"code"'), false, "Diagnostic displays raw JSON");
      assert.equal(await dialog.getByRole("button", { name: "保存", exact: true }).isEnabled(), false);
      await dialog.getByRole("button", { name: "更多", exact: true }).click();
      await dialog.getByRole("button", { name: "放弃此连接修改", exact: true }).click();
      await dialog.getByRole("button", { name: "放弃并重新载入", exact: true }).click();
      await page.waitForFunction(() => document.activeElement === document.querySelector(".llm-sheet-back-btn"));
      await dialog.locator("#llmProviderName").fill("尚未保存的名称");
      await dialog.getByRole("button", { name: "关闭模型配置", exact: true }).click();
      await dialog.getByRole("alertdialog").waitFor();
      await dialog.getByRole("button", { name: "继续编辑", exact: true }).click();
      assert.equal(await dialog.locator("#llmProviderName").inputValue(), "尚未保存的名称");
      await dialog.locator("#llmProviderName").press("Escape");
      await dialog.getByRole("button", { name: "放弃修改并关闭", exact: true }).click();
      await dialog.waitFor({ state: "hidden" });
      assert.equal(await page.locator(".settings-sheet").isVisible(), true, "Closing model config also closed settings");
      await page.waitForFunction(() => document.activeElement === document.querySelector(".settings-sheet .llm-settings-toggle"));
      assert.equal(await page.locator(".settings-sheet .llm-settings-toggle").evaluate(element => document.activeElement === element), true);
      assert.equal(errors.length, 0, "Browser errors: " + errors.join("; "));
      console.log("[api-config] " + label + " passed");
    } catch (error) {
      await page.screenshot({ path: resolve(output, label + "-failure.png"), fullPage: true }).catch(() => {});
      throw error;
    } finally { await context.close(); }
  }
  console.log("[api-config] 6 responsive/theme scenarios passed; screenshots: " + output);
} finally {
  await browser?.close();
  await new Promise(resolveClose => built.server.close(resolveClose));
}
