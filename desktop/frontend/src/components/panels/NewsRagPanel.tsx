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
import { CollapsibleNotes } from "../CollapsibleNotes";
import { StockCodeInput } from "../StockCodeInput";

type NewsTab = "newsRag" | "ragPackBuild" | "ragPackQuery" | "upstreamScan" | "upstreamImport";

const TABS: { key: NewsTab; label: string }[] = [
  { key: "newsRag", label: "新闻 RAG" },
  { key: "ragPackBuild", label: "构建包" },
  { key: "ragPackQuery", label: "查询包" },
  { key: "upstreamScan", label: "上游同步" },
  { key: "upstreamImport", label: "移动导入" },
];

interface NewsRagPanelProps {
  llmSettings?: LlmSettings | null;
}

interface UpstreamMobileListResult {
  root?: string;
  packs?: Record<string, unknown>[];
  notes?: string[];
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


  const run = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      if (tab === "newsRag") {
        if (!normalizedCode) throw new Error("请输入有效股票代码。");
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
          if (!normalizedCode) throw new Error("桌面端构建上游同步包需要目标股票代码。");
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
  }, [importPayload, llmSettings, manualUrls, newsDays, normalizedCode, queryText, scanAndImport, seedCodes, tab]);

  const listMobilePacks = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const data = isMobileTauriRuntime()
        ? await getJson<UpstreamMobileListResult>("/api/upstream-rag/mobile/list")
        : await getJson("/api/upstream-rag/status");
      setResult(data);
    } catch (err) {
      setError((err as Error).message);
    } finally {
      setLoading(false);
    }
  }, []);

  const showMobilePackDetail = useCallback(async (stockCode: string, packVersion = "") => {
    setLoading(true);
    setError(null);
    try {
      const params = new URLSearchParams({ stock_code: stockCode, pack_version: packVersion });
      const data = await getJson(`/api/upstream-rag/mobile/detail?${params}`);
      setResult(data);
    } catch (err) {
      setError((err as Error).message);
    } finally {
      setLoading(false);
    }
  }, []);

  const rollbackMobilePack = useCallback(async (stockCode: string) => {
    setLoading(true);
    setError(null);
    try {
      const data = await postJson("/api/upstream-rag/mobile/rollback", { stock_code: stockCode });
      setResult(data);
    } catch (err) {
      setError((err as Error).message);
    } finally {
      setLoading(false);
    }
  }, []);

  return (
    <div className="panel-container">
      <div className="panel-tabs rag-tabs">
        {TABS.map((t) => (
          <button key={t.key} type="button" className={`panel-tab ${tab === t.key ? "active" : ""}`} onClick={() => { setTab(t.key); setResult(null); setError(null); }}>
            {t.label}
          </button>
        ))}
      </div>

      <div className="panel-controls rag-controls">
        {(tab === "newsRag" || tab === "ragPackBuild" || tab === "ragPackQuery" || tab === "upstreamScan") && (
          <>
            <div className="form-row inline stock-code-row rag-code-field"><label htmlFor="newsCode">股票代码</label><StockCodeInput id="newsCode" value={newsCode} onChange={setNewsCode} placeholder="输入股票代码或名称" /></div>
            <div className="form-row inline rag-days-field"><label htmlFor="newsDays">天数</label><input id="newsDays" type="number" min="1" max="3650" value={newsDays} onChange={(e) => setNewsDays(Number(e.target.value) || 30)} /></div>
          </>
        )}
        {(tab === "ragPackBuild" || tab === "ragPackQuery") && (
          <div className="form-row rag-wide-field"><label htmlFor="seedCodesForRag">种子股票</label><StockCodeInput id="seedCodesForRag" value={seedCodes} onChange={setSeedCodes} placeholder="可选，多个股票用逗号分隔" listMode /></div>
        )}
        {tab === "ragPackQuery" && (
          <div className="form-row rag-wide-field"><label htmlFor="ragQuery">查询问题</label><input id="ragQuery" type="text" value={queryText} onChange={(e) => setQueryText(e.target.value)} placeholder="例如：供应链订单证据" /></div>
        )}
        {tab === "upstreamScan" && !isMobileTauriRuntime() && (
          <div className="form-row rag-textarea-field"><label htmlFor="manualUrls">手动来源 URL</label><textarea id="manualUrls" rows={3} value={manualUrls} onChange={(e) => setManualUrls(e.target.value)} placeholder="可选公共来源链接，每行一个" /></div>
        )}
        {tab === "upstreamImport" && (
          <div className="form-row rag-textarea-field"><label htmlFor="upstreamImportPayload">导入描述</label><textarea id="upstreamImportPayload" rows={5} value={importPayload} onChange={(e) => setImportPayload(e.target.value)} placeholder="粘贴二维码 JSON 或 manifest_url" /></div>
        )}
        <button type="button" className="run-btn rag-run-btn" onClick={run} disabled={loading || scanning}>{loading || scanning ? "运行中..." : "运行"}</button>
        {(tab === "upstreamScan" || tab === "upstreamImport") && (
          <button type="button" className="action-btn rag-secondary-btn" onClick={listMobilePacks} disabled={loading || scanning}>查看导入包</button>
        )}
      </div>

      <div className="panel-result">
        {error && <div className="result-error"><strong>请求失败</strong><p>{escapeHtml(error)}</p></div>}
        {loading && !result && !error && <div className="result-loading"><div className="loader" /><span>处理中...</span></div>}
        {result != null && !loading && <NewsResult tab={tab} result={result} onDetail={showMobilePackDetail} onRollback={rollbackMobilePack} />}
        {!result && !loading && !error && <div className="result-empty"><span>设置参数后运行。</span></div>}
      </div>
    </div>
  );
}

