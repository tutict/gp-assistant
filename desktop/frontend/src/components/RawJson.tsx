import { useMemo, useState } from "react";

const RAW_JSON_PREVIEW_LIMIT = 1800;
const RAW_JSON_DEV_LIMIT = 12000;
const viteEnv = (import.meta as ImportMeta & { env?: { DEV?: boolean; VITE_SHOW_RAW_JSON?: string } }).env;
const rawJsonEnabled = Boolean(viteEnv?.DEV) || viteEnv?.VITE_SHOW_RAW_JSON === "1";

interface RawJsonProps {
  result: unknown;
  className?: string;
  inline?: boolean;
  enabled?: boolean;
}

export function RawJson({ result, className = "raw-json", inline = false, enabled }: RawJsonProps) {
  const visible = enabled ?? rawJsonEnabled;
  const [expanded, setExpanded] = useState(inline && visible);
  const json = useMemo(() => {
    if (!visible && !expanded) return "";
    return stringifyJson(result);
  }, [expanded, result, visible]);
  const limit = visible ? RAW_JSON_DEV_LIMIT : RAW_JSON_PREVIEW_LIMIT;
  const visibleJson = json.length > limit && !expanded ? `${json.slice(0, limit)}\n...` : json;
  if (!visible) return null;

  if (inline) {
    return <pre className={className}>{visibleJson || "Release 已延迟渲染原始调试数据。"}</pre>;
  }

  return (
    <details
      className={className}
      onToggle={(event) => setExpanded((event.currentTarget as HTMLDetailsElement).open)}
    >
      <summary>原始 JSON</summary>
      <pre>{visibleJson}</pre>
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
