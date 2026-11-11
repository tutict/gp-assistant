import { useState } from "react";
import { getJson } from "../../lib/tauri";
import {
  readNetworkProxySettings,
  saveNetworkProxySettings,
  validateManualProxyUrl,
  type NetworkProxyMode,
  type NetworkProxySettings,
} from "../../lib/networkProxy";

interface HkNetworkProbeSource {
  source_id: string;
  ok: boolean;
  error?: string;
  elapsed_ms?: number;
}

interface HkNetworkProbeResult {
  market: "hk";
  code: string;
  proxy_mode: string;
  proxy_configured: boolean;
  sources: HkNetworkProbeSource[];
}

const SOURCE_LABELS: Record<string, string> = {
  tencent: "腾讯行情",
  tencent_history: "腾讯日 K",
  sina_hk_financial: "新浪财务",
  eastmoney_hk: "东方财富财务",
};

function defaultProxySettings(): NetworkProxySettings {
  if (typeof localStorage === "undefined") return { mode: "system", url: "" };
  return readNetworkProxySettings(localStorage);
}

export function NetworkProxyPanel() {
  const [settings, setSettings] = useState<NetworkProxySettings>(defaultProxySettings);
  const [savedSettings, setSavedSettings] = useState<NetworkProxySettings>(defaultProxySettings);
  const [probeCode, setProbeCode] = useState("00700.HK");
  const [probe, setProbe] = useState<HkNetworkProbeResult | null>(null);
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState("");
  const [fieldError, setFieldError] = useState("");
  const dirty = settings.mode !== savedSettings.mode || settings.url !== savedSettings.url;

  function updateMode(mode: NetworkProxyMode) {
    setSettings((current) => ({ ...current, mode }));
    setFieldError("");
    setMessage("");
  }

  function save() {
    if (settings.mode === "manual") {
      const error = validateManualProxyUrl(settings.url);
      if (error) {
        setFieldError(error);
        setMessage("");
        return;
      }
    }
    try {
      const saved = saveNetworkProxySettings(localStorage, settings);
      setSettings(saved);
      setSavedSettings(saved);
      setProbe(null);
      setFieldError("");
      setMessage("代理设置已保存；后续港股刷新会使用此网络配置。");
    } catch (error) {
      setMessage(error instanceof Error ? error.message : "代理设置未能保存，请重试。");
    }
  }

  async function testConnection() {
    if (dirty) {
      setMessage("请先保存代理设置，再测试数据源连接。");
      return;
    }
    const normalizedCode = probeCode.trim().toUpperCase();
    const match = normalizedCode.match(/^(\d{1,5})(?:\.HK)?$/);
    if (!match) {
      setMessage("请输入一至五位港股代码，例如 00700.HK。");
      return;
    }
    const code = `${match[1].padStart(5, "0")}.HK`;
    setBusy(true);
    setProbe(null);
    setMessage("");
    try {
      const result = await getJson<HkNetworkProbeResult>(
        `/api/hk/network-probe?code=${encodeURIComponent(code)}`,
      );
      setProbe(result);
      const working = result.sources.filter((source) => source.ok).length;
      setMessage(`测试完成：${working}/${result.sources.length} 个数据源可访问。`);
    } catch (error) {
      setMessage(error instanceof Error ? error.message : "代理连接测试失败，请检查地址和本机代理是否运行。");
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="settings-item-copy network-proxy-panel" aria-busy={busy}>
      <p>为港股行情、日线、财务和智能选股补数配置本机网络。模型连接仍使用各自的连接设置。</p>
      <label className="network-proxy-field">
        <span>代理模式</span>
        <select
          aria-label="代理模式"
          value={settings.mode}
          disabled={busy}
          onChange={(event) => updateMode(event.currentTarget.value as NetworkProxyMode)}
        >
          <option value="direct">直接连接</option>
          <option value="system">读取环境代理</option>
          <option value="manual">手动 HTTP(S) 代理</option>
        </select>
      </label>
      {settings.mode === "manual" ? (
        <label className="network-proxy-field">
          <span>代理地址</span>
          <input
            aria-label="手动代理地址"
            type="url"
            autoComplete="url"
            placeholder="http://127.0.0.1:7890"
            value={settings.url}
            disabled={busy}
            aria-invalid={Boolean(fieldError)}
            aria-describedby={fieldError ? "network-proxy-error" : "network-proxy-help"}
            onChange={(event) => {
              setSettings((current) => ({ ...current, url: event.currentTarget.value }));
              setFieldError("");
              setMessage("");
            }}
          />
          {fieldError ? <small id="network-proxy-error" role="alert">{fieldError}</small> : null}
          {!fieldError ? <small id="network-proxy-help">仅支持不带账号密码的 HTTP(S) 代理地址。</small> : null}
        </label>
      ) : null}
      <div className="network-proxy-actions">
        <button type="button" className="save-btn" disabled={busy || !dirty} onClick={save}>保存代理设置</button>
        <button type="button" className="clear-btn" disabled={busy || dirty} onClick={() => void testConnection()}>
          {busy ? "测试中…" : "测试港股数据源"}
        </button>
      </div>
      <label className="network-proxy-field network-proxy-code-field">
        <span>测试股票</span>
        <input
          aria-label="代理测试港股代码"
          value={probeCode}
          disabled={busy}
          onChange={(event) => setProbeCode(event.currentTarget.value)}
        />
      </label>
      {message ? <p className="network-proxy-message" role="status" aria-live="polite">{message}</p> : null}
      {probe ? (
        <ul className="network-proxy-results" aria-label="港股数据源连接测试结果">
          {probe.sources.map((source) => (
            <li key={source.source_id} data-status={source.ok ? "available" : "unavailable"}>
              <strong>{SOURCE_LABELS[source.source_id] || source.source_id}</strong>
              <span>{source.ok ? "可访问" : source.error || "暂不可用"}</span>
              {typeof source.elapsed_ms === "number" ? <small>{source.elapsed_ms} ms</small> : null}
            </li>
          ))}
        </ul>
      ) : null}
    </div>
  );
}
