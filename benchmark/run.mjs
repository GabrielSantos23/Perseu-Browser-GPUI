// perseu (GPUI + WebView2) against the Perseu Chromium numbers, on this PC: launch, idle CPU
// and wake-ups, memory, new tab, tab switching, and optionally Speedometer 3.1. The Chromium suite
// (flecha-browser/benchmarks) drives its browser over DevTools; perseu has no remote control
// of its own, so this one drives the webviews over the DevTools port that the WebView2 runtime
// opens for `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS` (no code change), and drives the shell itself
// by typing its own shortcuts (ctrl+t, ctrl+w, ctrl+1...) through SendInput, with the URL on the
// clipboard so a chord pastes it. Nothing here was added to the browser: everything measured is the
// browser as it already works.
//
// Two differences worth knowing when reading the numbers:
//   - WebView2 starts its DevTools server only when its first webview is created, so "cold launch →
//     page painted" waits for that server; the FCP it reads is the page's historical first paint,
//     so the number still is paint time from process start.
//   - visibilitychange never fires for a webview hosted as a child window (WebView2 quirk), so the
//     tab switch is measured over DevTools screencast: a hidden webview paints nothing, so the
//     first frame that arrives after the chord is the tab's first shown frame.
//
// A new tab opens through the command bar the browser opens for ctrl+t, and that popup is part of
// the flow it measures, so the new-tab number sits a paste-and-Enter above the Chromium one. There
// is no second window at all, so that test is skipped. Every run is a cold start on a profile of
// its own; the result of each test is the median of its runs. See README.md.
//   node benchmark/run.mjs [--runs 3] [--speedometer] [--app <perseu.exe>]
import { spawn } from 'node:child_process';
import fs from 'node:fs';
import http from 'node:http';
import os from 'node:os';
import path from 'node:path';
import readline from 'node:readline';
import { fileURLToPath } from 'node:url';

const here = path.dirname(fileURLToPath(import.meta.url));
const args = process.argv.slice(2);
const flag = (name) => args.includes(`--${name}`);
const option = (name, fallback) => {
  const i = args.indexOf(`--${name}`);
  return i >= 0 ? args[i + 1] : fallback;
};
const RUNS = Number(option('runs', 3));
const APP = path.resolve(option('app', path.join(here, '..', 'target', 'release', 'perseu.exe')));
const PORT = 9344; // DevTools of the WebView2 runtime; the shell's shortcuts drive everything else
const APP_NAME = 'perseu';

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const median = (xs) => {
  const s = xs.filter((x) => Number.isFinite(x)).sort((a, b) => a - b);
  if (!s.length) return NaN;
  return s.length % 2 ? s[(s.length - 1) / 2] : (s[s.length / 2 - 1] + s[s.length / 2]) / 2;
};

// ---- the test pages: a static article, the same one the Chromium suite serves ------------------

const PAGE = (n) => `<!doctype html><html><head><meta charset="utf-8"><title>Bench ${n}</title>
<style>body{font:16px/1.6 system-ui;max-width:760px;margin:40px auto;padding:0 16px;color:#222}h1{font-size:32px}
.card{border:1px solid #ddd;border-radius:12px;padding:16px;margin:16px 0;background:linear-gradient(#fff,#f6f7fb)}</style></head>
<body><h1>Benchmark page ${n}</h1>${Array.from({ length: 30 }, (_, i) => `<div class="card"><h2>Section ${i + 1}</h2><p>${'Lorem ipsum dolor sit amet, consectetur adipiscing elit. '.repeat(6)}</p><svg width="120" height="40"><rect width="${40 + i * 2}" height="40" rx="8" fill="hsl(${i * 12} 70% 60%)"/></svg></div>`).join('')}
</body></html>`;

const server = http.createServer((req, res) => {
  const n = new URL(req.url, 'http://x').searchParams.get('n') ?? '0';
  res.writeHead(200, { 'Content-Type': 'text/html; charset=utf-8', 'Cache-Control': 'no-store' });
  res.end(PAGE(n));
});
await new Promise((r) => server.listen(0, '127.0.0.1', r));
const pageUrl = (n) => `http://127.0.0.1:${server.address().port}/?n=${n}`;

// ---- the probe (probe.ps1): first window, CPU, wake-ups, memory, typed chords, clipboard --------

