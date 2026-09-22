import { useCallback, useEffect, useRef, useState } from "react";
import { postJson } from "../lib/tauri";
import type { LlmClientConfig } from "../types";
import type { SentimentAnalysis, SentimentFollowup, SentimentRun, SentimentSnapshot } from "../types/sentiment";

const message = (error: unknown) => error instanceof Error ? error.message : String(error);

export function useSentiment(code: string, watchlistCodes: readonly string[] = []) {
  const [snapshot, setSnapshot] = useState<SentimentSnapshot | null>(null);
  const [latest, setLatest] = useState<SentimentAnalysis | null>(null);
  const [analysis, setAnalysis] = useState<SentimentAnalysis | null>(null);
  const [history, setHistory] = useState<SentimentAnalysis[]>([]);
  const [stages, setStages] = useState<SentimentAnalysis[]>([]);
  const [run, setRun] = useState<SentimentRun | null>(null);
  const [loading, setLoading] = useState(false);
  const [starting, setStarting] = useState(false);
  const [error, setError] = useState("");
  const [stageError, setStageError] = useState("");
  const [asking, setAsking] = useState(false);
  const [followups, setFollowups] = useState<Array<SentimentFollowup & { question: string }>>([]);
  const generation = useRef(0);
  const loadedCode = useRef<string | null>(null);
  const operation = useRef(0);
  const startingRef = useRef(false);
  const askingRef = useRef(false);
  const activeRun = useRef<string | null>(null);
  const dismissedRun = useRef<string | null>(null);
  const analysisRef = useRef<SentimentAnalysis | null>(null);
  const [reload, setReload] = useState(0);
  const stageKey = [...new Set(watchlistCodes.filter(Boolean))].sort().join("|");

  const abandon = (runId: string | null) => {
    if (!runId) return;
    dismissedRun.current = runId;
    if (activeRun.current === runId) activeRun.current = null;
    void postJson("/api/sentiment/cancel", { run_id: runId }).catch(() => undefined);
  };

  useEffect(() => {
    const current = ++generation.current;
    operation.current += 1;
    startingRef.current = false;
    askingRef.current = false;
    const codeChanged = loadedCode.current !== code;
    loadedCode.current = code;
    if (codeChanged) {
      abandon(activeRun.current);
      analysisRef.current = null;
      setSnapshot(null);
      setLatest(null);
      setAnalysis(null);
      setHistory([]);
      setFollowups([]);
      setRun(null);
    }
    setStarting(false);
    setAsking(false);
    setError("");
    if (!code) {
      setLoading(false);
      return;
    }
    setLoading(true);
    const request = { stock_code: code, window_days: 30 };
    void Promise.allSettled([
      postJson<SentimentSnapshot>("/api/sentiment/snapshot", request),
      postJson<{ analysis: SentimentAnalysis | null }>("/api/sentiment/latest", { stock_code: code }),
      postJson<{ items: SentimentAnalysis[] }>("/api/sentiment/history", { stock_code: code }),
    ]).then(([fresh, previous, past]) => {
      if (generation.current !== current) return;
      if (fresh.status === "fulfilled") setSnapshot(fresh.value);
      if (previous.status === "fulfilled") {
        setLatest(previous.value.analysis);
        setAnalysis(previous.value.analysis);
        analysisRef.current = previous.value.analysis;
      }
      if (past.status === "fulfilled") setHistory(past.value.items);
      const failures = [fresh, previous, past].filter((result) => result.status === "rejected");
      if (failures.length) setError(`部分数据未能加载：${failures.map((result) => message(result.reason)).join("；")}`);
      setLoading(false);
    });
    return () => {
      generation.current += 1;
      abandon(activeRun.current);
    };
  }, [code, reload]);

  useEffect(() => {
    const codes = stageKey ? stageKey.split("|") : [];
    if (!codes.length) {
      setStages([]);
      setStageError("");
      return;
    }
    let disposed = false;
    void postJson<{ items: SentimentAnalysis[] }>("/api/sentiment/history", { stock_codes: codes })
      .then((result) => {
        if (disposed) return;
        setStages(result.items);
        setStageError("");
      })
      .catch((failure: unknown) => {
        if (disposed) return;
        setStageError(`自选阶段未能加载：${message(failure)}`);
      });
    return () => {
      disposed = true;
    };
  }, [stageKey, reload]);

  const runId = run?.status === "running" ? run.run_id : null;
  useEffect(() => {
    if (!runId) return;
    const current = generation.current;
    let disposed = false;
    let timer: ReturnType<typeof setTimeout> | undefined;
    const poll = async () => {
      try {
        const next = await postJson<SentimentRun>("/api/sentiment/status", { run_id: runId });
        if (disposed || current !== generation.current || dismissedRun.current === runId || activeRun.current !== runId) return;
        setRun((previous) => previous?.run_id === runId && previous.status === "cancelled" ? previous : next);
        if (next.status === "running") {
          timer = setTimeout(() => void poll(), 1000);
          return;
        }
        activeRun.current = null;
        if (next.status === "completed" && next.result) {
          analysisRef.current = next.result;
          setLatest(next.result);
          setAnalysis(next.result);
          setFollowups([]);
          setHistory((items) => [next.result!, ...items.filter((item) => item.analysis_id !== next.result!.analysis_id)].slice(0, 30));
          setStages((items) => [next.result!, ...items.filter((item) => item.stock_code !== next.result!.stock_code)].slice(0, 200));
        } else if (next.status === "failed") setError(next.error || "模型分析失败；上次结果已保留。");
      } catch (failure) {
        if (disposed || current !== generation.current || dismissedRun.current === runId) return;
        setError(`无法获取进度：${message(failure)}。正在重试，可取消本次分析。`);
        timer = setTimeout(() => void poll(), 3000);
      }
    };
    void poll();
    return () => {
      disposed = true;
      clearTimeout(timer);
    };
  }, [runId]);

  const start = useCallback(async (llm: LlmClientConfig | undefined) => {
    if (!code || loading || startingRef.current || activeRun.current) return;
    if (!llm) {
      setError("请先在 API 设置中配置模型和连接信息。");
      return;
    }
    const current = generation.current;
    const op = ++operation.current;
    startingRef.current = true;
    setStarting(true);
    setError("");
    try {
      const result = await postJson<{ run_id: string }>("/api/sentiment/start", {
        stock_code: code,
        window_days: 30,
        llm,
      });
      if (current !== generation.current || op !== operation.current) {
        abandon(result.run_id);
        return;
      }
      dismissedRun.current = null;
      activeRun.current = result.run_id;
      setRun({ run_id: result.run_id, stock_code: code, status: "running", stage: "准备证据", progress: 0 });
    } catch (failure) {
      if (current === generation.current) setError(`分析未完成：${message(failure)}。上次结果已保留。`);
    } finally {
      if (current === generation.current) {
        startingRef.current = false;
        setStarting(false);
      }
    }
  }, [code, loading]);

  const cancel = useCallback(async () => {
    const id = activeRun.current;
    if (!id) return;
    const current = generation.current;
    dismissedRun.current = id;
    activeRun.current = null;
    setRun((previous) => previous?.run_id === id ? { ...previous, status: "cancelled", stage: "已取消" } : previous);
    try {
      const result = await postJson<{ cancelled: boolean }>("/api/sentiment/cancel", { run_id: id });
      if (current !== generation.current || dismissedRun.current !== id) return;
      if (!result.cancelled) {
        dismissedRun.current = null;
        const next = await postJson<SentimentRun>("/api/sentiment/status", { run_id: id });
        if (current !== generation.current) return;
        setRun(next);
        if (next.status === "completed" && next.result) {
          analysisRef.current = next.result;
          setLatest(next.result);
          setAnalysis(next.result);
          setFollowups([]);
        }
      }
    } catch (failure) {
      if (current === generation.current) setError(`取消失败：${message(failure)}`);
    }
  }, []);

  const selectAnalysis = useCallback((next: SentimentAnalysis) => {
    analysisRef.current = next;
    setAnalysis(next);
    setFollowups([]);
    setError("");
    askingRef.current = false;
    setAsking(false);
    operation.current += 1;
  }, []);

  const showLatest = useCallback(() => {
    setAnalysis(latest);
    analysisRef.current = latest;
    setFollowups([]);
    setError("");
    askingRef.current = false;
    setAsking(false);
    operation.current += 1;
  }, [latest]);

  const ask = useCallback(async (question: string, llm: LlmClientConfig | undefined) => {
    const target = analysisRef.current;
    if (!target || !question.trim() || askingRef.current) return false;
    if (!llm) {
      setError("请先在 API 设置中配置模型。");
      return false;
    }
    const current = generation.current;
    const op = operation.current;
    askingRef.current = true;
    setAsking(true);
    setError("");
    try {
      const answer = await postJson<SentimentFollowup>("/api/sentiment/followup", {
        analysis_id: target.analysis_id,
        question: question.trim(),
        llm,
      });
      if (current !== generation.current || op !== operation.current || analysisRef.current?.analysis_id !== target.analysis_id) return false;
      setFollowups((items) => [...items, { ...answer, question: question.trim() }]);
      return true;
    } catch (failure) {
      if (current === generation.current && op === operation.current) setError(`追问失败：${message(failure)}`);
      return false;
    } finally {
      if (current === generation.current && op === operation.current) {
        askingRef.current = false;
        setAsking(false);
      }
    }
  }, []);

  return {
    snapshot,
    latest,
    analysis,
    history,
    stages,
    run,
    loading,
    starting,
    error,
    stageError,
    asking,
    followups,
    start,
    cancel,
    ask,
    selectAnalysis,
    showLatest,
    retry: () => setReload((value) => value + 1),
  };
}
