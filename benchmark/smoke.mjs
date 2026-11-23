// One pass over the moving parts of run.mjs, without the timed runs: launch, command bar paste,
// DevTools of the webviews, a new tab, a tab switch, closing tabs, and the quit. For checking the
// automation works before spending a benchmark on it.
//   node benchmark/smoke.mjs [--app <perseu.exe>]
import { spawn } from 'node:child_process';
import fs from 'node:fs';
import http from 'node:http';
import path from 'node:path';
import readline from 'node:readline';
import { fileURLToPath } from 'node:url';

const here = path.dirname(fileURLToPath(import.meta.url));
const args = process.argv.slice(2);
const option = (name, fallback) => {
  const i = args.indexOf(`--${name}`);
  return i >= 0 ? args[i + 1] : fallback;
};
const APP = path.resolve(option('app', path.join(here, '..', 'target', 'release', 'perseu.exe')));
const PORT = 9345; // off the benchmark's port, so they never meet
const PROFILE = path.join(here, '.profiles', 'smoke');

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const step = (name, ok, detail = '') => console.log(`${ok ? 'ok  ' : 'FAIL'} ${name}${detail ? ` — ${detail}` : ''}`);

const PAGE = (n) => `<!doctype html><html><head><meta charset="utf-8"><title>Bench ${n}</title>
<style>body{font:16px/1.6 system-ui;max-width:760px;margin:40px auto;padding:0 16px}</style></head>
<body><h1>Benchmark page ${n}</h1><p>${'Lorem ipsum dolor sit amet. '.repeat(40)}</p></body></html>`;


const server = http.createServer((req, res) => {
  const n = new URL(req.url, 'http://x').searchParams.get('n') ?? '0';
  res.writeHead(200, { 'Content-Type': 'text/html; charset=utf-8', 'Cache-Control': 'no-store' });
  res.end(PAGE(n));
});
await new Promise((r) => server.listen(0, '127.0.0.1', r));
const pageUrl = (n) => `http://127.0.0.1:${server.address().port}/?n=${n}`;

// ---- probe -------------------------------------------------------------------------------------

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

// ---- DevTools ----------------------------------------------------------------------------------

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

async function waitForTarget(url, timeoutMs = 20000) {
  for (const end = Date.now() + timeoutMs; Date.now() < end; await sleep(20)) {
    const found = (await targets().catch(() => [])).find((t) => t.url.includes(url));
    if (found) return found;
  }
  return null;
}

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

/**
 * The first painted frame of `targetId` after `chord` is typed, through DevTools screencast: a
 * hidden webview paints nothing, so the first frame that arrives after the chord is the tab's
 * first shown frame. (visibilitychange never fires for a child-hosted WebView2.)
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

const FCP = `(() => { const e = performance.getEntriesByName('first-contentful-paint')[0]; return e ? performance.timeOrigin + e.startTime : null })()`;

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

// ---- the flow ----------------------------------------------------------------------------------

function launch() {
  fs.rmSync(PROFILE, { recursive: true, force: true });
  fs.mkdirSync(PROFILE, { recursive: true });
  return spawn(APP, [], {
    env: {
      ...process.env,
      WEBVIEW2_USER_DATA_FOLDER: path.join(PROFILE, 'wv'),
      WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--remote-debugging-port=${PORT} --disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection`,
    },
    stdio: 'ignore',
  });
}

const quit = async (child) => {
  const exited = new Promise((r) => (child.exitCode !== null ? r() : child.once('exit', r)));
  await ask(`close ${child.pid}`);
  if (!(await Promise.race([exited.then(() => true), sleep(15000)]))) {
    spawn('taskkill', ['/PID', String(child.pid), '/T', '/F'], { stdio: 'ignore' });
  }
  await exited;
};

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
    await key('escape');
    await sleep(400);
    await key('ctrl+t');
    await sleep(250);
  }
  return null;
}

const tabCount = async () => (await targets().catch(() => [])).length;

if (!fs.existsSync(APP)) throw new Error(`perseu isn't at ${APP} (pass --app <perseu.exe>)`);
console.log(`smoke test against ${APP}\n`);
const t0 = Date.now();
const child = launch();
const shown = Number(await ask(`window ${child.pid} 30000`));
step('cold launch → window', shown > 0, `${shown > 0 ? shown - t0 : 'timeout'} ms`);

// The launch page goes through the startup command bar; its FCP is historical, so reading it after
// the fact is still the page's true first paint.
const first = await commandBarOpen(pageUrl('launch'), false);
if (!first) {
  step('command bar paste (startup bar)', false, 'no webview appeared');
  await quit(child);
  probe.stdin.end();
  server.close();
  process.exit(1);
}
await waitForCdp(5000).then((v) => step('WebView2 DevTools up', true, v.Browser), (e) => step('WebView2 DevTools up', false, e.message));
const painted = await pollPage(first.id, FCP);
step('launch page painted (FCP)', Number.isFinite(painted), `${Math.round(painted - t0)} ms from process start`);

const sent = Date.now();
const second = await commandBarOpen(pageUrl('switch'));
step('new tab via ctrl+t + paste', Boolean(second), second ? `${Math.round((await pollPage(second.id, FCP)) - sent)} ms to FCP` : 'no webview');
if (!second) {
  await quit(child);
  probe.stdin.end();
  server.close();
  process.exit(1);
}
await sleep(1500);

const switch1 = await firstFrameAfter(first.id, 'ctrl+1');
step('tab switch → shown (ctrl+1)', Number.isFinite(switch1), `${Math.round(switch1)} ms`);
const switch2 = await firstFrameAfter(second.id, 'ctrl+2');
step('tab switch → shown (ctrl+2)', Number.isFinite(switch2), `${Math.round(switch2)} ms`);

for (let i = 0; i < 3 && (await tabCount()) > 1; i++) {
  await key('ctrl+w');
  await sleep(800);
}
step('ctrl+w closes down to one tab', (await tabCount()) === 1, `${await tabCount()} webview(s) left`);

await quit(child);
step('probe close → app quit', child.exitCode === 0 || child.exitCode !== null, `exit code ${child.exitCode}`);
console.log(`\nwebview folder used: ${path.join(PROFILE, 'wv')}`);
probe.stdin.end();
server.close();
process.exit(0);
