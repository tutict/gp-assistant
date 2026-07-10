import { useMemo, useState } from "react";

const RAW_JSON_PREVIEW_LIMIT = 1800;
const RAW_JSON_DEV_LIMIT = 12000;
const viteEnv = (import.meta as ImportMeta & { env?: { DEV?: boolean; VITE_SHOW_RAW_JSON?: string } }).env;
const rawJsonEnabled = Boolean(viteEnv?.DEV) || viteEnv?.VITE_SHOW_RAW_JSON === "1";

interface RawJsonProps {
  result: unknown;
  className?: string;
  inline?: boolean;
}

export function RawJson({ result, className = "raw-json", inline = false }: RawJsonProps) {
  const [expanded, setExpanded] = useState(inline && rawJsonEnabled);
  const json = useMemo(() => {
    if (!expanded && !rawJsonEnabled) return "";
    return stringifyJson(result);
  }, [expanded, result]);
  const limit = rawJsonEnabled ? RAW_JSON_DEV_LIMIT : RAW_JSON_PREVIEW_LIMIT;
  const visibleJson = json.length > limit && !expanded ? `${json.slice(0, limit)}\n...` : json;

  if (inline) {
    return <pre className={className}>{visibleJson || "Release 已延迟渲染原始调试数据。"}</pre>;
  }

  return (
    <details
      className={className}
      onToggle={(event) => setExpanded((event.currentTarget as HTMLDetailsElement).open)}
    >
      <summary>原始 JSON</summary>
      {expanded || rawJsonEnabled ? (
        <pre>{visibleJson}</pre>
      ) : (
        <p className="raw-json-hint">Release 默认延迟渲染完整调试数据，展开后再生成。</p>
      )}
    </details>
  );
}

function stringifyJson(value: unknown): string {
  try {
    return JSON.stringify(value, null, 2);
  } catch {
    return String(value ?? "");
  }
}
