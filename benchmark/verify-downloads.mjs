// Reproduces the download flows someone hits when fetching an extension, and
// reports how the app's download pipeline handles each one: a plain file, a
// same-origin redirect, the cross-origin redirect the Chrome Web Store's CRX
// endpoint uses, and file types SmartScreen looks at (.crx, .exe). Outcomes
// come from the app's own log lines — "download {id} started", "completed",
// "interrupted: <reason>" — which carry the engine's interrupt reason.
//   node benchmark/verify-downloads.mjs [--app <exe>]
import { spawn } from 'node:child_process';
import fs from 'node:fs';
import http from 'node:http';
import os from 'node:os';
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
// DevTools gets its own port; the helpers take the two after it.
const CDP_PORT = 9350;
const PORT = 9351;
const PORT2 = 9352;
const PROFILE = path.join(here, '.profiles', 'dl-verify');
// PERSEU_DATA_ROOT gives this run its own data folder and instance
// mutex, so it can run beside the user's real instance untouched.
const DATA_ROOT = path.join(here, '.profiles', 'dl-verify-data');
const settingsFile = path.join(DATA_ROOT, 'perseu', 'settings.json');
const downloadsDir = path.join(DATA_ROOT, 'downloads');
const logFile = path.join(DATA_ROOT, 'perseu', 'perseu.log');

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const step = (name, ok, detail = '') => console.log(`${ok ? 'ok  ' : 'FAIL'} ${name}${detail ? ` — ${detail}` : ''}`);

// Small fake payloads with the right magic numbers, so the file type is what
// the runtime judges it to be.
const ZIP = Buffer.concat([Buffer.from('PK\x03\x04'), Buffer.alloc(2048, 7)]);
const CRX = Buffer.concat([Buffer.from('Cr24'), Buffer.alloc(1024, 9)]);
const EXE = Buffer.concat([Buffer.from('MZ'), Buffer.alloc(1536, 5)]);

function attachment(res, name, body) {
  res.writeHead(200, {
    'Content-Type': 'application/octet-stream',
    'Content-Length': body.length,
    'Content-Disposition': `attachment; filename="${name}"`,
    'Cache-Control': 'no-store',
  });
  res.end(body);
}

// Two servers, because a cross-origin redirect has to hop between origins,
// like the Web Store's CRX endpoint does.
const servers = await Promise.all(
  [PORT, PORT2].map(
    (port) =>
      new Promise((resolve) => {
        const server = http.createServer((req, res) => {
          const url = new URL(req.url, `http://127.0.0.1:${port}`);
          if (url.pathname === '/start') {
            res.writeHead(200, { 'Content-Type': 'text/html; charset=utf-8' });
            return res.end('<!doctype html><title>dl start</title><h1>ready</h1>');
          }
          if (url.pathname === '/file/hello.zip') return attachment(res, 'hello.zip', ZIP);
          if (url.pathname === '/file/ext.crx') return attachment(res, 'ext.crx', CRX);
          if (url.pathname === '/file/app.exe') return attachment(res, 'app.exe', EXE);
          if (url.pathname === '/same/hello.zip') {
            res.writeHead(302, { Location: `http://127.0.0.1:${port}/file/hello.zip` });
            return res.end();
          }
          if (url.pathname === '/cross/hello.zip') {
            res.writeHead(302, { Location: `http://127.0.0.1:${PORT2}/file/hello.zip` });
            return res.end();
          }
          if (url.pathname === '/cross/ext.crx') {
            res.writeHead(302, { Location: `http://127.0.0.1:${PORT2}/file/ext.crx` });
            return res.end();
          }
          res.writeHead(404);
          res.end('gone');
        });
        server.listen(port, '127.0.0.1', () => resolve(server));
      }),
  ),
);

// ---- probe: only to wait for the first window and to close the app -------------

const probe = spawn('powershell', ['-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', path.join(here, 'probe.ps1')], { stdio: ['pipe', 'pipe', 'inherit'] });
const probeLines = readline.createInterface({ input: probe.stdout });
const waiting = [];
probeLines.on('line', (line) => waiting.shift()?.(line.trim()));
const ask = (command) => new Promise((resolve) => (waiting.push(resolve), probe.stdin.write(command + '\n')));
await new Promise((r) => waiting.push(r));

// ---- DevTools: browser-level client that steers the app's own tab --------------

let browser = null;
async function connectBrowser() {
  const version = await waitForCdp(30000);
  const ws = new WebSocket(version.webSocketDebuggerUrl);
  let id = 0;
  const pending = new Map();
  ws.onmessage = (e) => {
    const m = JSON.parse(e.data);
    if (m.id && pending.has(m.id)) pending.get(m.id)(m.error ? Promise.reject(new Error(m.error.message)) : m.result), pending.delete(m.id);
  };
  await new Promise((resolve, reject) => ((ws.onopen = resolve), (ws.onerror = reject)));
  browser = {
    call: (method, params = {}, sessionId) =>
      new Promise((resolve) => {
        const i = ++id;
        pending.set(i, resolve);
        ws.send(JSON.stringify({ id: i, method, params, ...(sessionId ? { sessionId } : {}) }));
      }),
    ws,
  };
}