const probe = spawn('powershell', ['-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', path.join(here, 'probe.ps1')], { stdio: ['pipe', 'pipe', 'inherit'] });
const probeLines = readline.createInterface({ input: probe.stdout });
const waiting = [];
probeLines.on('line', (line) => waiting.shift()?.(line.trim()));
const ask = (command) => new Promise((resolve) => (waiting.push(resolve), probe.stdin.write(command + '\n')));
if ((await new Promise((r) => waiting.push(r))) !== 'ready') throw new Error('probe did not start');
const key = async (combo) => {
  const answer = await ask(`key ${combo}`);
  if (answer !== 'ok') throw new Error(`key ${combo}: ${answer}`);
};
async function stats(pid) {
  const processes = new Map((await ask(`stats ${pid}`)).split(' ').filter(Boolean).map((row) => {
    const [id, cpuMs, switches, privateBytes] = row.split(':').map(Number);
    return [id, { cpuMs, switches, privateBytes }];
  }));
  const privateBytes = [...processes.values()].reduce((n, p) => n + p.privateBytes, 0);
  return { processes, privateBytes, at: Date.now() };
}

// ---- DevTools of the webviews (the shell is GPUI and never appears there) ----------------------

const targets = async () => (await (await fetch(`http://127.0.0.1:${PORT}/json/list`)).json()).filter((t) => t.type === 'page');

async function waitForCdp(timeoutMs = 30000) {
  const end = Date.now() + timeoutMs;
  while (Date.now() < end) {
    try {
      return await (await fetch(`http://127.0.0.1:${PORT}/json/version`)).json();
    } catch {
      await sleep(50);
    }
  }
  throw new Error(`no DevTools on port ${PORT}`);
}

async function waitForTarget(url, timeoutMs = 20000, exclude = new Set()) {
  for (const end = Date.now() + timeoutMs; Date.now() < end; await sleep(20)) {
    const found = (await targets().catch(() => [])).find((t) => !exclude.has(t.id) && t.url.includes(url));
    if (found) return found;
  }
  return null;
}

/** One DevTools session per webview, kept open while it exists. Method events land in `page.events`. */
const pages = new Map();
async function pageOf(targetId) {
  let page = pages.get(targetId);
  if (page && page.ws.readyState <= 1) return page;
  const target = (await targets().catch(() => [])).find((t) => t.id === targetId);
  if (!target?.webSocketDebuggerUrl) return null;
  const ws = new WebSocket(target.webSocketDebuggerUrl);
  let id = 0;
  const pending = new Map();
  page = {
    call: (method, params = {}) =>
      new Promise((resolve) => {
        const i = ++id;
        pending.set(i, resolve);
        ws.send(JSON.stringify({ id: i, method, params }));
      }),
    events: [],
    onEvent: null,
    ws,
  };
  ws.onmessage = (e) => {
    const m = JSON.parse(e.data);
    if (m.id && pending.has(m.id)) pending.get(m.id)(m.error ? Promise.reject(new Error(m.error.message)) : m.result), pending.delete(m.id);
    else if (m.method) {
      page.events.push({ at: Date.now(), method: m.method, params: m.params });
      if (page.onEvent) page.onEvent(m);
    }
  };
  ws.onclose = () => pages.delete(targetId);
  await new Promise((resolve, reject) => ((ws.onopen = resolve), (ws.onerror = reject)));
  pages.set(targetId, page);
  return page;
}

const FCP = `(() => { const e = performance.getEntriesByName('first-contentful-paint')[0]; return e ? performance.timeOrigin + e.startTime : null })()`;

/** Evaluates `expression` in one webview until it returns a number (or the time runs out). */
async function pollPage(targetId, expression, timeoutMs = 20000) {
  const end = Date.now() + timeoutMs;
  while (Date.now() < end) {
    const page = await pageOf(targetId).catch(() => null);
    if (!page) {
      await sleep(50);
      continue;
    }
    const r = await page.call('Runtime.evaluate', { expression, returnByValue: true }).catch(() => undefined);
    if (typeof r?.result?.value === 'number') return r.result.value;
    await sleep(20);
  }
  return NaN;
}

