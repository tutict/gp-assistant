export type NetworkProxyMode = "direct" | "system" | "manual";

export interface NetworkProxySettings {
  mode: NetworkProxyMode;
  url: string;
}

export interface NetworkProxyStorage {
  getItem(key: string): string | null;
  setItem(key: string, value: string): void;
  removeItem(key: string): void;
}

export const NETWORK_PROXY_MODE_KEY = "stock-optimizer-network-proxy-mode";
export const NETWORK_PROXY_URL_KEY = "stock-optimizer-network-proxy-url";
export const DEFAULT_NETWORK_PROXY_SETTINGS: NetworkProxySettings = {
  mode: "system",
  url: "",
};

export function readNetworkProxySettings(storage: NetworkProxyStorage): NetworkProxySettings {
  const rawMode = storage.getItem(NETWORK_PROXY_MODE_KEY);
  const mode: NetworkProxyMode = rawMode === "manual"
    ? "manual"
    : rawMode === "direct" || rawMode === "none"
      ? "direct"
      : "system";
  return {
    mode,
    url: storage.getItem(NETWORK_PROXY_URL_KEY) || "",
  };
}

export function validateManualProxyUrl(value: string): string | null {
  const candidate = value.trim();
  if (!candidate) return "请输入代理地址，例如 http://127.0.0.1:7890。";
  let parsed: URL;
  try {
    parsed = new URL(candidate);
  } catch {
    return "代理地址格式无效，请输入完整的 http:// 或 https:// 地址。";
  }
  if (parsed.protocol !== "http:" && parsed.protocol !== "https:") {
    return "代理地址仅支持 HTTP 或 HTTPS。";
  }
  if (!parsed.hostname) return "代理地址必须包含主机名或 IP 地址。";
  if (parsed.username || parsed.password) {
    return "代理地址不能包含账号或密码；请使用不带认证的代理地址。";
  }
  if (parsed.pathname !== "/" || parsed.search || parsed.hash) {
    return "代理地址只填写协议、主机和端口，不要附加路径或参数。";
  }
  return null;
}

export function saveNetworkProxySettings(
  storage: NetworkProxyStorage,
  settings: NetworkProxySettings,
): NetworkProxySettings {
  const url = settings.mode === "manual" ? settings.url.trim() : "";
  if (settings.mode === "manual") {
    const error = validateManualProxyUrl(url);
    if (error) throw new Error(error);
  }
  if (!["direct", "system", "manual"].includes(settings.mode)) {
    throw new Error("请选择有效的代理模式。");
  }
  const previousMode = storage.getItem(NETWORK_PROXY_MODE_KEY);
  const previousUrl = storage.getItem(NETWORK_PROXY_URL_KEY);
  try {
    storage.setItem(NETWORK_PROXY_MODE_KEY, settings.mode);
    storage.setItem(NETWORK_PROXY_URL_KEY, url);
  } catch {
    try {
      if (previousMode === null) storage.removeItem(NETWORK_PROXY_MODE_KEY);
      else storage.setItem(NETWORK_PROXY_MODE_KEY, previousMode);
      if (previousUrl === null) storage.removeItem(NETWORK_PROXY_URL_KEY);
      else storage.setItem(NETWORK_PROXY_URL_KEY, previousUrl);
    } catch {
      // Keep the user-facing error generic and do not expose storage internals.
    }
    throw new Error("代理设置未能保存，请重试；当前已保存设置继续生效。");
  }
  return { mode: settings.mode, url };
}

export function networkProxyPayload(settings: NetworkProxySettings): Record<string, string> {
  return {
    network_proxy_mode: settings.mode,
    network_proxy_url: settings.url,
  };
}
