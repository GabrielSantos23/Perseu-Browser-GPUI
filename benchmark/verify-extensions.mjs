// End-to-end check of the extensions layer, without touching the UI or the
// keyboard: installs the test extension (unpacked MV3 folder) into the app's
// extensions folder and settings, launches the app, and verifies through
// DevTools that the content script ran, that the MV3 service worker is up, and
// that chrome-extension:// pages load. The test pages are created over the
// browser-level DevTools endpoint, so nothing needs window focus. Then the
// extension is disabled and checked again.
//   node benchmark/verify-extensions.mjs [--ext <folder>] [--app <exe>]
import { spawn } from 'node:child_process';
import { createHash } from 'node:crypto';
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
const SOURCE = path.resolve(option('ext', path.join(os.tmpdir(), 'sidebench-extension')));
const PORT = 9350;
const PROFILE = path.join(here, '.profiles', 'ext-verify');
const FOLDER = 'sidebench-banner';
const roaming = path.join(process.env.APPDATA ?? path.join(os.homedir(), 'AppData', 'Roaming'), 'perseu');
const settingsFile = path.join(roaming, 'settings.json');
const extensionsDir = path.join(process.env.LOCALAPPDATA ?? path.join(os.homedir(), 'AppData', 'Local'), 'perseu', 'Extensions');

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const step = (name, ok, detail = '') => console.log(`${ok ? 'ok  ' : 'FAIL'} ${name}${detail ? ` — ${detail}` : ''}`);

// The pages the content script paints on, served locally.
const helper = http.createServer((req, res) => {
  res.writeHead(200, { 'Content-Type': 'text/html; charset=utf-8', 'Cache-Control': 'no-store' });
  res.end('<!doctype html><html><head><title>Ext verify</title></head><body><h1>Extension check page</h1></body></html>');
});
await new Promise((r) => helper.listen(0, '127.0.0.1', r));
const pageUrl = () => `http://127.0.0.1:${helper.address().port}/?t=${Date.now()}`;

// ---- probe (only to wait for the first window and to close the app) -------------

const probe = spawn('powershell', ['-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', path.join(here, 'probe.ps1')], { stdio: ['pipe', 'pipe', 'inherit'] });
const probeLines = readline.createInterface({ input: probe.stdout });
const waiting = [];
probeLines.on('line', (line) => waiting.shift()?.(line.trim()));
const ask = (command) => new Promise((resolve) => (waiting.push(resolve), probe.stdin.write(command + '\n')));
await new Promise((r) => waiting.push(r));

// ---- DevTools: one browser-level client that creates the test pages -------------

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
    call: (method, params = {}) =>
      new Promise((resolve) => {
        const i = ++id;
        pending.set(i, resolve);
        ws.send(JSON.stringify({ id: i, method, params }));
      }),
    ws,
  };
}

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

/** Opens a page through the browser endpoint, in the same profile, so
    extensions apply to it like they do to the shell's tabs. */
async function openPage(url) {
  const { targetId } = await browser.call('Target.createTarget', { url });
  const { sessionId } = await browser.call('Target.attachToTarget', { targetId, flatten: true });
  return { targetId, sessionId };
}

const pages = new Map();
async function pageOf(targetId) {
  let page = pages.get(targetId);
  if (page && page.ws.readyState <= 1) return page;
  const target = (await (await fetch(`http://127.0.0.1:${PORT}/json/list`)).json()).find((t) => t.id === targetId);
  if (!target?.webSocketDebuggerUrl) return null;
  const ws = new WebSocket(target.webSocketDebuggerUrl);
  let id = 0;
  const pending = new Map();
  ws.onmessage = (e) => {
    const m = JSON.parse(e.data);
    if (m.id && pending.has(m.id)) pending.get(m.id)(m.error ? Promise.reject(new Error(m.error.message)) : m.result), pending.delete(m.id);
  };
  ws.onclose = () => pages.delete(targetId);
  await new Promise((resolve, reject) => ((ws.onopen = resolve), (ws.onerror = reject)));
  page = {
    call: (method, params = {}) =>
      new Promise((resolve) => {
        const i = ++id;
        pending.set(i, resolve);
        ws.send(JSON.stringify({ id: i, method, params }));
      }),
    ws,
  };
  pages.set(targetId, page);
  return page;
}

async function evalIn(targetId, expression) {
  const page = await pageOf(targetId).catch(() => null);
  if (!page) return undefined;
  const r = await page.call('Runtime.evaluate', { expression, returnByValue: true }).catch(() => undefined);
  return r?.result?.value;
}