function NewsResult({
  tab,
  result,
  onDetail,
  onRollback,
}: {
  tab: NewsTab;
  result: unknown;
  onDetail: (stockCode: string, packVersion?: string) => void;
  onRollback: (stockCode: string) => void;
}) {
  const record = asRecord(result);
  if (tab === "newsRag") return <NewsRagView result={result as NewsRagResult} />;
  if (tab === "ragPackQuery") return <RagPackQueryView result={result as RagPackQueryResult} />;
  if (tab === "upstreamScan" && (record.build || record.transfer)) return <UpstreamBuildView result={result as { build?: UpstreamRagBuildResult; transfer?: UpstreamRagTransferResult | null }} />;
  if (Array.isArray(record.packs)) return <UpstreamMobileListView result={record as UpstreamMobileListResult} onDetail={onDetail} onRollback={onRollback} />;
  if (record.manifest && !record.imported && !record.rolled_back) return <UpstreamDetailView result={record} />;
  if (record.imported || record.rolled_back) return <UpstreamImportView result={record} />;
  return <GenericJsonResult result={result} />;
}

export function NewsRagView({ result }: { result: NewsRagResult }) {
  const groups = normalizeNewsGroups(result.sentiment_groups);
  const findings = result.findings || [];
  const secondary = [...groups.mixed, ...groups.uncertain];

  return (
    <div className="news-rag-result">
      <div className="metric-strip">
        <div className="metric"><span>范围</span><strong>{result.scope_codes?.length ?? 0}</strong></div>
        <div className="metric"><span>关系</span><strong>{result.relation_count ?? 0}</strong></div>
        <div className="metric"><span>消息</span><strong>{result.message_count ?? 0}</strong></div>
        <div className="metric"><span>模式</span><strong>{newsModeLabel(groups.mode)}</strong></div>
      </div>

      {findings.length > 0 && <Findings findings={findings} />}

      <div className="plain-news-groups">
        <EvidenceColumn title="利好" tone="positive" items={groups.positive} />
        <EvidenceColumn title="利空" tone="negative" items={groups.negative} />
        {secondary.length > 0 && (
          <details className="plain-news-secondary" open>
            <summary><span>混合 / 不确定 {secondary.length}</span><b className="plain-news-toggle" /></summary>
            <EvidenceList items={secondary} />
          </details>
        )}
      </div>

      <CollapsibleNotes notes={result.notes || []} />
      <details className="raw-json"><summary>原始 JSON</summary><pre>{JSON.stringify(result, null, 2)}</pre></details>
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
            <span className={`impact-pill ${finding.direction}`}>{directionLabel(finding.direction)}</span>
          </header>
          <div className="finding-meta"><span>置信度 {finding.confidence}</span><span>证据 {finding.evidence?.length ?? 0}</span></div>
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
  if (!items.length) return <div className="result-empty"><span>暂无证据。</span></div>;
  return (
    <div className="evidence-list">
      {items.map((item, index) => (
        <article key={`${item.title}-${index}`}>
          <strong>{item.title || "--"}</strong>
          <span className="evidence-source">
            <span className={`source-tier ${item.source_tier || "news"}`}>{sourceTierLabel(item.source_tier)}</span>
            <span>{item.source || ""}</span>
            <span>{item.published_at || ""}</span>
          </span>
          {item.summary && <p>{item.summary}</p>}
          {item.url && <a className="evidence-link" href={item.url} target="_blank" rel="noreferrer">来源</a>}
        </article>
      ))}
    </div>
  );
}

function RagPackQueryView({ result }: { result: RagPackQueryResult }) {
  const hits = result.hits || [];
  return (
    <div className="rag-query-result">
      <div className="metric-strip"><div className="metric"><span>命中</span><strong>{hits.length}</strong></div><div className="metric"><span>版本</span><strong>{String(result.manifest?.pack_version || "--")}</strong></div></div>
      <div className="evidence-list">
        {hits.map((hit, i) => {
          const item = normalizeRagHit(hit);
          return <article key={`${item.title}-${i}`}><strong>{item.title}</strong><span className="evidence-source">{item.source} 分数 {item.score?.toFixed(3) ?? "--"}</span><p>{item.text}</p>{item.url && <a className="evidence-link" href={item.url} target="_blank" rel="noreferrer">来源</a>}</article>;
        })}
      </div>
      <CollapsibleNotes notes={result.notes || []} />
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
        <div className="metric"><span>有效</span><strong>{manifest.valid ? "是" : "否"}</strong></div>
        <div className="metric"><span>文档</span><strong>{String(manifest.document_count ?? 0)}</strong></div>
        <div className="metric"><span>证据</span><strong>{String(manifest.evidence_count ?? 0)}</strong></div>
        <div className="metric"><span>关系</span><strong>{String(manifest.relation_edge_count ?? 0)}</strong></div>
      </div>
      <section className="upstream-transfer">
        <header>
          <div><h3>{String(manifest.target_stock_name || "上游 RAG")} {String(manifest.target_stock_code || "")}</h3><p>{String(manifest.pack_version || "")} - {formatBytes(manifest.file_size)}</p></div>
          {transfer?.qr_svg && <img src={transfer.qr_svg} alt="上游同步二维码" />}
        </header>
        {transfer && <div className="detail-grid"><div><span>清单</span><strong>{transfer.manifest_url}</strong></div><div><span>包文件</span><strong>{transfer.pack_url}</strong></div><div><span>过期时间</span><strong>{formatDateTime(transfer.expires_at)}</strong></div></div>}
        {transfer?.descriptor_json && <textarea className="upstream-inline-descriptor" readOnly rows={4} value={transfer.descriptor_json} />}
      </section>
      <RelationGraph manifest={manifest} />
      <CollapsibleNotes notes={build.notes || []} />
      <details className="raw-json"><summary>原始 JSON</summary><pre>{JSON.stringify(result, null, 2)}</pre></details>
    </div>
  );
}

function UpstreamMobileListView({
  result,
  onDetail,
  onRollback,
}: {
  result: UpstreamMobileListResult;
  onDetail: (stockCode: string, packVersion?: string) => void;
  onRollback: (stockCode: string) => void;
}) {
  const packs = result.packs || [];
  if (!packs.length) return <div className="result-empty"><span>{result.notes?.[0] || "暂无已导入的移动端 RAG 包。"}</span></div>;
  return (
    <div className="upstream-mobile-list">
      <div className="metric-strip">
        <div className="metric"><span>包数量</span><strong>{packs.length}</strong></div>
        <div className="metric"><span>目录</span><strong>{result.root || "--"}</strong></div>
      </div>
      <div className="rag-pack-list">
        {packs.map((pack, index) => {
          const stockCode = String(pack.target_stock_code || "");
          const version = String(pack.pack_version || "");
          return (
            <article key={`${stockCode}-${version}-${index}`} className="rag-pack-item">
              <header>
                <div><h3>{String(pack.target_stock_name || stockCode || "RAG 包")}</h3><p>{stockCode} {version}</p></div>
                <span className={`pack-state ${pack.current ? "current" : "archived"}`}>{pack.current ? "当前" : "历史"}</span>
              </header>
              <div className="detail-grid">
                <div><span>文档</span><strong>{String(pack.document_count ?? 0)}</strong></div>
                <div><span>证据</span><strong>{String(pack.evidence_count ?? 0)}</strong></div>
                <div><span>关系</span><strong>{String(pack.relation_edge_count ?? 0)}</strong></div>
                <div><span>大小</span><strong>{formatBytes(pack.file_size)}</strong></div>
              </div>
              <div className="upstream-pack-actions">
                <button type="button" className="action-btn" onClick={() => onDetail(stockCode, version)}>详情</button>
                <button type="button" className="action-btn" onClick={() => onRollback(stockCode)}>回滚</button>
              </div>
            </article>
          );
        })}
      </div>
      <CollapsibleNotes notes={result.notes || []} />
    </div>
  );
}

function UpstreamImportView({ result }: { result: Record<string, unknown> }) {
  const manifest = asRecord(result.manifest);
  return (
    <div className="upstream-result">
      <div className="metric-strip">
        <div className="metric"><span>导入</span><strong>{result.imported ? "已导入" : result.rolled_back ? "已回滚" : "已更新"}</strong></div>
        <div className="metric"><span>股票</span><strong>{String(result.stock_code || manifest.target_stock_code || "--")}</strong></div>
        <div className="metric"><span>版本</span><strong>{String(result.pack_version || manifest.pack_version || "--")}</strong></div>
        <div className="metric"><span>文档</span><strong>{String(manifest.document_count ?? 0)}</strong></div>
      </div>
      <RelationGraph manifest={manifest} />
      <CollapsibleNotes notes={Array.isArray(result.notes) ? result.notes.map((note) => String(note)) : []} />
      <details className="raw-json"><summary>原始 JSON</summary><pre>{JSON.stringify(result, null, 2)}</pre></details>
    </div>
  );
}

function UpstreamDetailView({ result }: { result: Record<string, unknown> }) {
  const manifest = asRecord(result.manifest || result);
  return (
    <div className="upstream-result">
      <div className="metric-strip">
        <div className="metric"><span>目标</span><strong>{String(manifest.target_stock_code || "--")}</strong></div>
        <div className="metric"><span>版本</span><strong>{String(manifest.pack_version || "--")}</strong></div>
        <div className="metric"><span>证据</span><strong>{String(manifest.evidence_count ?? 0)}</strong></div>
        <div className="metric"><span>大小</span><strong>{formatBytes(manifest.file_size)}</strong></div>
      </div>
      <section className="upstream-transfer">
        <header><div><h3>{String(manifest.target_stock_name || "上游 RAG")}</h3><p>{String(manifest._local_pack_path || "")}</p></div></header>
        <div className="detail-grid">
          <div><span>sha256</span><strong>{String(manifest.sha256 || "--")}</strong></div>
          <div><span>导入时间</span><strong>{formatDateTime(manifest.imported_at || manifest.generated_at)}</strong></div>
        </div>
      </section>
      <RelationGraph manifest={manifest} />
      <details className="raw-json"><summary>原始 JSON</summary><pre>{JSON.stringify(result, null, 2)}</pre></details>
    </div>
  );
}

function RelationGraph({ manifest }: { manifest: Record<string, unknown> }) {
  const edges = Array.isArray(manifest.relation_edges) ? manifest.relation_edges as Record<string, unknown>[] : [];
  const chunks = Array.isArray(manifest.evidence_chunks) ? manifest.evidence_chunks as Record<string, unknown>[] : [];
  return (
    <>
      {edges.length > 0 && <section className="upstream-graph"><header><h3>关系图</h3><span>{formatNumber(edges.length)}</span></header><div className="relation-map">{edges.slice(0, 18).map((edge, i) => <article key={i} className="relation-edge"><span>{String(asRecord(edge.source_entity).entity_name || edge.source_code || "--")}</span><strong>{String(edge.relation_type || "--")}</strong><span>{String(asRecord(edge.target_entity).entity_name || edge.target_code || "--")}</span></article>)}</div></section>}
      {chunks.length > 0 && <section className="upstream-evidence"><header><h3>证据</h3><span>{chunks.length}</span></header><div className="evidence-list">{chunks.slice(0, 24).map((chunk, i) => <article key={i}><strong>{String(chunk.title || "--")}</strong><p>{String(chunk.evidence_text || chunk.text || "")}</p></article>)}</div></section>}
    </>
  );
}

function GenericJsonResult({ result }: { result: unknown }) {
  return <pre className="raw-result">{JSON.stringify(result, null, 2)}</pre>;
}

async function scanQrCode(): Promise<string> {
  if (!navigator.mediaDevices?.getUserMedia) throw new Error("当前 WebView 不支持摄像头。");
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
    throw new Error("未识别到二维码。");
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

function asRecord(value: unknown): Record<string, unknown> {
  return value && typeof value === "object" && !Array.isArray(value) ? value as Record<string, unknown> : {};
}

function newsModeLabel(mode?: string): string {
  const labels: Record<string, string> = {
    plain_news: "新闻分组",
    rag: "RAG 检索",
    direct: "直接分析",
    cache: "缓存分析",
  };
  return labels[String(mode || "plain_news")] || String(mode || "--");
}

function directionLabel(direction: string): string {
  const labels: Record<string, string> = {
    positive: "利好",
    negative: "利空",
    mixed: "混合",
    uncertain: "不确定",
    bullish: "看多",
    bearish: "看空",
    neutral: "中性",
  };
  return labels[String(direction || "")] || String(direction || "--");
}

function sourceTierLabel(tier?: string): string {
  const labels: Record<string, string> = {
    filing: "公告",
    news: "新闻",
    community: "社区",
    research: "研报",
  };
  return labels[String(tier || "news")] || String(tier || "新闻");
}
