const screenBtn = document.getElementById("screenBtn");
const graphBtn = document.getElementById("graphBtn");
const backtestBtn = document.getElementById("backtestBtn");
const agentBtn = document.getElementById("agentBtn");

const screenResult = document.getElementById("screenResult");
const graphResult = document.getElementById("graphResult");
const backtestResult = document.getElementById("backtestResult");
const agentResult = document.getElementById("agentResult");

screenBtn.addEventListener("click", async () => {
  screenResult.textContent = "筛选中...";
  const data = await postJson("/api/screen", buildCriteria({ limit: 30 }), screenResult);
  if (data) {
    screenResult.textContent = formatJson(data);
  }
});

graphBtn.addEventListener("click", async () => {
  graphResult.textContent = "关系传播中...";
  const payload = {
    criteria: buildCriteria({ limit: 100 }),
    seed_codes: parseCodes(document.getElementById("seedCodes").value),
    relation_depth: clampInt(document.getElementById("relationDepth").value, 1, 3, 1),
    relation_weight: clampFloat(document.getElementById("relationWeight").value, 0, 1, 0.4),
    limit: 20,
  };
  const data = await postJson("/api/graph-screen", payload, graphResult);
  if (data) {
    graphResult.textContent = formatJson(data);
  }
});

backtestBtn.addEventListener("click", async () => {
  backtestResult.textContent = "回测中...";
  const payload = {
    criteria: buildCriteria({ limit: 100 }),
    start_date: document.getElementById("btStart").value || "20200101",
    end_date: document.getElementById("btEnd").value || "20240101",
    top_n: clampInt(document.getElementById("btTopN").value, 1, 100, 10),
  };
  const data = await postJson("/api/backtest", payload, backtestResult);
  if (data) {
    backtestResult.textContent = formatJson(data);
  }
});

agentBtn.addEventListener("click", async () => {
  const message = document.getElementById("agentMsg").value.trim();
  if (!message) return;
  agentResult.textContent = "Agent 思考中...";
  const data = await postJson("/api/agent", { message }, agentResult);
  if (data) {
    agentResult.textContent = formatJson(data);
  }
});

function buildCriteria(overrides = {}) {
  return {
    industry: document.getElementById("industry").value.trim() || null,
    min_roe: readNumber("minRoe"),
    max_pe: readNumber("maxPe"),
    max_pb: readNumber("maxPb"),
    ...overrides,
  };
}

function readNumber(id) {
  const value = Number.parseFloat(document.getElementById(id).value);
  return Number.isFinite(value) ? value : null;
}

function parseCodes(raw) {
  return raw
    .split(/[,\s，]+/)
    .map((item) => item.trim().toUpperCase())
    .filter(Boolean);
}

function clampInt(raw, min, max, fallback) {
  const value = Number.parseInt(raw, 10);
  if (!Number.isFinite(value)) return fallback;
  return Math.min(Math.max(value, min), max);
}

function clampFloat(raw, min, max, fallback) {
  const value = Number.parseFloat(raw);
  if (!Number.isFinite(value)) return fallback;
  return Math.min(Math.max(value, min), max);
}

async function postJson(url, payload, resultNode) {
  try {
    const resp = await fetch(url, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(payload),
    });
    if (!resp.ok) {
      const text = await resp.text();
      resultNode.textContent = `请求失败: ${resp.status} ${text}`;
      return null;
    }
    return await resp.json();
  } catch (err) {
    resultNode.textContent = `请求异常: ${err.message}`;
    return null;
  }
}

function formatJson(data) {
  return JSON.stringify(data, null, 2);
}