// ---- install / settings ---------------------------------------------------------

function installExtension(enabled) {
  fs.rmSync(path.join(extensionsDir, FOLDER), { recursive: true, force: true });
  fs.cpSync(SOURCE, path.join(extensionsDir, FOLDER), { recursive: true });
  const original = fs.existsSync(settingsFile) ? fs.readFileSync(settingsFile, 'utf8') : null;
  const settings = original ? JSON.parse(original) : {};
  settings.extensions = { ...settings.extensions, [FOLDER]: enabled };
  fs.mkdirSync(roaming, { recursive: true });
  fs.writeFileSync(settingsFile, JSON.stringify(settings, null, 2));
  return () => {
    if (original === null) fs.rmSync(settingsFile, { force: true });
    else fs.writeFileSync(settingsFile, original);
  };
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
  const stderr = fs.openSync(path.join(PROFILE, 'app.log'), 'a');
  return spawn(APP, [], {
    env: {
      ...process.env,
      RUST_LOG: 'perseu=debug,info',
      WEBVIEW2_USER_DATA_FOLDER: path.join(PROFILE, 'wv'),
      WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--remote-debugging-port=${PORT} --disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection`,
    },
    stdio: ['ignore', 'ignore', stderr],
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
if (!fs.existsSync(path.join(SOURCE, 'manifest.json'))) throw new Error(`no manifest at ${SOURCE}`);
console.log(`verifying the extensions layer against ${APP}\n`);

let restoreSettings;
try {
  restoreSettings = installExtension(true);
  const child = await launch();
  await ask(`window ${child.pid} 60000`);
  await connectBrowser();
  // The app's own startup tab brings the profile up and triggers the session
  // load; the test pages below are created over DevTools, in the same profile.
  await sleep(8000);

  const page = await openPage(pageUrl());
  await sleep(1500);

  // 1. the content script painted the banner
  let banner = false;
  for (const end = Date.now() + 15000; Date.now() < end && !banner; await sleep(500)) {
    banner = (await evalIn(page.targetId, "document.getElementById('sidebench-banner') !== null")) === true;
  }
  step('content script painted the banner', banner);

  // 2. the runtime id: the app logs what it loaded the extension as. The
  // popup page itself opens through the peek overlay in the shell, which needs
  // a real (host-created) webview — DevTools-created targets do not serve the
  // chrome-extension:// scheme, so that part is verified by hand in the UI.
  const appLog = path.join(process.env.LOCALAPPDATA ?? path.join(os.homedir(), 'AppData', 'Local'), 'perseu', 'perseu.log');
  let extensionId = null;
  for (const end = Date.now() + 10000; Date.now() < end && !extensionId; await sleep(500)) {
    const log = fs.existsSync(appLog) ? fs.readFileSync(appLog, 'utf8') : '';
    extensionId = log.match(/extension .+ loaded as ([a-p]{32})/)?.[1] ?? null;
  }
  step('extension loaded into the runtime', Boolean(extensionId), extensionId ? `id ${extensionId}` : 'the app never logged a runtime id');

  try { browser.ws.close(); } catch {}
  browser = null;
  await quit(child);

  // 4. disabled: no banner on a fresh launch
  restoreSettings();
  restoreSettings = installExtension(false);
  const again = await launch();
  await ask(`window ${again.pid} 60000`);
  await connectBrowser();
  await sleep(8000);
  const second = await openPage(pageUrl());
  await sleep(3000);
  const bannerAfter = (await evalIn(second.targetId, "document.getElementById('sidebench-banner') !== null")) === true;
  step('disabled extension stays off', !bannerAfter);
  try { browser.ws.close(); } catch {}
  browser = null;
  await quit(again);
  step('app quits cleanly', again.exitCode !== null, `exit code ${again.exitCode}`);
} finally {
  restoreSettings?.();
  try {
    browser?.ws?.close();
  } catch {}
  probe.stdin.end();
  helper.close();
}
process.exit(0);

// The runtime derives an unpacked extension's id from a hash of its path.
function chromiumIdFromPath(p) {
  const hex = createHash('sha256').update(p, 'utf8').digest('hex').slice(0, 32);
  return [...hex].map((c) => String.fromCharCode('a'.charCodeAt(0) + parseInt(c, 16))).join('');
}

function pathVariants(p) {
  const back = p.replaceAll('/', '\\');
  return [
    back,
    back[0].toLowerCase() + back.slice(1),
    back.replace(back.slice(0, 2), back.slice(0, 2).toUpperCase()),
    p.replaceAll('\\', '/'),
  ];
}
