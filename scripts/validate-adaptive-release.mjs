#!/usr/bin/env node

const option = (name, fallback) => {
  const index = process.argv.indexOf(name);
  return index < 0 ? fallback : process.argv[index + 1];
};
const endpoint = option(`--cdp-endpoint`, `http://127.0.0.1:9223`).replace(/\/$/, ``);
const now = new Date();
const endDate = option(
  `--end-date`,
  [now.getFullYear(), now.getMonth() + 1, now.getDate()]
    .map((value, index) => String(value).padStart(index ? 2 : 4, `0`))
    .join(``),
);
const backtestOnly = process.argv.includes(`--backtest-only`);
const screensOnly = process.argv.includes(`--screens-only`);
const sleep = (milliseconds) => new Promise((resolve) => setTimeout(resolve, milliseconds));

class CdpClient {
  constructor(url) {
    this.id = 0;
    this.pending = new Map();
    this.socket = new WebSocket(url);
  }

  async connect() {
    await new Promise((resolve) => this.socket.addEventListener(`open`, resolve, { once: true }));
    this.socket.addEventListener(`message`, (event) => {
      const message = JSON.parse(String(event.data));
      const pending = this.pending.get(message.id);
      if (pending == null) return;
      this.pending.delete(message.id);
      if (message.error) pending.reject(new Error(message.error.message));
      else pending.resolve(message.result);
    });
  }

  send(method, params = {}) {
    const id = ++this.id;
    this.socket.send(JSON.stringify({ id, method, params }));
    return new Promise((resolve, reject) => this.pending.set(id, { resolve, reject }));
  }

  close() {
    this.socket.close();
  }
}

const discover = async () => {
  const response = await fetch(`${endpoint}/json/list`);
  if (response.ok !== true) throw new Error(`CDP discovery failed: ${response.status}`);
  const targets = await response.json();
  return targets.find((target) => target.type === `page` && target.webSocketDebuggerUrl);
};

const invoke = async (client, command, payload) => {
  const args = JSON.stringify({ payload });
  const expression = `(async()=>{try{return {ok:true,value:await window.__TAURI_INTERNALS__.invoke(${JSON.stringify(command)},${args})}}catch(error){return {ok:false,error:String(error)}}})()`;
  const evaluated = await client.send(`Runtime.evaluate`, {
    expression,
    awaitPromise: true,
    returnByValue: true,
  });
  if (evaluated.exceptionDetails) {
    const details = evaluated.exceptionDetails;
    throw new Error(details.exception?.description || details.text);
  }
  const response = evaluated.result?.value;
  if (response?.ok !== true) throw new Error(response?.error || `${command} failed`);
  return response.value;
};

const criteria = (limit) => ({
  include_st: false,
  require_institution_buy_ratio_gt_sell_ratio: false,
  limit,
  sort_by: `score`,
  sort_dir: `desc`,
  score_profile: `balanced`,
});

const screenPayload = (runId, force = true) => ({
  criteria: criteria(80),
  mode: `auto`,
  horizon: `swing_10_30d`,
  primary_limit: 10,
  exploration_limit: 10,
  run_id: runId,
  ...(force ? { internal_algorithm: `adaptive_swing_v1` } : {}),
});

const backtestPayload = {
  internal_release_validation: true,
  source: `criteria`,
  criteria: criteria(100),
  strategy_mode: `adaptive_swing_v1:auto`,
  stock_codes: [],
  start_date: `20200101`,
  end_date: endDate,
  top_n: 10,
  initial_cash: 1_000_000,
  rebalance_frequency: `monthly`,
  transaction_cost_bps: 10,
  benchmark: `candidate_equal_weight`,
};

const main = async () => {
  const target = await discover();
  if (target == null) throw new Error(`no WebView2 page target is available`);
  const client = new CdpClient(target.webSocketDebuggerUrl);
  await client.connect();
  await client.send(`Runtime.enable`);

  try {
    for (let index = 0; index < (backtestOnly ? 0 : 5); index += 1) {
      const id = `release-validation-${Date.now()}-${index + 1}`;
      const startedAt = Date.now();
      const payload = screenPayload(id);
      if (index === 0) payload.internal_release_validation_cold_start = true;
      const result = await invoke(client, `api_screen`, payload);
      if (result?.algorithm_version !== `adaptive_swing_v1`) {
        throw new Error(`screen ${index + 1} did not use adaptive_swing_v1`);
      }
      console.log(`[adaptive-release] screen ${index + 1}/5: ${Date.now() - startedAt}ms`);
      await sleep(500);
    }
    await sleep(1_500);
    if (screensOnly) {
      console.log(`[adaptive-release] screen evidence recorded`);
      return;
    }

    console.log(`[adaptive-release] strict backtest through ${endDate}`);
    const backtest = await invoke(client, `api_backtest`, backtestPayload);
    const gate = backtest?.adaptive_release_gate;
    if (gate == null) throw new Error(`backtest did not return a release gate report`);
    console.table(gate.checks);
    if (gate.passed !== true) throw new Error(`adaptive release gate did not pass`);
    const routeId = `release-routing-check-${Date.now()}`;
    const routed = await invoke(client, `api_screen`, screenPayload(routeId, false));
    if (routed?.algorithm_version !== `adaptive_swing_v1`) {
      throw new Error(`default route did not switch to adaptive_swing_v1`);
    }
    console.log(`[adaptive-release] passed; default route uses adaptive_swing_v1`);
  } finally {
    client.close();
  }
};

main().catch((error) => {
  console.error(`[adaptive-release]`, error);
  process.exitCode = 1;
});
