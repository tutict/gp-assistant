import { useCallback, useState } from "react";
import type {
  LlmSettings,
  NewsEvidence,
  NewsImpactFinding,
  NewsRagResult,
  RagPackQueryResult,
  UpstreamRagBuildResult,
  UpstreamRagTransferResult,
} from "../../types";
import { getJson, isMobileTauriRuntime, postJson } from "../../lib/tauri";
import {
  buildLlmConfig,
  buildNewsRagRequest,
  buildRagPackBuildRequest,
  buildRagPackQueryRequest,
  buildUpstreamRagBuildRequest,
  fetchUpstreamImportPayload,
  normalizeNewsGroups,
  normalizeRagHit,
  parseUpstreamImportDescriptor,
} from "../../lib/contracts";
import { escapeHtml, formatBytes, formatDateTime, formatNumber, normalizeStockCode } from "../../lib/format";

type NewsTab = "newsRag" | "ragPackBuild" | "ragPackQuery" | "upstreamScan" | "upstreamImport";

const TABS: { key: NewsTab; label: string }[] = [
  { key: "newsRag", label: "News RAG" },
  { key: "ragPackBuild", label: "Build pack" },
  { key: "ragPackQuery", label: "Query pack" },
  { key: "upstreamScan", label: "Upstream sync" },
  { key: "upstreamImport", label: "Mobile import" },
];

interface NewsRagPanelProps {
  llmSettings?: LlmSettings | null;
}

export function NewsRagPanel({ llmSettings }: NewsRagPanelProps) {
  const [tab, setTab] = useState<NewsTab>("newsRag");
  const [loading, setLoading] = useState(false);
  const [result, setResult] = useState<unknown>(null);
  const [error, setError] = useState<string | null>(null);
  const [newsCode, setNewsCode] = useState("");
  const [newsDays, setNewsDays] = useState(30);
  const [seedCodes, setSeedCodes] = useState("");
  const [queryText, setQueryText] = useState("");
  const [manualUrls, setManualUrls] = useState("");
  const [importPayload, setImportPayload] = useState("");
  const [scanning, setScanning] = useState(false);

  const normalizedCode = normalizeStockCode(newsCode);

  const run = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      if (tab === "newsRag") {
        if (!normalizedCode) throw new Error("Please enter a valid stock code.");
        const data = await postJson<NewsRagResult>("/api/news-rag", buildNewsRagRequest(normalizedCode, newsDays, buildLlmConfig(llmSettings)));
        setResult(data);
      } else if (tab === "ragPackBuild") {
        const data = await postJson("/api/rag-pack/build-from-news-cache", buildRagPackBuildRequest(normalizedCode, newsDays, seedCodes));
        setResult(data);
      } else if (tab === "ragPackQuery") {
        const data = await postJson<RagPackQueryResult>("/api/rag-pack/query", buildRagPackQueryRequest(queryText, normalizedCode, seedCodes));
        setResult(data);
      } else if (tab === "upstreamScan") {
        if (isMobileTauriRuntime()) {
          await scanAndImport();
        } else {
          if (!normalizedCode) throw new Error("Desktop upstream build needs a target stock code.");
          const build = await postJson<UpstreamRagBuildResult>("/api/upstream-rag/build", buildUpstreamRagBuildRequest(normalizedCode, newsDays, manualUrls));
          const manifest = build.manifest || {};
          const transfer = manifest.valid
            ? await postJson<UpstreamRagTransferResult>("/api/upstream-rag/transfer/start", { ttl_minutes: 15 })
            : null;
          setResult({ build, transfer });
        }
      } else if (tab === "upstreamImport") {
        if (isMobileTauriRuntime()) {
          const descriptor = parseUpstreamImportDescriptor(importPayload);
          const payload = await fetchUpstreamImportPayload(descriptor);
          const data = await postJson("/api/upstream-rag/mobile/import", payload);
          setResult(data);
        } else {
          const data = await getJson("/api/upstream-rag/status");
          setResult(data);
        }
      }
    } catch (err) {
      setError((err as Error).message);
    } finally {
      setLoading(false);
    }
  }, [importPayload, llmSettings, manualUrls, newsDays, normalizedCode, queryText, seedCodes, tab]);

  const scanAndImport = useCallback(async () => {
    setScanning(true);
    try {
      const raw = await scanQrCode();
      setImportPayload(raw);
      const descriptor = parseUpstreamImportDescriptor(raw);
      const payload = await fetchUpstreamImportPayload(descriptor);
      const data = await postJson("/api/upstream-rag/mobile/import", payload);
      setResult(data);
    } finally {
      setScanning(false);
    }
  }, []);

  return (
    <div className="panel-container">
      <div className="panel-tabs">
        {TABS.map((t) => (
          <button key={t.key} type="button" className={`panel-tab ${tab === t.key ? "active" : ""}`} onClick={() => { setTab(t.key); setResult(null); setError(null); }}>
            {t.label}
          </button>
        ))}
      </div>

      <div className="panel-controls">
        {(tab === "newsRag" || tab === "ragPackBuild" || tab === "ragPackQuery" || tab === "upstreamScan") && (
          <>
            <div className="form-row inline"><label htmlFor="newsCode">Code</label><input id="newsCode" type="text" value={newsCode} onChange={(e) => setNewsCode(e.target.value)} placeholder="300750.SZ" /></div>
            <div className="form-row inline"><label htmlFor="newsDays">Days</label><input id="newsDays" type="number" min="1" max="3650" value={newsDays} onChange={(e) => setNewsDays(Number(e.target.value) || 30)} /></div>
          </>
        )}
        {(tab === "ragPackBuild" || tab === "ragPackQuery") && (
          <div className="form-row"><label htmlFor="seedCodesForRag">Seed codes</label><input id="seedCodesForRag" type="text" value={seedCodes} onChange={(e) => setSeedCodes(e.target.value)} placeholder="Optional comma separated codes" /></div>
        )}
        {tab === "ragPackQuery" && (
          <div className="form-row"><label htmlFor="ragQuery">Query</label><input id="ragQuery" type="text" value={queryText} onChange={(e) => setQueryText(e.target.value)} placeholder="supply chain order evidence" /></div>
        )}
        {tab === "upstreamScan" && !isMobileTauriRuntime() && (
          <div className="form-row"><label htmlFor="manualUrls">Manual URLs</label><textarea id="manualUrls" rows={3} value={manualUrls} onChange={(e) => setManualUrls(e.target.value)} placeholder="Optional public source URLs, one per line" /></div>
        )}
        {tab === "upstreamImport" && (
          <div className="form-row"><label htmlFor="upstreamImportPayload">Descriptor</label><textarea id="upstreamImportPayload" rows={5} value={importPayload} onChange={(e) => setImportPayload(e.target.value)} placeholder="Paste QR JSON or manifest_url" /></div>
        )}
        <button type="button" className="run-btn" onClick={run} disabled={loading || scanning}>{loading || scanning ? "Running..." : "Run"}</button>
      </div>

      <div className="panel-result">
        {error && <div className="result-error"><strong>Request failed</strong><p>{escapeHtml(error)}</p></div>}
        {loading && !result && !error && <div className="result-loading"><div className="loader" /><span>Working...</span></div>}
        {result != null && !loading && <NewsResult tab={tab} result={result} />}
        {!result && !loading && !error && <div className="result-empty"><span>Configure parameters, then run.</span></div>}
      </div>
    </div>
  );
}