/**
 * The first painted frame of `targetId` after `chord` is typed, through DevTools screencast: a
 * hidden webview paints nothing, so the first frame that arrives after the chord is the tab's
 * first shown frame. The frame's own compositor timestamp is used when the browser reports one.
 */
async function firstFrameAfter(targetId, chord) {
  const page = await pageOf(targetId).catch(() => null);
  if (!page) return NaN;
  const frames = [];
  page.onEvent = (m) => {
    if (m.method !== 'Page.screencastFrame') return;
    const ts = m.params.metadata?.timestamp;
    const at = typeof ts === 'number' && ts > 1e9 && ts < 1e10 ? Math.round(ts * 1000) : Date.now();
    frames.push(at);
    page.call('Page.screencastFrameAck', { sessionId: m.params.sessionId }).catch(() => undefined);
  };
  try {
    await page.call('Page.startScreencast', { everyNthFrame: 1 });
    await sleep(300); // frames from before the switch are not part of it
    frames.length = 0;
    const sent = Date.now();
    await key(chord);
    for (const end = Date.now() + 5000; Date.now() < end; await sleep(5)) {
      const first = frames.find((at) => at >= sent);
      if (first !== undefined) return first - sent;
    }
    return NaN;
  } finally {
    await page.call('Page.stopScreencast').catch(() => undefined);
    page.onEvent = null;
  }
}

// ---- the browser, driven the way its own UI drives it ------------------------------------------

