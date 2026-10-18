/**
 * SoundCraft 界面语言冒烟测试：注入 window.__OTOOLS_LOCALE__，启动后截图。
 *
 * 上游 SoundCraft 没有界面语言偏好项（见 upstream/docs/localization-parity.md），
 * 「跟随 OTools 宿主语言」由 shell/src/i18n.rs 完成：读 index.html 里
 * window.soundcraftOtools.language() 暴露的宿主 locale，按 shell/i18n/zh-CN.tsv
 * 查表。这个探针用一个假 locale 驱动它，产出两张截图供肉眼核对：
 *
 *   dist-pack/lang-menubar.png    菜单栏 + 面板标题 + 传输栏
 *   dist-pack/lang-file-menu.png  展开的 File 菜单
 *
 * 用法：node scripts/probe-language.mjs [locale]     （默认 zh-CN）
 */
import fs from 'node:fs';
import http from 'node:http';
import path from 'node:path';
import process from 'node:process';
import { spawn } from 'node:child_process';

const repoRoot = path.resolve(path.dirname(new URL(import.meta.url).pathname.slice(1)), '..');
const distDir = path.join(repoRoot, 'dist');
const outDir = path.join(repoRoot, 'dist-pack');
fs.mkdirSync(outDir, { recursive: true });
const LOCALE = process.argv[2] ?? 'zh-CN';
const LIMIT = 120000;
const MIME = { '.html': 'text/html', '.js': 'text/javascript', '.wasm': 'application/wasm', '.svg': 'image/svg+xml' };

const server = http.createServer((req, res) => {
  const rel = decodeURIComponent(req.url.split('?')[0]).replace(/^\/+/, '') || 'index.html';
  const file = path.join(distDir, rel);
  if (!file.startsWith(path.resolve(distDir)) || !fs.existsSync(file) || fs.statSync(file).isDirectory()) { res.writeHead(404).end('nf'); return; }
  res.writeHead(200, { 'content-type': MIME[path.extname(file)] ?? 'application/octet-stream' });
  fs.createReadStream(file).pipe(res);
});
await new Promise((r) => server.listen(0, '127.0.0.1', r));
const url = `http://127.0.0.1:${server.address().port}/index.html`;

const CHROME = 'C:/Users/Admin/AppData/Local/ms-playwright/chromium_headless_shell-1243/chrome-headless-shell-win64/chrome-headless-shell.exe';
const PORT = 9911;
const profile = path.join(process.env.TEMP ?? '/tmp', 'otools-lang-soundcraft');
fs.rmSync(profile, { recursive: true, force: true });
const chrome = spawn(CHROME, ['--no-sandbox', '--disable-dev-shm-usage', '--use-angle=swiftshader', '--enable-unsafe-swiftshader',
  '--no-first-run', `--remote-debugging-port=${PORT}`, `--user-data-dir=${profile}`, '--window-size=1440,900', 'about:blank'], { stdio: ['ignore', 'ignore', 'pipe'] });
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const cleanup = () => { try { chrome.kill(); } catch {} server.close(); };
process.on('exit', cleanup);

async function waitDevtools() {
  for (let i = 0; i < 40; i += 1) {
    try { const r = await fetch(`http://127.0.0.1:${PORT}/json/version`, { signal: AbortSignal.timeout(2000) }); if (r.ok) return await r.json(); } catch { /* 还没起来 */ }
    await sleep(500);
  }
  return null;
}
async function newTarget(u) {
  for (const m of ['PUT', 'GET']) {
    try { const r = await fetch(`http://127.0.0.1:${PORT}/json/new?${encodeURIComponent(u)}`, { method: m, signal: AbortSignal.timeout(5000) }); if (r.ok) return await r.json(); } catch { /* 换下一种 */ }
  }
  return null;
}
// 宿主语言必须在任何页面脚本之前就位，否则 wasm 首帧读不到。
const LOCALE_SCRIPT = `window.__OTOOLS_LOCALE__ = { locale: ${JSON.stringify(LOCALE)} }; document.documentElement.classList.add('dark');`;

const version = await waitDevtools();
if (!version) { console.error('chromium devtools not up'); process.exit(1); }
const target = await newTarget(url);
const ws = new WebSocket(target.webSocketDebuggerUrl);
await new Promise((res, rej) => { ws.onopen = res; ws.onerror = () => rej(new Error('ws fail')); });
let id = 0;
const pending = new Map();
ws.onmessage = (e) => { const m = JSON.parse(String(e.data)); if (m.id && pending.has(m.id)) { pending.get(m.id)(m); pending.delete(m.id); } };
const send = (method, params = {}) => { id += 1; ws.send(JSON.stringify({ id, method, params })); return new Promise((r) => pending.set(id, r)); };
const evalJs = async (expression, awaitPromise = false) => {
  const r = await send('Runtime.evaluate', { expression, returnByValue: true, awaitPromise });
  if (r.result?.exceptionDetails) return `__ERR__${r.result.exceptionDetails.text}`;
  return r.result?.result?.value;
};
await send('Runtime.enable');
await send('Page.enable');
await send('Page.addScriptToEvaluateOnNewDocument', { source: LOCALE_SCRIPT });
await send('Page.navigate', { url });

const CHECK = `(() => { const l = document.getElementById('soundcraft_loading'); const c = document.getElementById('soundcraft_canvas');
  return JSON.stringify({ gone: l === null, canvas: c ? [c.width, c.height] : null,
    lang: window.soundcraftOtools ? window.soundcraftOtools.language() : null }); })()`;
let up = false;
const t0 = Date.now();
while (Date.now() - t0 < LIMIT) {
  const raw = await evalJs(CHECK);
  const st = raw ? JSON.parse(raw) : null;
  if (st && st.gone && st.canvas && st.canvas[0] > 0) { up = true; console.log(`启动成功 locale=${LOCALE}`, JSON.stringify(st)); break; }
  await sleep(1000);
}
if (!up) { console.error('did not start'); process.exit(1); }
await sleep(2500);
const shot = await send('Page.captureScreenshot', { format: 'png', captureBeyondViewport: false });
fs.writeFileSync(path.join(outDir, 'lang-menubar.png'), Buffer.from(shot.result.data, 'base64'));
// 点开菜单栏第一个根菜单（File），坐标按 1440x900 视口。
await send('Input.dispatchMouseEvent', { type: 'mousePressed', x: 70, y: 12, button: 'left', clickCount: 1 });
await send('Input.dispatchMouseEvent', { type: 'mouseReleased', x: 70, y: 12, button: 'left', clickCount: 1 });
await sleep(1200);
const shot2 = await send('Page.captureScreenshot', { format: 'png', captureBeyondViewport: false });
fs.writeFileSync(path.join(outDir, 'lang-file-menu.png'), Buffer.from(shot2.result.data, 'base64'));
console.log(`截图已写入 ${outDir}（lang-menubar.png / lang-file-menu.png）`);
cleanup();
process.exit(0);