function NewsResult({ tab, result }: { tab: NewsTab; result: unknown }) {
  if (tab === "newsRag") return <NewsRagView result={result as NewsRagResult} />;
  if (tab === "ragPackQuery") return <RagPackQueryView result={result as RagPackQueryResult} />;
  if (tab === "upstreamScan") return <UpstreamBuildView result={result as { build?: UpstreamRagBuildResult; transfer?: UpstreamRagTransferResult | null }} />;
  return <GenericJsonResult result={result} />;
}

function NewsRagView({ result }: { result: NewsRagResult }) {
  const groups = normalizeNewsGroups(result.sentiment_groups);
  const findings = result.findings || [];
  const secondary = [...groups.mixed, ...groups.uncertain];

  return (
    <div className="news-rag-result">
      <div className="metric-strip">
        <div className="metric"><span>Scope</span><strong>{result.scope_codes?.length ?? 0}</strong></div>
        <div className="metric"><span>Relations</span><strong>{result.relation_count ?? 0}</strong></div>
        <div className="metric"><span>Messages</span><strong>{result.message_count ?? 0}</strong></div>
        <div className="metric"><span>Mode</span><strong>{groups.mode}</strong></div>
      </div>

      {findings.length > 0 && <Findings findings={findings} />}

      <div className="plain-news-groups">
        <EvidenceColumn title="Positive" tone="positive" items={groups.positive} />
        <EvidenceColumn title="Negative" tone="negative" items={groups.negative} />
        {secondary.length > 0 && (
          <details className="plain-news-secondary" open>
            <summary><span>Mixed / uncertain {secondary.length}</span><b className="plain-news-toggle" /></summary>
            <EvidenceList items={secondary} />
          </details>
        )}
      </div>

      {result.notes?.length ? <div className="notes">{result.notes.map((note) => <p key={note}>{note}</p>)}</div> : null}
      <details className="raw-json"><summary>Raw JSON</summary><pre>{JSON.stringify(result, null, 2)}</pre></details>
    </div>
  );
}

