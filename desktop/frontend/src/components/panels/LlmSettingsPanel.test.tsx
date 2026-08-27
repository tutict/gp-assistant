import { useState } from "react";
import { act, create, type ReactTestRenderer } from "react-test-renderer";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { LlmConnectionTestResult, LlmSettings } from "../../types";

const { postJsonMock } = vi.hoisted(() => ({ postJsonMock: vi.fn() }));

vi.mock("../../lib/tauri", () => ({
  postJson: postJsonMock,
}));

import { LlmSettingsPanel } from "./LlmSettingsPanel";

const initialSettings: LlmSettings = {
  active_provider_id: "gateway",
  providers: [{
    id: "gateway",
    name: "公司网关",
    provider: "custom",
    base_url: "https://gateway.example/v1",
    model: "model-a",
    api_key: "sk-secret",
    api_format: "openai_chat",
    endpoint_mode: "base_url",
    timeout: 30,
  }],
};

function Harness({ initial = initialSettings }: { initial?: LlmSettings }) {
  const [settings, setSettings] = useState<LlmSettings | null>(initial);
  return <LlmSettingsPanel settings={settings} onChange={setSettings} />;
}

async function renderOpenPanel(initial = initialSettings): Promise<ReactTestRenderer> {
  let renderer!: ReactTestRenderer;
  await act(async () => {
    renderer = create(<Harness initial={initial} />);
  });
  const toggle = renderer.root.find((node) => node.type === "button" && node.props.className === "llm-settings-toggle");
  await act(async () => {
    toggle.props.onClick();
  });
  return renderer;
}

describe("LlmSettingsPanel provider configuration", () => {
  beforeEach(() => {
    vi.stubGlobal("window", {
      setTimeout: vi.fn(),
      requestAnimationFrame: (callback: () => void) => callback(),
    });
    vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  });

  afterEach(() => {
    postJsonMock.mockReset();
    vi.unstubAllGlobals();
  });

  it("supports manual models, full URLs, protocol selection, and secret visibility", async () => {
    const renderer = await renderOpenPanel();
    const modelInput = renderer.root.find((node) => node.props.id === "llmModel");
    expect(modelInput.props.type).toBe("text");
    await act(async () => {
      modelInput.props.onChange({ target: { value: "model-manual" } });
    });
    expect(renderer.root.find((node) => node.props.id === "llmModel").props.value).toBe("model-manual");

    const fullUrl = renderer.root.find((node) =>
      node.type === "button" && node.children.join("") === "完整 URL",
    );
    await act(async () => {
      fullUrl.props.onClick();
    });
    expect(fullUrl.props["aria-pressed"]).toBe(true);

    const protocol = renderer.root.find((node) => node.props.id === "llmApiFormat");
    await act(async () => {
      protocol.props.onChange({ target: { value: "openai_responses" } });
    });
    expect(renderer.root.find((node) => node.props.id === "llmApiFormat").props.value).toBe("openai_responses");

    const secret = renderer.root.find((node) => node.props.id === "llmApiKey");
    expect(secret.props.type).toBe("password");
    const reveal = renderer.root.find((node) => node.props["aria-label"] === "显示 API 密钥");
    await act(async () => {
      reveal.props.onClick();
    });
    expect(renderer.root.find((node) => node.props.id === "llmApiKey").props.type).toBe("text");

    const save = renderer.root.find((node) => node.type === "button" && node.children.join("") === "保存");
    await act(async () => {
      save.props.onClick();
    });
    expect(renderer.root.findAll((node) => node.props.className === "llm-settings-body llm-switcher")).toHaveLength(0);
  });

  it("tests the configured inference protocol instead of only fetching models", async () => {
    postJsonMock.mockResolvedValue({
      ok: true,
      endpoint: "https://gateway.example/v1/chat/completions",
      elapsed_ms: 42,
      api_format: "openai_chat",
    });
    const renderer = await renderOpenPanel();
    const testButton = renderer.root.find((node) =>
      node.type === "button" && node.children.some((child) => child === "测试连接"),
    );
    await act(async () => {
      await testButton.props.onClick();
    });
    expect(postJsonMock).toHaveBeenCalledWith(
      "/api/llm/test",
      expect.objectContaining({
        base_url: "https://gateway.example/v1",
        model: "model-a",
        api_format: "openai_chat",
        endpoint_mode: "base_url",
      }),
      { timeoutMs: 35000 },
    );
    expect(renderer.root.find((node) =>
      typeof node.props.className === "string" && node.props.className.includes("llm-connection-state"),
    ).children.join("")).toContain("42 ms");
  });

  it("keeps model discovery available for Anthropic-compatible relay endpoints", async () => {
    const renderer = await renderOpenPanel({
      active_provider_id: "deepseek-anthropic",
      providers: [{
        id: "deepseek-anthropic",
        name: "DeepSeek Anthropic",
        provider: "anthropic-compatible",
        base_url: "https://api.deepseek.com/anthropic",
        model: "deepseek-v4-flash",
        api_key: "sk-secret",
        api_format: "anthropic_messages",
        endpoint_mode: "base_url",
      }],
    });
    const refresh = renderer.root.find((node) => node.props["aria-label"] === "拉取供应商模型列表");
    expect(refresh.props.disabled).toBeFalsy();
    const hint = renderer.root.find((node) =>
      typeof node.props.className === "string" && node.props.className.includes("llm-model-hint"),
    );
    expect(hint.children.join("")).toContain("可直接输入模型 ID");
  });

  it("ignores a stale connection result after the provider configuration changes", async () => {
    let resolveTest!: (value: LlmConnectionTestResult) => void;
    postJsonMock.mockImplementation(() => new Promise((resolve) => {
      resolveTest = resolve;
    }));
    const renderer = await renderOpenPanel();
    const testButton = renderer.root.find((node) =>
      node.type === "button" && node.children.some((child) => child === "测试连接"),
    );
    await act(async () => {
      testButton.props.onClick();
      await Promise.resolve();
    });
    const modelInput = renderer.root.find((node) => node.props.id === "llmModel");
    await act(async () => {
      modelInput.props.onChange({ target: { value: "model-b" } });
    });
    await act(async () => {
      resolveTest({
        ok: true,
        endpoint: "https://gateway.example/v1/chat/completions",
        elapsed_ms: 42,
        api_format: "openai_chat",
      });
      await Promise.resolve();
    });
    expect(renderer.root.findAll((node) =>
      typeof node.props.className === "string" && node.props.className.includes("llm-connection-state"),
    )).toHaveLength(0);
  });
});
