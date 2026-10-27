#!/usr/bin/env node
// Matriz de aceitação do aplicativo desktop real (Tauri + WebKitGTK).
//
// Diferente de `ui-acceptance.mjs`, este script abre o binário de release em uma
// WebView real via tauri-driver/WebKitWebDriver, usa o IPC do produto e grava
// capturas da janela. Ele é ferramenta de validação, não parte do aplicativo.
//
// Pré-requisitos no Linux:
//   - binário `target/release/reqlivo-desktop`
//   - `tauri-driver` 2.x no PATH
//   - `WebKitWebDriver` (pacote webkitgtk-webdriver) ou WEBKIT_WEBDRIVER
//   - Xvfb, xwininfo, dbus-run-session e Python 3 com libX11/libXtst
//
// Uso:
//   WEBKIT_WEBDRIVER=/caminho/WebKitWebDriver node scripts/desktop-acceptance.mjs
//
// As coleções são cópias descartáveis de `examples/demo`. Nenhum dado real é
// usado. O script inicia e encerra o driver; não usa o servidor HTTP do produto.

import { spawn, spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { cp, mkdir, mkdtemp, readFile, rm, writeFile } from 'node:fs/promises';
import http from 'node:http';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const REPO = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const APP = process.env.DESKTOP_APP || path.join(REPO, 'target/release/reqlivo-desktop');
const DEMO = path.join(REPO, 'examples/demo');
const EVIDENCE =
  process.env.DESKTOP_EVIDENCE ||
  path.resolve(REPO, '../evidencias/reqlivo-v0.3.0/desktop');
const DISPLAY = process.env.DESKTOP_DISPLAY || ':99';
const DRIVER_PORT = Number(process.env.DESKTOP_DRIVER_PORT || 4444);
const NATIVE_PORT = Number(process.env.DESKTOP_NATIVE_PORT || 4445);
const DRIVER_URL = `http://127.0.0.1:${DRIVER_PORT}`;
const DRIVER_LOG = path.join(tmpdir(), 'reqlivo-desktop-driver.log');
const XCLICK = path.join(REPO, 'scripts/x11-dialog-ok.py');
const WEBKIT_VERSION = process.env.DESKTOP_WEBKIT_VERSION || 'versão não informada';

const ECHO = 'req_4a2b3c4d5e6f7081';
const STATUS = 'req_5b3c4d5e6f708192';
const BASICOS = 'fld_3f2a1b4c5d6e7f80';

const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

function assert(condition, message) {
  if (!condition) throw new Error(message);
}

function assertEqual(actual, expected, message) {
  if (actual !== expected) {
    throw new Error(
      `${message}: esperado ${JSON.stringify(expected)}, obtido ${JSON.stringify(actual)}`,
    );
  }
}

function assertIncludes(value, expected, message) {
  if (!String(value ?? '').includes(expected)) {
    throw new Error(`${message}: esperado conter ${JSON.stringify(expected)}, obtido ${JSON.stringify(value)}`);
  }
}

function outputOf(command, args = []) {
  const result = spawnSync(command, args, { encoding: 'utf8' });
  return result.status === 0 ? result.stdout.trim() : '';
}

function shellQuote(value) {
  return `'${String(value).replaceAll("'", "'\\''")}'`;
}

// ---------------------------------------------------------------------------
// Driver, display e servidor sintético
// ---------------------------------------------------------------------------

async function driverRequest(method, route, body) {
  const init = { method, headers: {} };
  if (body !== undefined) {
    init.headers['content-type'] = 'application/json';
    init.body = JSON.stringify(body);
  }
  const response = await fetch(`${DRIVER_URL}${route}`, init);
  const text = await response.text();
  let data = null;
  if (text) {
    try {
      data = JSON.parse(text);
    } catch (_) {
      data = text;
    }
  }
  if (!response.ok) {
    throw new Error(`${method} ${route}: ${response.status} ${text}`);
  }
  if (data && data.value && data.value.error) {
    throw new Error(`${method} ${route}: ${data.value.error}: ${data.value.message || ''}`);
  }
  return data;
}

async function driverStatus() {
  try {
    const data = await driverRequest('GET', '/status');
    return data.value;
  } catch (_) {
    return null;
  }
}

async function waitDriverReady(timeout = 20000) {
  const deadline = Date.now() + timeout;
  while (Date.now() < deadline) {
    const status = await driverStatus();
    if (status?.ready) return;
    await sleep(150);
  }
  throw new Error('tauri-driver não ficou pronto');
}

async function waitDriverNoSession(timeout = 15000) {
  const deadline = Date.now() + timeout;
  while (Date.now() < deadline) {
    const status = await driverStatus();
    if (status?.ready) return;
    await sleep(150);
  }
  throw new Error('a sessão WebDriver anterior não foi encerrada');
}

function findProcesses(names) {
  const result = spawnSync('ps', ['-eo', 'pid=,comm=,args='], { encoding: 'utf8' });
  if (result.status !== 0) return [];
  return result.stdout
    .split('\n')
    .map((line) => line.trim())
    .filter(Boolean)
    .map((line) => {
      const match = line.match(/^(\d+)\s+(\S+)\s+(.*)$/);
      return match ? { pid: Number(match[1]), comm: match[2], args: match[3] } : null;
    })
    .filter(Boolean)
    .filter((process) => names.includes(process.comm));
}

function killProcesses(names) {
  for (const item of findProcesses(names)) {
    try {
      globalThis.process.kill(item.pid, 'SIGTERM');
    } catch (_) {
      // processo já encerrado
    }
  }
}

function killDriverProcesses() {
  killProcesses(['tauri-driver', 'WebKitWebDriver', 'dbus-run-sessio']);
}

function killAppProcesses() {
  const result = spawnSync('ps', ['-eo', 'pid=,args='], { encoding: 'utf8' });
  if (result.status !== 0) return;
  for (const line of result.stdout.split('\n')) {
    if (!line.includes(APP)) continue;
    const match = line.trim().match(/^(\d+)\s+/);
    if (!match) continue;
    try {
      globalThis.process.kill(Number(match[1]), 'SIGTERM');
    } catch (_) {
      // processo já encerrado
    }
  }
}

async function ensureDisplay() {
  const check = spawnSync('xwininfo', ['-root', '-display', DISPLAY], { stdio: 'ignore' });
  if (check.status === 0) return null;

  const xvfb = spawn('Xvfb', [DISPLAY, '-screen', '0', '1600x1000x24', '-ac'], {
    stdio: 'ignore',
  });
  const deadline = Date.now() + 8000;
  while (Date.now() < deadline) {
    const ready = spawnSync('xwininfo', ['-root', '-display', DISPLAY], { stdio: 'ignore' });
    if (ready.status === 0) return xvfb;
    await sleep(150);
  }
  xvfb.kill('SIGTERM');
  throw new Error(`não foi possível iniciar o Xvfb em ${DISPLAY}`);
}

function startSyntheticServer() {
  const server = http.createServer((request, response) => {
    const send = (status, payload) => {
      if (response.writableEnded) return;
      const body = JSON.stringify(payload);
      response.writeHead(status, {
        'content-type': 'application/json; charset=utf-8',
        'content-length': Buffer.byteLength(body),
      });
      response.end(body);
    };
    if (request.url.startsWith('/delay')) {
      const timer = setTimeout(() => send(200, { delayed: true }), 5000);
      response.on('close', () => clearTimeout(timer));
      return;
    }
    if (request.url.startsWith('/status/404')) {
      send(404, { error: 'not_found', synthetic: true });
      return;
    }
    if (request.url.startsWith('/echo')) {
      send(200, { ok: true, path: request.url, synthetic: true });
      return;
    }
    send(200, { ok: true, path: request.url, synthetic: true });
  });
  return new Promise((resolve, reject) => {
    server.once('error', reject);
    server.listen(0, '127.0.0.1', () => {
      resolve({ server, port: server.address().port });
    });
  });
}

async function startDriver(webkitDriver) {
  killDriverProcesses();
  killAppProcesses();
  await sleep(300);
  const tauriDriver = process.env.TAURI_DRIVER || 'tauri-driver';
  const command = [
    `export DISPLAY=${shellQuote(DISPLAY)}`,
    'export GDK_BACKEND=x11',
    'export WEBKIT_DISABLE_COMPOSITING_MODE=1',
    'export LIBGL_ALWAYS_SOFTWARE=1',
    'export NO_AT_BRIDGE=1',
    'export GTK_USE_PORTAL=0',
    `exec ${shellQuote(tauriDriver)} --native-driver ${shellQuote(webkitDriver)} --port ${DRIVER_PORT} --native-port ${NATIVE_PORT}`,
  ].join('; ');
  const log = await import('node:fs').then(({ openSync }) => openSync(DRIVER_LOG, 'w'));
  const hasDbus = spawnSync('dbus-run-session', ['--version'], { stdio: 'ignore' }).status === 0;
  const child = hasDbus
    ? spawn('dbus-run-session', ['--', 'bash', '-c', command], {
        stdio: ['ignore', log, log],
      })
    : spawn('bash', ['-c', command], { stdio: ['ignore', log, log] });
  const deadline = Date.now() + 20000;
  while (Date.now() < deadline) {
    const status = await driverStatus();
    if (status?.ready) return child;
    await sleep(150);
  }
  child.kill('SIGTERM');
  throw new Error(`tauri-driver não abriu a porta ${DRIVER_PORT}; veja ${DRIVER_LOG}`);
}

function resolveWebKitDriver() {
  if (process.env.WEBKIT_WEBDRIVER) return process.env.WEBKIT_WEBDRIVER;
  const which = spawnSync('which', ['WebKitWebDriver'], { encoding: 'utf8' });
  if (which.status === 0) return which.stdout.trim();
  throw new Error(
    'WebKitWebDriver não encontrado; instale webkitgtk-webdriver ou defina WEBKIT_WEBDRIVER',
  );
}

// ---------------------------------------------------------------------------
// Cliente WebDriver e utilitários da WebView
// ---------------------------------------------------------------------------

function makePage(sessionId) {
  const base = `/session/${sessionId}`;
  async function command(method, route, body) {
    return driverRequest(method, `${base}${route}`, body);
  }
  async function exec(script, args = []) {
    const data = await command('POST', '/execute/sync', { script, args });
    return data.value;
  }
  async function execAsync(script, args = []) {
    const data = await command('POST', '/execute/async', { script, args });
    return data.value;
  }
  async function wait(script, timeout = 12000, label = script) {
    const deadline = Date.now() + timeout;
    let last = null;
    while (Date.now() < deadline) {
      try {
        if (await exec(script)) return;
      } catch (error) {
        last = error;
      }
      await sleep(120);
    }
    throw new Error(
      `tempo esgotado aguardando: ${label}${last ? ` (${last.message})` : ''}`,
    );
  }
  const selector = (value) => JSON.stringify(value);
  return {
    sessionId,
    exec,
    execAsync,
    wait,
    async click(css) {
      const result = await exec(
        `return (() => { const element = document.querySelector(${selector(css)}); if (!element) return false; element.click(); return true; })()`,
      );
      assert(result, `elemento ausente: ${css}`);
    },
    async clickText(css, text) {
      const result = await exec(
        `return (() => { const element = [...document.querySelectorAll(${selector(css)})].find((item) => (item.textContent || '').trim() === ${selector(text)}); if (!element) return false; element.click(); return true; })()`,
      );
      assert(result, `elemento com texto ${JSON.stringify(text)} ausente em ${css}`);
    },
    async setInput(css, value) {
      const result = await exec(
        `return (() => { const element = document.querySelector(${selector(css)}); if (!element) return false; element.focus(); element.value = ${selector(value)}; element.dispatchEvent(new Event('input', { bubbles: true })); return true; })()`,
      );
      assert(result, `campo ausente: ${css}`);
    },
    async setSelect(css, value) {
      const result = await exec(
        `return (() => { const element = document.querySelector(${selector(css)}); if (!element) return false; element.value = ${selector(value)}; element.dispatchEvent(new Event('change', { bubbles: true })); return true; })()`,
      );
      assert(result, `select ausente: ${css}`);
    },
    text(css) {
      return exec(`return document.querySelector(${selector(css)})?.textContent ?? ''`);
    },
    value(css) {
      return exec(`return document.querySelector(${selector(css)})?.value ?? ''`);
    },
    visible(css) {
      return exec(
        `return (() => { const element = document.querySelector(${selector(css)}); return !!element && !element.hidden && (element.open === undefined || element.open); })()`,
      );
    },
    attr(css, name) {
      return exec(
        `return document.querySelector(${selector(css)})?.getAttribute(${selector(name)}) ?? null`,
      );
    },
    async ipc(route, method = 'GET', body = null) {
      const raw = await execAsync(
        `const done = arguments[arguments.length - 1]; const args = arguments[0]; window.__TAURI__.core.invoke('api_request', args).then((result) => done(JSON.stringify(result))).catch((error) => done(JSON.stringify({ ipcError: String(error) })));`,
        [{ path: route, method, body }],
      );
      const parsed = JSON.parse(raw);
      return parsed.ipcError ? parsed : (parsed.data ?? parsed);
    },
    async host(command, args = {}) {
      const raw = await execAsync(
        `const done = arguments[arguments.length - 1]; const args = arguments[0]; window.__TAURI__.core.invoke(${selector(command)}, args).then((result) => done(JSON.stringify(result))).catch((error) => done(JSON.stringify({ hostError: String(error) })));`,
        [args],
      );
      return JSON.parse(raw);
    },
    async key(value) {
      await driverRequest('POST', `${base}/actions`, {
        actions: [
          {
            type: 'key',
            id: 'keyboard',
            actions: [
              { type: 'keyDown', value },
              { type: 'keyUp', value },
            ],
          },
        ],
      });
    },
    async screenshot(file) {
      const data = await command('GET', '/screenshot');
      await writeFile(file, Buffer.from(data.value, 'base64'));
    },
    async waitApp() {
      await wait(
        "return document.querySelector('#collectionName')?.textContent === 'Reqlivo Demo'",
        30000,
        'nome da coleção',
      );
      await wait(
        "return document.querySelectorAll('#tree [data-tree-main]').length >= 3",
        12000,
        'árvore da coleção',
      );
    },
    async selectRequest(id) {
      const ok = await exec(
        `return (() => { const element = document.querySelector('#tree [data-tree-main][data-id=${JSON.stringify(id)}]'); if (!element) return false; element.click(); return true; })()`,
      );
      assert(ok, `requisição ausente na árvore: ${id}`);
    },
    async currentUrl() {
      return this.value('#url');
    },
    async waitCurrentUrl(expected, timeout = 10000) {
      await wait(
        `return document.querySelector('#url')?.value === ${selector(expected)}`,
        timeout,
        `URL ${expected}`,
      );
    },
  };
}

async function createSession(root) {
  const data = await driverRequest('POST', '/session', {
    capabilities: {
      alwaysMatch: {
        'tauri:options': {
          application: APP,
          args: ['--collection', root],
        },
      },
    },
  });
  const sessionId = data.value.sessionId;
  await sleep(500);
  return sessionId;
}

async function deleteSession(sessionId) {
  await driverRequest('DELETE', `/session/${sessionId}`).catch(() => {});
  await waitDriverNoSession().catch(() => {});
}

// ---------------------------------------------------------------------------
// Diálogos nativos GTK
// ---------------------------------------------------------------------------

function nativeDialogGeometry() {
  const result = spawnSync('xwininfo', ['-root', '-tree', '-display', DISPLAY], {
    encoding: 'utf8',
  });
  if (result.status !== 0) return null;
  for (const line of result.stdout.split('\n')) {
    if (!line.includes('"Reqlivo"')) continue;
    const match = line.match(/(\d+)x(\d+)([+-]\d+)([+-]\d+)/);
    if (!match) continue;
    const width = Number(match[1]);
    const height = Number(match[2]);
    if (width > 0 && width < 800 && height > 0 && height < 400) {
      return { width, height, x: Number(match[3]), y: Number(match[4]) };
    }
  }
  return null;
}

async function confirmNative(page, timeout = 12000) {
  const deadline = Date.now() + timeout;
  let geometry = null;
  while (Date.now() < deadline) {
    geometry = nativeDialogGeometry();
    if (geometry) break;
    await sleep(100);
  }
  if (!geometry) {
    const tree = spawnSync('xwininfo', ['-root', '-tree', '-display', DISPLAY], {
      encoding: 'utf8',
    }).stdout;
    throw new Error(`diálogo nativo de confirmação não apareceu\n${tree}`);
  }
  await sleep(250);
  const x = Math.max(20, geometry.x + geometry.width - 70);
  const y = Math.max(20, geometry.y + geometry.height - 24);
  const result = spawnSync(
    'python3',
    [XCLICK, '--display', DISPLAY, '--x', String(x), '--y', String(y)],
    { encoding: 'utf8', env: { ...process.env, DISPLAY } },
  );
  if (result.status !== 0) {
    throw new Error(`falha ao confirmar diálogo nativo: ${result.stderr.trim()}`);
  }
  const gone = Date.now() + timeout;
  while (Date.now() < gone) {
    if (!nativeDialogGeometry()) {
      await sleep(300);
      return;
    }
    await sleep(100);
  }
  throw new Error('diálogo nativo não fechou após a confirmação');
}

// ---------------------------------------------------------------------------
// Coleções de teste
// ---------------------------------------------------------------------------

async function prepareCollection(port) {
  const root = await mkdtemp(path.join(tmpdir(), 'reqlivo-desktop-'));
  await cp(DEMO, root, {
    recursive: true,
    filter: (source) => {
      const relative = path.relative(DEMO, source);
      return !relative.split(path.sep).includes('.reqlivo');
    },
  });
  const manifest = path.join(root, 'opencollection.yml');
  const text = await readFile(manifest, 'utf8');
  await writeFile(manifest, text.replaceAll('http://127.0.0.1:18080', `http://127.0.0.1:${port}`));
  return root;
}

async function prepareMigrationCollection(port) {
  const root = await prepareCollection(port);
  const manifest = path.join(root, 'opencollection.yml');
  const text = await readFile(manifest, 'utf8');
  const older = text.replace('schemaVersion: 1', 'schemaVersion: 0');
  assert(older !== text, 'não foi possível preparar schemaVersion 0');
  await writeFile(manifest, older);
  return root;
}

function requestFile(root, name = 'basicos/echo.yml') {
  return path.join(root, name);
}

async function readText(file) {
  return readFile(file, 'utf8');
}

// ---------------------------------------------------------------------------
// Casos executados na WebView real
// ---------------------------------------------------------------------------

async function caseD01(env) {
  const { page } = env;
  await page.waitApp();
  await page.selectRequest(ECHO);
  await page.wait("return document.querySelector('#url')?.value.includes('/echo')");
  await page.click('#sendBtn');
  await page.wait(
    "return document.querySelector('#responseStatus')?.textContent.includes('200')",
    20000,
    'status HTTP 200',
  );
  assertIncludes(await page.text('#responseBody'), 'synthetic', 'corpo da resposta 200');
  assertIncludes(await page.text('#responseMeta'), 'bytes', 'metadados da resposta');

  await page.selectRequest(STATUS);
  await page.wait("return document.querySelector('#url')?.value.includes('/status/404')");
  await page.click('#sendBtn');
  await page.wait(
    "return document.querySelector('#responseStatus')?.textContent.includes('404')",
    20000,
    'status HTTP 404',
  );
  assertIncludes(await page.text('#responseBody'), 'not_found', 'corpo da resposta 404');
  await env.screenshot('D-01-envio-200-404');
}

async function caseD02(env) {
  const { page, root } = env;
  await page.waitApp();
  await page.selectRequest(ECHO);
  await page.wait("return document.querySelector('#url')?.value.includes('/echo')");
  await page.setInput('#url', '{{baseUrl}}/rascunho-real');
  await page.wait(
    "return document.querySelector('#saveState')?.textContent.includes('rascunho protegido')",
    10000,
    'rascunho protegido',
  );
  const canonicalBefore = await readText(requestFile(root));
  assert(!canonicalBefore.includes('rascunho-real'), 'YAML canônico não pode receber a edição');

  await env.restart();
  const restarted = env.page;
  await restarted.waitApp();
  await restarted.wait("return document.querySelector('#draftsDialog')?.open", 15000, 'lista de rascunhos');
  assertIncludes(await restarted.text('#draftsList'), 'Echo', 'lista de rascunhos');
  await restarted.clickText('#draftsList button', 'Restaurar');
  await restarted.waitCurrentUrl('{{baseUrl}}/rascunho-real');
  assertIncludes(await restarted.text('#saveState'), 'protegido', 'estado após restaurar');
  const canonicalAfter = await readText(requestFile(root));
  assertEqual(canonicalAfter, canonicalBefore, 'YAML canônico intacto após reabrir');
  await env.screenshot('D-02-reabertura-rascunho');
}

async function caseD03(env) {
  const { page, root } = env;
  await page.waitApp();
  await page.selectRequest(ECHO);
  await page.wait("return document.querySelector('#url')?.value.includes('/echo')");
  await page.setInput('#url', '{{baseUrl}}/editado-guardiao');
  await page.selectRequest(STATUS);
  await page.wait("return document.querySelector('#navGuardDialog')?.open");
  await sleep(250);
  assertEqual(await page.exec('return document.activeElement?.id'), 'navSaveBtn', 'foco inicial no guardião');
  assertEqual(await page.attr('#saveState', 'role'), 'status', 'role do estado de salvamento');
  await env.screenshot('D-03-guardiao-navegacao');

  await page.click('#navCancelBtn');
  await page.wait("return !document.querySelector('#navGuardDialog')?.open");
  await page.waitCurrentUrl('{{baseUrl}}/editado-guardiao');
  assertIncludes(await page.text('#requestPath'), 'Echo', 'Cancelar conserva a requisição');

  await page.selectRequest(STATUS);
  await page.wait("return document.querySelector('#navGuardDialog')?.open");
  await page.click('#navKeepBtn');
  await page.wait("return document.querySelector('#url')?.value.includes('/status/404')");
  let protectedDrafts = null;
  for (let attempt = 0; attempt < 40; attempt += 1) {
    protectedDrafts = await page.ipc('/api/drafts');
    if (protectedDrafts.drafts?.some((draft) => draft.requestId === ECHO && draft.generation >= 1)) break;
    await sleep(150);
  }
  assert(
    protectedDrafts.drafts?.some((draft) => draft.requestId === ECHO && draft.generation >= 1),
    `Manter precisa proteger o rascunho; lista: ${JSON.stringify(protectedDrafts)}`,
  );

  await page.selectRequest(ECHO);
  await page.wait("return document.querySelector('#url')?.value.includes('/echo')");
  await page.setInput('#url', '{{baseUrl}}/salvo-guardiao');
  await page.selectRequest(STATUS);
  await page.wait("return document.querySelector('#navGuardDialog')?.open");
  await page.click('#navSaveBtn');
  await page.wait("return document.querySelector('#url')?.value.includes('/status/404')");
  assertIncludes(await readText(requestFile(root)), 'salvo-guardiao', 'Salvar grava o YAML antes de navegar');

  await page.selectRequest(ECHO);
  await page.waitCurrentUrl('{{baseUrl}}/salvo-guardiao');
  await page.setInput('#url', '{{baseUrl}}/descartado-guardiao');
  await sleep(2400);
  await page.selectRequest(STATUS);
  await page.wait("return document.querySelector('#navGuardDialog')?.open");
  await page.click('#navDiscardBtn');
  await confirmNative(page);
  await page.wait("return document.querySelector('#url')?.value.includes('/status/404')");
  const afterDiscard = await page.ipc('/api/drafts');
  assert(
    !afterDiscard.drafts?.some((draft) => draft.requestId === ECHO),
    'Descartar precisa remover o rascunho',
  );
  assertIncludes(await readText(requestFile(root)), 'salvo-guardiao', 'Descartar não altera o YAML');
}

async function caseD04(env) {
  const { page, root } = env;
  await page.waitApp();
  await page.selectRequest(ECHO);
  await page.wait("return document.querySelector('#url')?.value.includes('/echo')");
  const file = requestFile(root);
  await page.setInput('#url', '{{baseUrl}}/local-conflito');
  await page.wait(
    "return document.querySelector('#saveState')?.textContent.includes('rascunho protegido')",
    10000,
    'rascunho antes do conflito',
  );
  let text = await readText(file);
  await writeFile(file, text.replace('{{baseUrl}}/echo?origem=reqlivo', '{{baseUrl}}/externo'));
  await page.wait(
    "return !document.querySelector('#externalBanner')?.hidden && document.querySelector('#externalBannerText')?.textContent.includes('Alteração externa')",
    12000,
    'aviso de alteração externa',
  );
  await page.click('#externalCompareBtn');
  await page.wait("return document.querySelector('#conflictDialog')?.open");
  assertIncludes(await page.value('#conflictLocal'), '/local-conflito', 'comparação local');
  assertIncludes(await page.value('#conflictDisk'), '/externo', 'comparação do disco');
  await env.screenshot('D-04-comparacao-conflito');

  await page.click('#keepMineBtn');
  await page.wait("return !document.querySelector('#conflictDialog')?.open");
  assertIncludes(await page.currentUrl(), '/local-conflito', 'Manter preserva a edição');
  assertIncludes(await readText(file), '/externo', 'Manter não sobrescreve o disco');

  await page.click('#externalCompareBtn');
  await page.wait("return document.querySelector('#conflictDialog')?.open");
  await page.click('#applyOverBtn');
  await confirmNative(page);
  await page.wait(
    "return document.querySelector('#saveState')?.textContent.trim().startsWith('salvo · revisão')",
    15000,
    'aplicar sobre o disco',
  );
  assertIncludes(await readText(file), '/local-conflito', 'Aplicar grava a edição local');

  await page.setInput('#url', '{{baseUrl}}/local-dois');
  await page.wait(
    "return document.querySelector('#saveState')?.textContent.includes('rascunho protegido')",
    10000,
    'segundo rascunho',
  );
  text = await readText(file);
  await writeFile(file, text.replace('/local-conflito', '/externo-dois'));
  await page.wait(
    "return !document.querySelector('#externalBanner')?.hidden && document.querySelector('#externalBannerText')?.textContent.includes('Alteração externa')",
    12000,
    'segundo aviso externo',
  );
  await page.click('#externalCompareBtn');
  await page.wait("return document.querySelector('#conflictDialog')?.open");
  await page.click('#useDiskBtn');
  await confirmNative(page);
  await page.waitCurrentUrl('{{baseUrl}}/externo-dois', 15000);
  const drafts = await page.ipc('/api/drafts');
  assert(
    !drafts.drafts?.some((draft) => draft.requestId === ECHO),
    'Usar disco precisa descartar o rascunho',
  );
  assertIncludes(await readText(file), '/externo-dois', 'Usar disco mantém o arquivo');
}

async function caseD05(env) {
  const { page, root } = env;
  await page.waitApp();
  await page.selectRequest(ECHO);
  await page.wait("return document.querySelector('#url')?.value.includes('/echo')");
  const file = requestFile(root);
  const original = await readText(file);
  await writeFile(file, 'info: [\n');
  await page.wait(
    "return !document.querySelector('#externalBanner')?.hidden",
    15000,
    'aviso de YAML inválido',
  );
  const banner = await page.text('#externalBannerText');
  assert(
    banner.includes('YAML inválido') || banner.includes('Não foi possível recarregar'),
    `aviso de YAML inválido esperado, obtido: ${banner}`,
  );
  assertIncludes(await page.currentUrl(), '/echo', 'editor preservado com YAML inválido');
  await env.screenshot('D-05-yaml-invalido');

  await writeFile(file, original);
  await page.wait("return document.querySelector('#externalBanner')?.hidden", 15000, 'recuperação do YAML');
  await page.setInput('#url', '{{baseUrl}}/pos-correcao');
  await page.wait(
    "return document.querySelector('#saveState')?.textContent.includes('rascunho protegido')",
    10000,
    'rascunho após correção',
  );
  await page.click('#saveBtn');
  await page.wait(
    "return document.querySelector('#saveState')?.textContent.trim().startsWith('salvo · revisão')",
    15000,
    'salvamento após correção',
  );
  assertIncludes(await readText(file), '/pos-correcao', 'salvamento após YAML corrigido');
}

async function caseD06(env) {
  const { page, root } = env;
  await page.waitApp();
  await page.selectRequest(ECHO);
  await page.wait("return document.querySelector('#url')?.value.includes('/echo')");
  const file = requestFile(root);
  const originalBytes = await readText(file);
  assert(originalBytes.includes('/echo'), 'arquivo inicial');

  await page.click('#deleteRequestBtn');
  await confirmNative(page);
  await page.wait(
    "return !document.querySelector('#tree [data-tree-main][data-id=\"req_4a2b3c4d5e6f7081\"]')",
    12000,
    'requisição removida da árvore',
  );
  await page.click('#trashBtn');
  await page.wait("return document.querySelector('#trashDialog')?.open");
  await page.wait("return document.querySelector('#trashList')?.textContent.includes('Echo')", 10000);
  await page.setSelect('#trashParent', BASICOS);
  await page.click('#restoreTrashBtn');
  await page.wait(
    "return !document.querySelector('#trashResult')?.hidden && document.querySelector('#trashResult')?.textContent.includes('Restaurado')",
    15000,
    'restauração da lixeira',
  );
  await page.wait(
    "return !!document.querySelector('#tree [data-tree-main][data-id=\"req_4a2b3c4d5e6f7081\"]')",
    12000,
    'requisição restaurada',
  );
  assertEqual(await readText(file), originalBytes, 'bytes restaurados no caminho original');
  await env.screenshot('D-06-lixeira-restaurada');
}

async function caseD07(env) {
  const { page, root } = env;
  await page.waitApp();
  await page.click('#environmentsBtn');
  await page.wait("return document.querySelector('#envDialog')?.open");
  await page.wait(
    'return !!document.querySelector(\'#envVarsEditor input[aria-label="Valor da variável"]\')',
  );
  await page.exec(
    `return (() => { const input = document.querySelector('#envVarsEditor input[aria-label="Valor da variável"]'); input.value = 'form-local'; input.dispatchEvent(new Event('input', { bubbles: true })); return true; })()`,
  );
  const manifest = path.join(root, 'opencollection.yml');
  const text = await readText(manifest);
  await writeFile(manifest, text.replace('value: token-demo\n', 'value: token-externo\n'));
  await page.wait(
    "return !document.querySelector('#externalBanner')?.hidden && document.querySelector('#externalBannerText')?.textContent.includes('O manifesto mudou')",
    12000,
    'aviso de manifesto alterado',
  );
  assert(await page.visible('#envDialog'), 'formulário de ambiente deve permanecer aberto');
  assertEqual(
    await page.value('#envVarsEditor input[aria-label="Valor da variável"]'),
    'form-local',
    'valor digitado preservado',
  );

  await page.click('#saveEnvBtn');
  await page.wait(
    "return document.querySelector('#toast')?.textContent.includes('Conflito de revisão')",
    12000,
    'conflito de revisão do ambiente',
  );
  assertIncludes(await readText(manifest), 'token-externo', 'manifesto externo intacto');
  await page.click('#externalReloadBtn');
  await page.wait("return document.querySelector('#externalBanner')?.hidden", 15000, 'recarregar manifesto');
  assertEqual(
    await page.value('#envVarsEditor input[aria-label="Valor da variável"]'),
    'form-local',
    'formulário preservado após recarregar',
  );
  await page.click('#saveEnvBtn');
  await page.wait(
    "return document.querySelector('#toast')?.textContent.includes('Ambiente salvo')",
    12000,
    'salvamento explícito do ambiente',
  );
  assertIncludes(await readText(manifest), 'value: form-local', 'formulário aplicado ao manifesto');
  await env.screenshot('D-07-conflito-manifesto');
}

async function caseD08(env) {
  const { page } = env;
  await page.waitApp();
  await page.selectRequest(ECHO);
  await page.wait("return document.querySelector('#url')?.value.includes('/echo')");
  await page.exec(
    "return (() => { state.snapshot.generation = 'cursor-de-outra-colecao'; window.dispatchEvent(new Event('focus')); return true; })()",
  );
  await sleep(1800);
  assertEqual(
    await page.exec("return state.snapshot.generation === 'cursor-de-outra-colecao'"),
    false,
    'cursor antigo deve ser apenas realinhado',
  );
  assertIncludes(await page.text('#requestPath'), 'Echo', 'seleção preservada no cursor antigo');
  await env.screenshot('D-08-cursor-antigo');
}

async function caseD09(env) {
  const { page } = env;
  await page.waitApp();
  await page.selectRequest(ECHO);
  await page.wait("return document.querySelector('#url')?.value.includes('/echo')");
  await page.setInput('#url', '{{baseUrl}}/acessivel');
  await page.selectRequest(STATUS);
  await page.wait("return document.querySelector('#navGuardDialog')?.open");
  await sleep(250);
  assertEqual(await page.exec('return document.activeElement?.id'), 'navSaveBtn', 'foco inicial');
  await page.key('\ue004');
  await sleep(250);
  assertEqual(await page.exec('return document.activeElement?.id'), 'navKeepBtn', 'Tab entre botões');
  await page.key('\ue00c');
  await page.wait("return !document.querySelector('#navGuardDialog')?.open", 5000, 'Escape fecha o guardião');
  await page.waitCurrentUrl('{{baseUrl}}/acessivel');
  assertIncludes(await page.text('#requestPath'), 'Echo', 'Escape conserva a requisição');
  await env.screenshot('D-09-foco-teclado');
}

async function caseD10(env) {
  const { page } = env;
  await page.waitApp();
  await page.selectRequest(ECHO);
  await page.wait("return document.querySelector('#url')?.value.includes('/echo')");
  await page.setInput('#url', '{{baseUrl}}/delay');
  const started = Date.now();
  await page.click('#sendBtn');
  await page.wait("return !document.querySelector('#cancelSendBtn')?.hidden", 5000, 'botão Cancelar');
  await sleep(300);
  await page.click('#cancelSendBtn');
  await page.wait(
    "return document.querySelector('#responseStatus')?.textContent.toLowerCase().includes('cancelled') || document.querySelector('#responseBody')?.textContent.includes('requisição cancelada')",
    8000,
    'resposta cancelada',
  );
  await page.wait("return document.querySelector('#cancelSendBtn')?.hidden", 8000, 'envio encerrado');
  const elapsed = Date.now() - started;
  assert(elapsed < 4500, `cancelamento demorou ${elapsed} ms`);
  await env.screenshot('D-10-cancelamento');
}

async function caseD11(env) {
  const { page, root } = env;
  await page.wait("return document.querySelector('#migrationDialog')?.open", 20000, 'diálogo de migração');
  assertIncludes(await page.text('#migrationSummary'), 'Schema atual 0', 'schema pendente');
  await env.screenshot('D-11-migracao-pendente');
  await page.click('#migrationApplyBtn');
  await confirmNative(page);
  await page.waitApp();
  const manifest = path.join(root, 'opencollection.yml');
  assertIncludes(await readText(manifest), 'schemaVersion: 1', 'manifesto migrado');
  const backups = path.join(root, '.reqlivo/backups');
  const backupEntries = await import('node:fs/promises').then(({ readdir }) => readdir(backups).catch(() => []));
  assert(backupEntries.length > 0, 'backup da migração deve existir');

  await page.click('#migrationsBtn');
  await page.wait("return document.querySelector('#migrationDialog')?.open");
  await page.wait("return !document.querySelector('#migrationRollbackBtn')?.hidden", 10000, 'rollback elegível');
  await page.click('#migrationRollbackBtn');
  await confirmNative(page);
  await page.wait("return document.querySelector('#migrationDialog')?.open", 20000, 'migração pendente após rollback');
  assertIncludes(await page.text('#migrationSummary'), 'Schema atual 0', 'rollback restaura schema 0');
  assertIncludes(await readText(manifest), 'schemaVersion: 0', 'manifesto revertido');
  await env.screenshot('D-11-rollback');
}

// ---------------------------------------------------------------------------
// Execução da matriz
// ---------------------------------------------------------------------------

const CASES = [
  ['D-01', 'abrir coleção e enviar HTTP 200/404', caseD01],
  ['D-02', 'autosave e recuperação após reinício', caseD02],
  ['D-03', 'guardião de navegação', caseD03],
  ['D-04', 'alteração externa, comparação e escolha', caseD04],
  ['D-05', 'YAML inválido e recuperação', caseD05],
  ['D-06', 'excluir para lixeira e restaurar', caseD06],
  ['D-07', 'manifesto alterado durante edição de ambiente', caseD07],
  ['D-08', 'cursor de coleção anterior', caseD08],
  ['D-09', 'foco, Tab e Escape na WebView real', caseD09],
  ['D-10', 'cancelar execução HTTP', caseD10],
  ['D-11', 'migração e rollback com confirmação nativa', caseD11],
];

async function main() {
  await mkdir(EVIDENCE, { recursive: true });
  assert(
    (await import('node:fs')).existsSync(APP),
    `binário de release ausente: ${APP}; execute cargo build --release -p reqlivo-desktop`,
  );
  const webkitDriver = resolveWebKitDriver();
  const display = await ensureDisplay();
  const driver = await startDriver(webkitDriver);
  const synthetic = await startSyntheticServer();
  const selected = process.env.DESKTOP_ONLY
    ? CASES.filter(([id]) => id === process.env.DESKTOP_ONLY)
    : CASES;
  const results = [];
  let failures = 0;
  let sessionId = null;

  try {
    const binaryHash = createHash('sha256').update(await readFile(APP)).digest('hex');
    const system = outputOf('uname', ['-sr']) || `${process.platform} ${process.arch}`;
    const cpu = (await readFile('/proc/cpuinfo', 'utf8').catch(() => ''))
      .split('\n')
      .find((line) => line.startsWith('model name'))
      ?.split(':')[1]
      ?.trim() || 'desconhecida';
    const installed = outputOf('cargo', ['install', '--list']);
    const tauriVersion =
      installed.split('\n').find((line) => line.startsWith('tauri-driver ')) ||
      'tauri-driver (versão não exposta)';

    for (const [id, name, run] of selected) {
      const root = id === 'D-11'
        ? await prepareMigrationCollection(synthetic.port)
        : await prepareCollection(synthetic.port);
      sessionId = await createSession(root);
      const env = {
        root,
        get page() {
          return page;
        },
        async restart() {
          await deleteSession(sessionId);
          sessionId = await createSession(root);
          page = makePage(sessionId);
        },
        async screenshot(file) {
          const target = path.join(EVIDENCE, `${file}.png`);
          await page.screenshot(target);
          return target;
        },
      };
      let page = makePage(sessionId);
      const started = Date.now();
      try {
        await run(env);
        const target = await env.screenshot(`${id}-final`).catch(() => null);
        results.push({
          id,
          name,
          status: 'ok',
          elapsedMs: Date.now() - started,
          screenshot: target,
        });
        console.log(`[ok] ${id} — ${name} (${Date.now() - started} ms)`);
      } catch (error) {
        failures += 1;
        const target = await env.screenshot(`falha-${id}`).catch(() => null);
        results.push({
          id,
          name,
          status: 'falha',
          elapsedMs: Date.now() - started,
          error: error.message,
          screenshot: target,
        });
        console.error(`[falha] ${id} — ${name}: ${error.message}`);
      } finally {
        await deleteSession(sessionId).catch(() => {});
        sessionId = null;
        await rm(root, { recursive: true, force: true, maxRetries: 5, retryDelay: 100 }).catch(
          () => {},
        );
      }
    }

    const report = [
      '# Matriz real do aplicativo desktop — Linux/Tauri WebDriver',
      `Data: ${new Date().toISOString()}`,
      `Aplicativo: ${APP}`,
      `SHA-256 do binário: ${binaryHash}`,
      `Sistema: ${system} · CPU: ${cpu}`,
      `Driver: ${tauriVersion.replace(/:$/, '')} · WebKitWebDriver: ${WEBKIT_VERSION} (${webkitDriver})`,
      `Display: ${DISPLAY} (Xvfb 1600x1000x24, sem gerenciador de janelas)`,
      `Coleções: cópias descartáveis de examples/demo; servidor HTTP sintético em 127.0.0.1:${synthetic.port}`,
      `Casos: ${selected.length}, falhas: ${failures}`,
      '',
      ...results.map((result) =>
        result.status === 'ok'
          ? `[ok] ${result.id} — ${result.name} (${result.elapsedMs} ms)`
          : `[falha] ${result.id} — ${result.name}: ${result.error}`,
      ),
      '',
      '## Cobertura',
      '',
      '- Executado na WebView real: envio HTTP 200/404, cancelamento, autosave, recuperação após reinício, guardião, conflito externo com confirmação nativa, YAML inválido, lixeira, conflito de manifesto, cursor antigo, foco/Tab/Escape e migração/rollback com confirmação nativa.',
      '- Não executado: seleção nativa de pasta, importação/exportação por diálogo nativo, fechamento da janela com rascunho, leitor de tela/WCAG e plataforma Windows.',
      '- Os diálogos nativos GTK foram acionados por clique X11 nas coordenadas do Xvfb; isso não substitui uma revisão manual de usabilidade ou acessibilidade.',
      '- D-08 simula um cursor de coleção anterior no estado da UI; a troca real de coleção pelo seletor nativo não foi exercitada.',
      '',
    ].join('\n');
    await writeFile(path.join(EVIDENCE, 'desktop-acceptance.txt'), report);
    await writeFile(
      path.join(EVIDENCE, 'desktop-acceptance.json'),
      JSON.stringify(
        {
          date: new Date().toISOString(),
          app: APP,
          binarySha256: binaryHash,
          system,
          cpu,
          tauriDriver: tauriVersion.replace(/:$/, ''),
          webkitDriver,
          webkitVersion: WEBKIT_VERSION,
          display: DISPLAY,
          results,
          failures,
        },
        null,
        2,
      ),
    );
    console.log(`\n${selected.length - failures}/${selected.length} casos aprovados`);
    console.log(`Relatório: ${path.join(EVIDENCE, 'desktop-acceptance.txt')}`);
    if (failures) process.exitCode = 1;
  } finally {
    if (sessionId) await deleteSession(sessionId).catch(() => {});
    synthetic.server.closeAllConnections?.();
    synthetic.server.close();
    driver.kill('SIGTERM');
    killDriverProcesses();
    killAppProcesses();
    if (display) display.kill('SIGTERM');
  }
}

main().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