function Findings({ findings }: { findings: NewsImpactFinding[] }) {
  return (
    <div className="news-findings">
      {findings.map((finding, index) => (
        <section key={`${finding.target}-${index}`} className="news-finding">
          <header>
            <div><h3>{finding.target}</h3><p>{finding.impact_chain}</p></div>
            <span className={`impact-pill ${finding.direction}`}>{finding.direction}</span>
          </header>
          <div className="finding-meta"><span>Confidence {finding.confidence}</span><span>Evidence {finding.evidence?.length ?? 0}</span></div>
          <EvidenceList items={finding.evidence || []} />
          {finding.pending_checks?.length ? <div className="checklist">{finding.pending_checks.map((item) => <span key={item}>{item}</span>)}</div> : null}
        </section>
      ))}
    </div>
  );
}

function EvidenceColumn({ title, tone, items }: { title: string; tone: string; items: NewsEvidence[] }) {
  return (
    <section className={`plain-news-column ${tone}`}>
      <header><h3>{title}</h3><span>{items.length}</span></header>
      <EvidenceList items={items} />
    </section>
  );
}

function EvidenceList({ items }: { items: NewsEvidence[] }) {
  if (!items.length) return <div className="result-empty"><span>No evidence.</span></div>;
  return (
    <div className="evidence-list">
      {items.map((item, index) => (
        <article key={`${item.title}-${index}`}>
          <strong>{item.title || "--"}</strong>
          <span className="evidence-source">
            <span className={`source-tier ${item.source_tier || "news"}`}>{item.source_tier || "news"}</span>
            <span>{item.source || ""}</span>
            <span>{item.published_at || ""}</span>
          </span>
          {item.summary && <p>{item.summary}</p>}
          {item.url && <a className="evidence-link" href={item.url} target="_blank" rel="noreferrer">Source</a>}
        </article>
      ))}
    </div>
  );
}

function RagPackQueryView({ result }: { result: RagPackQueryResult }) {
  const hits = result.hits || [];
  return (
    <div className="rag-query-result">
      <div className="metric-strip"><div className="metric"><span>Hits</span><strong>{hits.length}</strong></div><div className="metric"><span>Version</span><strong>{String(result.manifest?.pack_version || "--")}</strong></div></div>
      <div className="evidence-list">
        {hits.map((hit, i) => {
          const item = normalizeRagHit(hit);
          return <article key={`${item.title}-${i}`}><strong>{item.title}</strong><span className="evidence-source">{item.source} score {item.score?.toFixed(3) ?? "--"}</span><p>{item.text}</p>{item.url && <a className="evidence-link" href={item.url} target="_blank" rel="noreferrer">Source</a>}</article>;
        })}
      </div>
      {result.notes?.length ? <div className="notes">{result.notes.map((note) => <p key={note}>{note}</p>)}</div> : null}
    </div>
  );
}

function UpstreamBuildView({ result }: { result: { build?: UpstreamRagBuildResult; transfer?: UpstreamRagTransferResult | null } }) {
  const build = result.build || {};
  const manifest = build.manifest || {};
  const transfer = result.transfer;
  return (
    <div className="upstream-result">
      <div className="metric-strip">
        <div className="metric"><span>Valid</span><strong>{manifest.valid ? "yes" : "no"}</strong></div>
        <div className="metric"><span>Docs</span><strong>{String(manifest.document_count ?? 0)}</strong></div>
        <div className="metric"><span>Evidence</span><strong>{String(manifest.evidence_count ?? 0)}</strong></div>
        <div className="metric"><span>Relations</span><strong>{String(manifest.relation_edge_count ?? 0)}</strong></div>
      </div>
      <section className="upstream-transfer">
        <header>
          <div><h3>{String(manifest.target_stock_name || "Upstream RAG")} {String(manifest.target_stock_code || "")}</h3><p>{String(manifest.pack_version || "")} · {formatBytes(manifest.file_size)}</p></div>
          {transfer?.qr_svg && <img src={transfer.qr_svg} alt="Upstream RAG transfer QR" />}
        </header>
        {transfer && <div className="detail-grid"><div><span>Manifest</span><strong>{transfer.manifest_url}</strong></div><div><span>Pack</span><strong>{transfer.pack_url}</strong></div><div><span>Expires</span><strong>{formatDateTime(transfer.expires_at)}</strong></div></div>}
        {transfer?.descriptor_json && <textarea className="upstream-inline-descriptor" readOnly rows={4} value={transfer.descriptor_json} />}
      </section>
      <RelationGraph manifest={manifest} />
      {build.notes?.length ? <div className="notes">{build.notes.map((note) => <p key={note}>{note}</p>)}</div> : null}
      <details className="raw-json"><summary>Raw JSON</summary><pre>{JSON.stringify(result, null, 2)}</pre></details>
    </div>
  );
}