function launch(profile) {
  const env = {
    ...process.env,
    // No code change: the WebView2 runtime itself takes its data folder and switches from these.
    WEBVIEW2_USER_DATA_FOLDER: path.join(profile, 'wv'),
    WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--remote-debugging-port=${PORT} --disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection`,
  };
  return spawn(APP, [], { env, stdio: 'ignore' });
}

const profileDir = () => path.join(here, '.profiles', APP_NAME);
const runProfile = () => path.join(here, '.profiles', `${APP_NAME}-run`);

/**
 * Pastes `url` into the open command bar and presses Enter, waiting for the webview to appear.
 * The command bar is a popup the browser opens for its own shortcuts (ctrl+t at startup; one per
 * ctrl+t), so nothing has to be found: the paste lands in the input that is already focused. If a
 * paste was lost, a retry dismisses the bar and asks for it again, which is what a user does.
 */
async function commandBarOpen(url, fresh = true) {
  for (let attempt = 0; attempt < 6; attempt++) {
    if (fresh && attempt === 0) {
      await key('ctrl+t');
      await sleep(250);
    }
    await ask(`clip ${url}`);
    await key('ctrl+v');
    await sleep(80);
    await key('enter');
    // WebView2 starts its DevTools server when its first webview is created, which this paste just
    // asked for; waiting here first keeps a slow first webview from looking like a lost chord.
    await waitForCdp(30000).catch(() => undefined);
    const target = await waitForTarget(url, 5000);
    if (target) return target;
    // A chord may have landed on a page instead of the bar, or the bar closed early: ask again.
    await key('escape');
    await sleep(400);
    await key('ctrl+t');
    await sleep(250);
  }
  return null;
}

/** Idle CPU (% of one core) and wake-ups (context switches per second) over `ms`. */
async function idle(pid, ms) {
  const a = await stats(pid);
  await sleep(ms);
  const b = await stats(pid);
  const seconds = (b.at - a.at) / 1000;
  // Only processes there at both ends: one that exited would take its whole count with it.
  let cpuMs = 0;
  let switches = 0;
  for (const [id, end] of b.processes) {
    const start = a.processes.get(id);
    if (!start) continue;
    cpuMs += end.cpuMs - start.cpuMs;
    // A thread that ended takes its count with it; that process's change reads as none rather than negative.
    switches += Math.max(0, end.switches - start.switches);
  }
  return { cpu: (cpuMs / (seconds * 1000)) * 100, wakeups: switches / seconds, memoryMb: b.privateBytes / 2 ** 20 };
}

const tabCount = async () => (await targets().catch(() => [])).length;

/** Closes every tab but the first, through the browser's own ctrl+w, until only one webview is left. */
async function closeAllButFirst() {
  for (let presses = 0; (await tabCount()) > 1 && presses < 40; presses++) {
    await key('ctrl+w');
    await sleep(500);
  }
}

async function quit(child) {
  const exited = new Promise((r) => (child.exitCode !== null ? r() : child.once('exit', r)));
  await ask(`close ${child.pid}`);
  if (!(await Promise.race([exited.then(() => true), sleep(15000)]))) {
    spawn('taskkill', ['/PID', String(child.pid), '/T', '/F'], { stdio: 'ignore' });
  }
  await exited;
  await sleep(1500);
}

/** Once: a first launch, so that WebView2's first-run work (environment, profile folder, shader disk) is out of the way. */
async function prepare() {
  if (fs.existsSync(path.join(profileDir(), '.ready'))) return;
  fs.mkdirSync(profileDir(), { recursive: true });
  const child = launch(profileDir());
  // One page through the command bar, which is also what brings WebView2 and its DevTools up.
  const warm = await commandBarOpen(pageUrl('warmup'));
  if (warm) {
    await key('ctrl+w');
    await sleep(1000);
  }
  await sleep(5000);
  await quit(child);
  fs.writeFileSync(path.join(profileDir(), '.ready'), new Date().toISOString());
}

async function run() {
  fs.rmSync(runProfile(), { recursive: true, force: true });
  fs.cpSync(profileDir(), runProfile(), { recursive: true });
  const r = {};
  const t0 = Date.now();
  const child = launch(runProfile());
  const shown = Number(await ask(`window ${child.pid} 30000`));
  r.coldLaunchWindow = shown > 0 ? shown - t0 : NaN;

  // The browser opens its command bar on startup: the launch page goes in through it.
  const first = (await commandBarOpen(pageUrl('launch'), false)) ?? (await commandBarOpen(pageUrl('launch'), true));
  const painted = first ? await pollPage(first.id, FCP) : NaN;
  r.coldLaunchPainted = Number.isFinite(painted) ? painted - t0 : NaN;

  // One tab with the article, then settled (a minute in).
  await sleep(5000);
  const one = await idle(child.pid, 15000);
  r.idleCpu1 = one.cpu;
  r.wakeups1 = one.wakeups;
  r.memory1 = one.memoryMb;
  await sleep(Math.max(0, 60000 - (Date.now() - t0)));
  const settled = await idle(child.pid, 15000);
  r.idleCpu1Settled = settled.cpu;
  r.wakeups1Settled = settled.wakeups;

  // New tab, through ctrl+t and the command bar, to first paint; closed again, so the next one starts equal.
  const newTabs = [];
  for (let i = 0; i < RUNS; i++) {
    const sent = Date.now();
    const target = await commandBarOpen(pageUrl(`newtab${i}`));
    if (!target) {
      newTabs.push(NaN);
      continue;
    }
    const shownAt = await pollPage(target.id, FCP);
    newTabs.push(Number.isFinite(shownAt) ? shownAt - sent : NaN);
    await key('ctrl+w');
    await sleep(800);
  }
  r.newTab = median(newTabs);

  // Tab switch: from the first tab to a second one and back, to the first frame shown (ctrl+1 / ctrl+2).
  const second = await commandBarOpen(pageUrl('switch'));
  if (!second) throw new Error('the switch tab never opened');
  await sleep(1500);
  const switches = [];
  for (let i = 0; i < Math.max(5, RUNS); i++) {
    const to = i % 2 ? first : second;
    switches.push(await firstFrameAfter(to.id, to === first ? 'ctrl+1' : 'ctrl+2'));
    await sleep(600);
  }
  r.tabSwitch = median(switches);
  for (let i = 0; i < 3 && (await tabCount()) > 1; i++) {
    await key('ctrl+w');
    await sleep(800);
  }

  // 10 and 20 tabs: memory; at 20, idle CPU and wake-ups, then settled. Every tab opens in the
  // foreground here (the browser has no background tabs), so each page paints before the next opens.
  for (let i = 1; i < 20; i++) {
    const target = await commandBarOpen(pageUrl(`m${i}`));
    if (!target) throw new Error(`tab ${i} never opened`);
    if (i === 9) {
      await sleep(10000);
      r.memory10 = (await stats(child.pid)).privateBytes / 2 ** 20;
    }
  }
  await sleep(10000);
  const twenty = await idle(child.pid, 15000);
  r.idleCpu20 = twenty.cpu;
  r.wakeups20 = twenty.wakeups;
  r.memory20 = twenty.memoryMb;
  await sleep(45000);
  const twentySettled = await idle(child.pid, 15000);
  r.idleCpu20Settled = twentySettled.cpu;
  r.wakeups20Settled = twentySettled.wakeups;

  // Back to one tab, through ctrl+w until only the first is left.
  await closeAllButFirst();
  await sleep(10000);
  r.memoryBackTo1 = (await stats(child.pid)).privateBytes / 2 ** 20;

  if (flag('speedometer')) r.speedometer = await speedometer();
  await quit(child);
  return r;
}

async function speedometer() {
  const target = await commandBarOpen('https://browserbench.org/Speedometer3.1/?startAutomatically=true');
  if (!target) return NaN;
  // About five minutes; the score shows in #result-number when it is done.
  return pollPage(target.id, `(() => { const t = document.querySelector('#result-number')?.textContent; return t && parseFloat(t) ? parseFloat(t) : null })()`, 15 * 60000);
}

// ---- all of it ---------------------------------------------------------------------------------

const METRICS = [
  ['coldLaunchWindow', 'Cold launch → window on screen', 'ms'],
  ['coldLaunchPainted', 'Cold launch → first page painted', 'ms'],
  ['idleCpu1', 'Idle CPU, 1 tab', '%'],
  ['idleCpu1Settled', 'Idle CPU, 1 tab, settled', '%'],
  ['idleCpu20', 'Idle CPU, 20 tabs', '%'],
  ['idleCpu20Settled', 'Idle CPU, 20 tabs, settled', '%'],
  ['wakeups1', 'Idle wake-ups, 1 tab', '/s'],
  ['wakeups1Settled', 'Idle wake-ups, 1 tab, settled', '/s'],
  ['wakeups20', 'Idle wake-ups, 20 tabs', '/s'],
  ['wakeups20Settled', 'Idle wake-ups, 20 tabs, settled', '/s'],
  ['newTab', 'New tab → first paint', 'ms'],
  ['tabSwitch', 'Tab switch → shown', 'ms'],
  ['memory1', 'Memory, 1 tab', 'MB'],
  ['memory10', 'Memory, 10 tabs', 'MB'],
  ['memory20', 'Memory, 20 tabs', 'MB'],
  ['memoryBackTo1', 'Memory, back to 1 tab after 20', 'MB'],
  ['speedometer', 'Speedometer 3.1 (higher is better)', 'points'],
];

if (!fs.existsSync(APP)) throw new Error(`perseu isn't at ${APP} (pass --app <perseu.exe>)`);
console.log(`${APP_NAME}: preparing its profile…`);
await prepare();
const runs = [];
for (let i = 0; i < RUNS; i++) {
  console.log(`${APP_NAME}: run ${i + 1} of ${RUNS} (about ${flag('speedometer') ? 8 : 3} minutes)…`);
  runs.push(await run());
}
const results = Object.fromEntries(METRICS.map(([key]) => [key, median(runs.map((r) => r[key]))]));
results.runs = runs;

const fmt = (v, unit) => (Number.isFinite(v) ? (unit === '%' ? v.toFixed(2) : unit === '/s' ? v.toFixed(1) : Math.round(v).toString()) : '–');
const table = [`| Test | ${APP_NAME} (GPUI) |`, `|---|---:|`, ...METRICS.map(([key, label, unit]) => `| ${label} (${unit}) | ${fmt(results[key], unit)} |`)].join('\n');
console.log(`\nMedian of ${RUNS} runs, local pages; lower is better except Speedometer.\n\n${table}\n`);

fs.mkdirSync(path.join(here, 'results'), { recursive: true });
const stamp = new Date().toISOString().replace(/[:.]/g, '-');
const resultPath = path.join(here, 'results', `${stamp}.json`);
fs.writeFileSync(resultPath, JSON.stringify({ date: new Date().toISOString(), runs: RUNS, app: APP, results }, null, 2));
fs.writeFileSync(path.join(here, 'results', `${stamp}.md`), table + '\n');

// The comparison against the Perseu Chromium numbers, straight away.
const { compare } = await import('./compare.mjs');
await compare(resultPath, option('baseline', undefined));

probe.stdin.end();
server.close();
process.exit(0);
