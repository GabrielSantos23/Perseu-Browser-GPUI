// Compares this suite's perseu numbers with the Perseu Chromium ones from the flecha-browser
// benchmark suite, medians against medians. The Chromium run drives its browser over DevTools;
// this one drives perseu through its own shortcuts (see run.mjs), so the launch, new-tab and
// tab-switch numbers carry a paste-and-Enter of keyboard flow that Chromium's do not. Idle, wake-ups
// and memory are measured the same way in both, by the probe, and compare directly.
//   node benchmark/compare.mjs <results.json> [--baseline <flecha results.json>]
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const here = path.dirname(fileURLToPath(import.meta.url));

const METRICS = [
  ['coldLaunchWindow', 'Cold launch → window on screen', 'ms', 'lower'],
  ['coldLaunchPainted', 'Cold launch → first page painted', 'ms', 'lower'],
  ['idleCpu1', 'Idle CPU, 1 tab', '%', 'lower'],
  ['idleCpu1Settled', 'Idle CPU, 1 tab, settled', '%', 'lower'],
  ['idleCpu20', 'Idle CPU, 20 tabs', '%', 'lower'],
  ['idleCpu20Settled', 'Idle CPU, 20 tabs, settled', '%', 'lower'],
  ['wakeups1', 'Idle wake-ups, 1 tab', '/s', 'lower'],
  ['wakeups1Settled', 'Idle wake-ups, 1 tab, settled', '/s', 'lower'],
  ['wakeups20', 'Idle wake-ups, 20 tabs', '/s', 'lower'],
  ['wakeups20Settled', 'Idle wake-ups, 20 tabs, settled', '/s', 'lower'],
  ['newWindow', 'New window → first paint', 'ms', 'lower'],
  ['newTab', 'New tab → first paint', 'ms', 'lower'],
  ['tabSwitch', 'Tab switch → shown', 'ms', 'lower'],
  ['memory1', 'Memory, 1 tab', 'MB', 'lower'],
  ['memory10', 'Memory, 10 tabs', 'MB', 'lower'],
  ['memory20', 'Memory, 20 tabs', 'MB', 'lower'],
  ['memoryBackTo1', 'Memory, back to 1 tab after 20', 'MB', 'lower'],
  ['speedometer', 'Speedometer 3.1 (higher is better)', 'points', 'higher'],
];

function newestBaseline() {
  const candidates = [
    path.resolve(here, '..', '..', 'flecha-browser', 'benchmarks', 'results'),
    path.join(process.env.LOCALAPPDATA ?? path.join(os.homedir(), 'AppData', 'Local'), 'flecha-browser', 'benchmarks', 'results'),
  ].filter((dir) => fs.existsSync(dir));
  for (const dir of candidates) {
    const files = fs
      .readdirSync(dir)
      .filter((f) => f.endsWith('.json'))
      .sort()
      .reverse();
    for (const file of files) {
      try {
        const data = JSON.parse(fs.readFileSync(path.join(dir, file), 'utf8'));
        if (data.results?.Perseu) return path.join(dir, file);
      } catch {}
    }
  }
  return undefined;
}

const fmt = (v, unit) => (Number.isFinite(v) ? (unit === '%' ? v.toFixed(2) : unit === '/s' ? v.toFixed(1) : Math.round(v).toString()) : '–');

/** The comparison table as Markdown, and the same rows as plain data. */
export function compareTable(gpui, perseu) {
  const rows = [];
  for (const [key, label, unit, direction] of METRICS) {
    const a = gpui[key];
    const b = perseu[key];
    if (!Number.isFinite(a) && !Number.isFinite(b)) continue;
    let ratio = '';
    let verdict = '';
    if (Number.isFinite(a) && Number.isFinite(b) && a > 0 && b > 0) {
      const factor = a / b;
      ratio = `${factor.toFixed(2)}×`;
      const better = direction === 'higher' ? factor >= 1 : factor <= 1;
      verdict = better ? `GPUI ${Math.max(factor, 1 / factor).toFixed(2)}×` : `Chromium ${Math.max(factor, 1 / factor).toFixed(2)}×`;
    }
    rows.push({ key, label, unit, gpui: fmt(a, unit), chromium: fmt(b, unit), ratio, verdict });
  }
  const lines = [
    `| Test | perseu (GPUI) | Perseu (Chromium) | GPUI / Chromium | Best |`,
    `|---|---:|---:|---:|---|`,
    ...rows.map((r) => `| ${r.label} (${r.unit}) | ${r.gpui} | ${r.chromium} | ${r.ratio} | ${r.verdict} |`),
  ];
  return { table: lines.join('\n'), rows };
}

/** Loads `resultPath` and the baseline, prints the comparison, and writes it beside the results. */
export async function compare(resultPath, baselinePath) {
  const gpuiData = JSON.parse(fs.readFileSync(resultPath, 'utf8'));
  const baseline = baselinePath ?? newestBaseline();
  if (!baseline) {
    console.log('No Perseu Chromium results found; pass --baseline <flecha results.json>.');
    return;
  }
  const baselineData = JSON.parse(fs.readFileSync(baseline, 'utf8'));
  const { table } = compareTable(gpuiData.results, baselineData.results.Perseu);
  const header = [
    `Perseu Chromium baseline: ${path.basename(baseline)} (${baselineData.date}, median of ${baselineData.runs} runs${baselineData.realSites ? ', real sites' : ', local pages'}).`,
    `perseu: median of ${gpuiData.runs} runs, local pages. Idle, wake-ups and memory compare directly;`,
    `launch, new-tab and tab-switch carry the keyboard flow (paste + Enter through the command bar) that`,
    `the Chromium suite does not have, so read those rows as "GPUI flow" against "Chromium API".`,
    `No new-window test: perseu has no second window. Lower is better, except Speedometer.`,
    ``,
  ].join('\n');
  console.log(`\n${header}${table}\n`);
  fs.writeFileSync(path.join(resultPath.replace(/\.json$/, '.comparison.md')), header + table + '\n');
}