async function waitForCdp(timeoutMs = 30000) {
  const end = Date.now() + timeoutMs;
  while (Date.now() < end) {
    try {
      return await (await fetch(`http://127.0.0.1:${CDP_PORT}/json/version`)).json();
    } catch {
      await sleep(50);
    }
  }
  throw new Error(`no DevTools on port ${CDP_PORT}`);
}

/** The app's startup tab: a page target hosted by the shell (which is the one
    holding our DownloadStarting handler). */
async function appTab() {
  for (let i = 0; i < 50; i++) {
    const targets = await (await fetch(`http://127.0.0.1:${CDP_PORT}/json/list`)).json();
    const tab = targets.find(
      (t) => t.type === 'page' && !t.url.startsWith('chrome-extension') && !t.url.startsWith('devtools'),
    );
    if (tab) return tab;
    await sleep(200);
  }
  throw new Error('no app tab found over DevTools');
}

// ---- the app's own download log lines ------------------------------------------

function logSlice(from) {
  if (!fs.existsSync(logFile)) return '';
  const fd = fs.openSync(logFile, 'r');
  const length = Math.max(0, fs.statSync(logFile).size - from);
  const buffer = Buffer.alloc(length);
  fs.readSync(fd, buffer, 0, length, from);
  fs.closeSync(fd);
  return buffer.toString('utf8');
}

const logSize = () => (fs.existsSync(logFile) ? fs.statSync(logFile).size : 0);

async function waitForDownloadEnd(from, timeoutMs = 12000) {
  const end = Date.now() + timeoutMs;
  while (Date.now() < end) {
    const finished = logSlice(from).match(/download (\d+) (completed|cancelled|interrupted: .+)/);
    if (finished) return finished[2];
    await sleep(300);
  }
  return /download \d+ started/.test(logSlice(from)) ? 'still in progress' : 'no download events';
}

// ---- settings / launch ----------------------------------------------------------

function installSettings() {
  fs.rmSync(downloadsDir, { recursive: true, force: true });
  fs.mkdirSync(downloadsDir, { recursive: true });
  fs.mkdirSync(path.dirname(settingsFile), { recursive: true });
  fs.writeFileSync(
    settingsFile,
    JSON.stringify({ downloads_folder: downloadsDir }, null, 2),
  );
  return () => {};
}

async function launch() {
  for (let i = 0; i < 15; i++) {
    try {
      fs.rmSync(path.join(PROFILE, 'wv'), { recursive: true, force: true });
      break;
    } catch {
      // WebView2 processes of the previous instance may still hold the folder.
      await sleep(1000);
    }
  }
  // No feature disables here: this run should behave like the user's daily
  // app, SmartScreen and all. Only the DevTools port and the data root are
  // added. The start URL argument gives the first tab a page to host, so a
  // webview (and its DevTools) comes up on a fresh profile.
  return spawn(APP, [`http://127.0.0.1:${PORT}/start`], {
    env: {
      ...process.env,
      RUST_LOG: 'perseu=debug,info',
      PERSEU_DATA_ROOT: DATA_ROOT,
      WEBVIEW2_USER_DATA_FOLDER: path.join(PROFILE, 'wv'),
      WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--remote-debugging-port=${CDP_PORT}`,
    },
    stdio: ['ignore', 'ignore', 'ignore'],
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

if (!fs.existsSync(APP)) throw new Error(`perseu isn't at ${APP}`);
console.log(`verifying downloads against ${APP}\n`);

let restoreSettings;
let child = null;
try {
  restoreSettings = installSettings();
  child = await launch();
  await ask(`window ${child.pid} 60000`);
  await connectBrowser();
  await sleep(4000);
  const tab = await appTab();
  const { sessionId } = await browser.call('Target.attachToTarget', { targetId: tab.id, flatten: true });
  await browser.call('Page.enable', {}, sessionId);

  const scenarios = [
    ['plain .zip', `${PORT}/file/hello.zip`],
    ['same-origin redirect', `${PORT}/same/hello.zip`],
    ['cross-origin redirect', `${PORT}/cross/hello.zip`],
    ['.crx direct', `${PORT}/file/ext.crx`],
    ['.crx via cross-origin redirect (Web Store shape)', `${PORT}/cross/ext.crx`],
    ['.exe (SmartScreen territory)', `${PORT}/file/app.exe`],
  ];
  const outcomes = new Map();
  for (const [name, suffix] of scenarios) {
    const before = logSize();
    await browser.call('Page.navigate', { url: `http://127.0.0.1:${suffix}` }, sessionId);
    const outcome = await waitForDownloadEnd(before);
    outcomes.set(name, outcome);
    step(name, outcome === 'completed', outcome);
  }

  const landed = fs.existsSync(downloadsDir) ? fs.readdirSync(downloadsDir) : [];
  step('files landed in the downloads folder', landed.length > 0, landed.join(', ') || 'nothing on disk');
  await quit(child);
  step('app quits cleanly', child.exitCode !== null, `exit code ${child.exitCode}`);
} finally {
  restoreSettings?.();
  try {
    browser?.ws?.close();
  } catch {}
  if (child && child.exitCode === null) child.kill();
  probe.stdin.end();
  for (const server of servers) server.close();
  fs.rmSync(downloadsDir, { recursive: true, force: true });
}
process.exit(0);
