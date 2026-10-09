import { openSync, fstatSync, readFileSync, closeSync } from 'node:fs';
import { pathToFileURL } from 'node:url';

const METRICS = ['coldstartup_ms', 'offlinep95_ms', 'peak_memory_bytes', 'background_requests'];
const METADATA = ['device_id', 'platform', 'fixture_id'];
function exactKeys(value, keys, label) {
  if (!value || typeof value !== 'object' || Array.isArray(value) || Object.keys(value).some(k => !keys.includes(k)) || keys.some(k => !Object.hasOwn(value, k))) {
    throw new Error(`invalid ${label}: unknown or missing fields`);
  }
}
function validateBaseline(value) {
  exactKeys(value, ['schema_version', ...METADATA, 'metrics'], 'baseline');
  if (value.schema_version !== 1) throw new Error('invalid baseline schema_version');
  for (const key of METADATA) {
    if (typeof value[key] !== 'string' || !/^[A-Za-z0-9][A-Za-z0-9._-]{0,95}$/.test(value[key])) throw new Error(`invalid metadata ${key}`);
  }
  exactKeys(value.metrics, METRICS, 'metrics');
  for (const key of METRICS) {
    const reading = value.metrics[key];
    if (typeof reading !== 'number' || !Number.isFinite(reading) || reading < 0 || reading > Number.MAX_SAFE_INTEGER || ((key === 'peak_memory_bytes' || key === 'background_requests') && !Number.isSafeInteger(reading))) {
      throw new Error(`invalid metric ${key}`);
    }
  }
}
function validateExemptions(value) {
  if (value === undefined) return new Map();
  exactKeys(value, ['schema_version', 'exemptions'], 'exemption');
  if (value.schema_version !== 1 || !Array.isArray(value.exemptions) || value.exemptions.length > METRICS.length) throw new Error('invalid exemption schema');
  const result = new Map();
  for (const item of value.exemptions) {
    exactKeys(item, ['metric', 'reason'], 'exemption');
    if (!METRICS.includes(item.metric) || result.has(item.metric) || typeof item.reason !== 'string' || item.reason.trim().length < 20 || item.reason.length > 1000 || !/[A-Za-z\u3400-\u9fff]/u.test(item.reason)) throw new Error('invalid exemption metric or reason');
    result.set(item.metric, item.reason.trim());
  }
  return result;
}

function overTenPercent(previous, current) {
  const decimal = value => {
    const [mantissa, exponent = '0'] = value.toString().toLowerCase().split('e');
    const [whole, fraction = ''] = mantissa.split('.');
    return { coefficient: BigInt(whole + fraction), exponent: Number(exponent) - fraction.length };
  };
  const p=decimal(previous), c=decimal(current);
  const scale=Math.min(p.exponent,c.exponent);
  return c.coefficient * 10n * 10n ** BigInt(c.exponent-scale) > p.coefficient * 11n * 10n ** BigInt(p.exponent-scale);
}

/** Evaluate readings, never generate them. Synthetic unit fixtures are not device measurements. */
export function compareBaselines(baseline, candidate, exemptionDocument) {
  validateBaseline(baseline); validateBaseline(candidate);
  for (const key of METADATA) if (baseline[key] !== candidate[key]) throw new Error(`metadata mismatch: ${key}`);
  const exemptions = validateExemptions(exemptionDocument);
  const regressions = [];
  for (const metric of METRICS) {
    const previous = baseline.metrics[metric], current = candidate.metrics[metric];
    // Compare canonical decimal readings exactly; never waive a real fractional regression.
    if (overTenPercent(previous, current)) {
      regressions.push({ metric, baseline: previous, candidate: current,
        regression_percent: previous === 0 ? null : ((current - previous) / previous) * 100,
        exempted: exemptions.has(metric), ...(exemptions.has(metric) ? { reason: exemptions.get(metric) } : {}) });
    }
  }
  for (const metric of exemptions.keys()) if (!regressions.some(r => r.metric === metric)) throw new Error('invalid exemption: no matching regression');
  return { schema_version: 1, status: regressions.some(r => !r.exempted) ? 'fail' : regressions.length ? 'exempted' : 'pass',
    device_id: baseline.device_id, platform: baseline.platform, fixture_id: baseline.fixture_id,
    threshold_percent: 10, regressions };
}
function loadJson(path) {
  const fd = openSync(path, 'r');
  try {
    const stat = fstatSync(fd);
    if (!stat.isFile() || stat.size > 65536) throw new Error('invalid input file size/type');
    return JSON.parse(readFileSync(fd, 'utf8').replace(/^\uFEFF/, ''));
  } finally { closeSync(fd); }
}
export function main(args) {
  try {
    const options = new Map();
    for (let i = 0; i < args.length; i += 2) {
      if (!['--baseline','--candidate','--exemptions'].includes(args[i]) || !args[i + 1] || args[i + 1].startsWith('--') || options.has(args[i])) throw new Error('invalid command options');
      options.set(args[i],args[i + 1]);
    }
    if (!options.has('--baseline') || !options.has('--candidate')) throw new Error('actual baseline and candidate readings required');
    const result=compareBaselines(loadJson(options.get('--baseline')), loadJson(options.get('--candidate')), options.has('--exemptions') ? loadJson(options.get('--exemptions')) : undefined);
    console.log(JSON.stringify(result,null,2));
    return result.status === 'fail' ? 1 : 0;
  } catch (error) {
    // Never echo user paths or raw JSON parse errors (which can include payload fragments).
    console.log(JSON.stringify({ status: 'requires_user', reason: error?.code === 'ENOENT' ? 'actual device baseline/candidate file missing' : 'invalid or incompatible baseline/candidate/exemption input; check schema and matching metadata' }));
    return 2;
  }
}
if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) process.exitCode = main(process.argv.slice(2));