function RelationGraph({ manifest }: { manifest: Record<string, unknown> }) {
  const edges = Array.isArray(manifest.relation_edges) ? manifest.relation_edges as Record<string, unknown>[] : [];
  const chunks = Array.isArray(manifest.evidence_chunks) ? manifest.evidence_chunks as Record<string, unknown>[] : [];
  return (
    <>
      {edges.length > 0 && <section className="upstream-graph"><header><h3>Relation graph</h3><span>{formatNumber(edges.length)}</span></header><div className="relation-map">{edges.slice(0, 18).map((edge, i) => <article key={i} className="relation-edge"><span>{String((edge.source_entity as Record<string, unknown>)?.entity_name || edge.source_code || "--")}</span><strong>{String(edge.relation_type || "--")}</strong><span>{String((edge.target_entity as Record<string, unknown>)?.entity_name || edge.target_code || "--")}</span></article>)}</div></section>}
      {chunks.length > 0 && <section className="upstream-evidence"><header><h3>Evidence</h3><span>{chunks.length}</span></header><div className="evidence-list">{chunks.slice(0, 24).map((chunk, i) => <article key={i}><strong>{String(chunk.title || "--")}</strong><p>{String(chunk.evidence_text || chunk.text || "")}</p></article>)}</div></section>}
    </>
  );
}

function GenericJsonResult({ result }: { result: unknown }) {
  return <pre className="raw-result">{JSON.stringify(result, null, 2)}</pre>;
}

async function scanQrCode(): Promise<string> {
  if (!navigator.mediaDevices?.getUserMedia) throw new Error("Camera is not available in this WebView.");
  const stream = await navigator.mediaDevices.getUserMedia({ video: { facingMode: { ideal: "environment" } } });
  const video = document.createElement("video");
  video.playsInline = true;
  video.muted = true;
  video.srcObject = stream;
  await video.play();
  const canvas = document.createElement("canvas");
  try {
    const detectorCtor = (window as unknown as { BarcodeDetector?: new (options: { formats: string[] }) => { detect(video: HTMLVideoElement): Promise<{ rawValue?: string }[]> } }).BarcodeDetector;
    const detector = detectorCtor ? new detectorCtor({ formats: ["qr_code"] }) : null;
    for (let i = 0; i < 250; i++) {
      if (detector) {
        const codes = await detector.detect(video).catch(() => []);
        if (codes[0]?.rawValue) return codes[0].rawValue;
      } else {
        const raw = detectWithJsQr(video, canvas);
        if (raw) return raw;
      }
      await new Promise((resolve) => window.setTimeout(resolve, 200));
    }
    throw new Error("No QR code detected.");
  } finally {
    stream.getTracks().forEach((track) => track.stop());
  }
}

function detectWithJsQr(video: HTMLVideoElement, canvas: HTMLCanvasElement): string {
  const jsQR = (window as unknown as { jsQR?: (data: Uint8ClampedArray, width: number, height: number, options?: Record<string, unknown>) => { data?: string } | null }).jsQR;
  if (!jsQR || !video.videoWidth || !video.videoHeight) return "";
  const maxDimension = 900;
  const scale = Math.min(1, maxDimension / Math.max(video.videoWidth, video.videoHeight));
  const width = Math.max(1, Math.round(video.videoWidth * scale));
  const height = Math.max(1, Math.round(video.videoHeight * scale));
  canvas.width = width;
  canvas.height = height;
  const context = canvas.getContext("2d");
  if (!context) return "";
  context.drawImage(video, 0, 0, width, height);
  const image = context.getImageData(0, 0, width, height);
  return jsQR(image.data, width, height, { inversionAttempts: "attemptBoth" })?.data || "";
}
