#!/usr/bin/env node
// Validação automatizada dos fluxos de interface em Chrome headless.
//
// Usa o exemplo `ui_harness` (somente teste) e injeta um shim de `__TAURI__`
// que encaminha `api_request` para o roteador em memória. Nenhum dado real é
// usado: as coleções são cópias descartáveis de `examples/demo`.

import { spawn, spawnSync } from 'node:child_process';
import { cp, mkdir, mkdtemp, readFile, rename, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const REPO = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const HARNESS = path.join(REPO, 'target/debug/examples/ui_harness');
const CHROME = process.env.CHROME || 'google-chrome';
const EVIDENCE =
  process.env.UI_EVIDENCE || path.resolve(REPO, '../evidencias/reqlivo-v0.3.0/ui');
const DEMO = path.join(REPO, 'examples/demo');
const ECHO = 'req_4a2b3c4d5e6f7081';
const STATUS = 'req_5b3c4d5e6f708192';

const SHIM = `(() => {
  window.__CALLS__ = [];
  window.__CONFIRM__ = true;
  window.__FAIL_DRAFTS__ = false;
  const read = async (response) => {
    const text = await response.text();
    if (!text) return null;
    try { return JSON.parse(text); } catch (_) { return text; }
  };
  window.__TAURI__ = {
    core: {
      invoke: async (command, args = {}) => {
        if (command === 'api_request') {
          const method = args.method || 'GET';
          const path = args.path;
          const at = Date.now();
          if (window.__FAIL_DRAFTS__ && method === 'PUT' && String(path).startsWith('/api/drafts/')) {
            window.__CALLS__.push({ command, path, method, at, failed: true });
            throw new Error('disco indisponível (simulado)');
          }
          const init = { method };
          if (args.body !== null && args.body !== undefined) {
            init.headers = { 'content-type': 'application/json' };
            init.body = JSON.stringify(args.body);
          }
          const response = await fetch(path, init);
          const data = await read(response);
          window.__CALLS__.push({
            command, path, method, at, status: response.status,
            changed: data && typeof data === 'object' ? data.changed : undefined,
            reconcile: data && typeof data === 'object' ? data.reconcile : undefined,
          });
          return { status: response.status, data };
        }
        window.__CALLS__.push({ command, at: Date.now() });
        if (command === 'collection_status') return { status: 'ready', previousActive: true };
        if (command === 'confirm_action') return window.__CONFIRM__;
        if (command === 'migration_inspect') {
          return {
            inspection: { required: false, currentSchema: 1, targetSchema: 1, files: [], reasons: [], planId: null },
            operations: [],
          };
        }
        return null;
      },
    },
    window: {
      getCurrentWindow: () => ({ onCloseRequested: async () => {} }),
    },
  };
})();`;

const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

function buildHarness() {
  const result = spawnSync('cargo', ['build', '--locked', '--quiet', '--example', 'ui_harness'], {
    cwd: REPO,
    stdio: 'inherit',
  });
  if (result.status !== 0) throw new Error('falha ao compilar ui_harness');
}

async function launchChrome() {
  const profile = await mkdtemp(path.join(tmpdir(), 'reqlivo-ui-chrome-'));
  const child = spawn(
    CHROME,
    [
      '--headless=new',
      '--disable-gpu',
      '--no-sandbox',
      '--hide-scrollbars',
      '--window-size=1440,900',
      '--remote-debugging-port=0',
      `--user-data-dir=${profile}`,
      'about:blank',
    ],
    { stdio: ['ignore', 'ignore', 'pipe'] },
  );
  const url = await new Promise((resolve, reject) => {
    let buffer = '';
    const timer = setTimeout(() => reject(new Error('Chrome não expôs o DevTools')), 20000);
    child.stderr.on('data', (chunk) => {
      buffer += chunk.toString();
      const match = buffer.match(/DevTools listening on (ws:\/\/\S+)/);
      if (match) {
        clearTimeout(timer);
        resolve(match[1]);
      }
    });
    child.on('exit', (code) => reject(new Error(`Chrome encerrou (${code})`)));
  });
  return { child, profile, url };
}

function connect(url) {
  return new Promise((resolve, reject) => {
    const socket = new WebSocket(url);
    const pending = new Map();
    let nextId = 0;
    socket.addEventListener('error', () => reject(new Error('WebSocket do DevTools falhou')));
    socket.addEventListener('message', (event) => {
      const message = JSON.parse(event.data);
      if (!message.id) return;
      const entry = pending.get(message.id);
      if (!entry) return;
      pending.delete(message.id);
      if (message.error) entry.reject(new Error(message.error.message));
      else entry.resolve(message.result);
    });
    socket.addEventListener('open', () => {
      resolve({
        send(method, params = {}, sessionId) {
          const id = ++nextId;
          const payload = { id, method, params };
          if (sessionId) payload.sessionId = sessionId;
          socket.send(JSON.stringify(payload));
          return new Promise((res, rej) => pending.set(id, { resolve: res, reject: rej }));
        },
        close() {
          socket.close();
        },
      });
    });
  });
}

async function startHarness(root) {
  const child = spawn(HARNESS, [root], { stdio: ['ignore', 'pipe', 'pipe'] });
  const port = await new Promise((resolve, reject) => {
    let buffer = '';
    const timer = setTimeout(() => reject(new Error('harness não abriu a porta')), 15000);
    child.stdout.on('data', (chunk) => {
      buffer += chunk.toString();
      const match = buffer.match(/PORT=(\d+)/);
      if (match) {
        clearTimeout(timer);
        resolve(Number(match[1]));
      }
    });
    child.stderr.on('data', (chunk) => {
      buffer += chunk.toString();
    });
    child.on('exit', (code) => reject(new Error(`harness encerrou (${code}): ${buffer}`)));
  });
  return { child, port };
}

async function api(base, route, options = {}) {
  const init = { method: options.method || 'GET' };
  if (options.body !== undefined) {
    init.headers = { 'content-type': 'application/json' };
    init.body = JSON.stringify(options.body);
  }
  const response = await fetch(`${base}${route}`, init);
  const text = await response.text();
  let data = null;
  if (text) {
    try {
      data = JSON.parse(text);
    } catch (_) {
      data = text;
    }
  }
  return { status: response.status, data };
}

async function prepareCollection() {
  const root = await mkdtemp(path.join(tmpdir(), 'reqlivo-ui-'));
  await cp(DEMO, root, {
    recursive: true,
    filter: (source) => {
      const relative = path.relative(DEMO, source);
      return !relative.split(path.sep).includes('.reqlivo');
    },
  });
  return root;
}

async function openPage(client) {
  const { targetId } = await client.send('Target.createTarget', { url: 'about:blank' });
  const { sessionId } = await client.send('Target.attachToTarget', {
    targetId,
    flatten: true,
  });
  await client.send('Page.enable', {}, sessionId);
  await client.send('Runtime.enable', {}, sessionId);
  await client.send(
    'Page.addScriptToEvaluateOnNewDocument',
    { source: SHIM },
    sessionId,
  );
  return sessionId;
}

function pageApi(client, sessionId) {
  async function evaluate(expression) {
    const result = await client.send(
      'Runtime.evaluate',
      { expression, awaitPromise: true, returnByValue: true },
      sessionId,
    );
    if (result.exceptionDetails) {
      const description =
        result.exceptionDetails.exception?.description ||
        result.exceptionDetails.text ||
        'exceção na página';
      throw new Error(description);
    }
    return result.result.value;
  }

  async function waitFor(expression, timeout = 8000) {
    const deadline = Date.now() + timeout;
    let lastError = null;
    while (Date.now() < deadline) {
      try {
        if (await evaluate(expression)) return;
      } catch (error) {
        lastError = error;
      }
      await sleep(120);
    }
    throw new Error(`tempo esgotado aguardando: ${expression}${lastError ? ` (${lastError.message})` : ''}`);
  }

  async function navigate(url) {
    await client.send('Page.navigate', { url }, sessionId);
    await sleep(250);
    await waitFor("document.readyState === 'complete'");
  }

  async function click(selector) {
    const clicked = await evaluate(
      `(() => { const element = document.querySelector(${JSON.stringify(selector)}); if (!element) return false; element.click(); return true; })()`,
    );
    if (!clicked) throw new Error(`elemento ausente: ${selector}`);
  }

  async function clickText(selector, text) {
    const clicked = await evaluate(
      `(() => {
        const element = [...document.querySelectorAll(${JSON.stringify(selector)})]
          .find((item) => (item.textContent || '').trim() === ${JSON.stringify(text)});
        if (!element) return false;
        element.click();
        return true;
      })()`,
    );
    if (!clicked) throw new Error(`elemento com texto "${text}" ausente em ${selector}`);
  }

  async function setInput(selector, value) {
    const ok = await evaluate(
      `(() => {
        const element = document.querySelector(${JSON.stringify(selector)});
        if (!element) return false;
        element.value = ${JSON.stringify(value)};
        element.dispatchEvent(new Event('input', { bubbles: true }));
        return true;
      })()`,
    );
    if (!ok) throw new Error(`campo ausente: ${selector}`);
  }

  async function selectRequest(id) {
    await click(`#tree [data-tree-main][data-id="${id}"]`);
  }

  async function waitForApp() {
    await waitFor(
      "document.querySelector('#collectionName')?.textContent === 'Reqlivo Demo' && document.querySelectorAll('#tree [data-tree-main]').length >= 3",
      12000,
    );
  }

  async function screenshot(name) {
    const { data } = await client.send(
      'Page.captureScreenshot',
      { format: 'png', captureBeyondViewport: false },
      sessionId,
    );
    await writeFile(path.join(EVIDENCE, `${name}.png`), Buffer.from(data, 'base64'));
  }

  async function calls() {
    return evaluate('window.__CALLS__');
  }

  return {
    evaluate,
    waitFor,
    navigate,
    click,
    clickText,
    setInput,
    selectRequest,
    waitForApp,
    screenshot,
    calls,
    client,
    sessionId,
  };
}

function assert(condition, message) {
  if (!condition) throw new Error(message);
}

function assertEqual(actual, expected, message) {
  if (actual !== expected) {
    throw new Error(`${message}: esperado ${JSON.stringify(expected)}, obtido ${JSON.stringify(actual)}`);
  }
}

// ---------------------------------------------------------------------------
// Casos de interface
// ---------------------------------------------------------------------------

async function caseR02(env) {
  const { page, base, root } = env;
  await page.navigate(`${base}/`);
  await page.waitForApp();

  const view = await api(base, `/api/requests/${ECHO}`);
  assertEqual(view.status, 200, 'requisição base');
  const edited = structuredClone(view.data.payload.request);
  edited.http.url = '{{baseUrl}}/rascunho-restaurado';
  const saved = await api(base, `/api/drafts/${ECHO}`, {
    method: 'PUT',
    body: {
      request: edited,
      baseRevision: view.data.payload.revision,
      expectedGeneration: 0,
    },
  });
  assertEqual(saved.status, 200, 'rascunho semeado');

  // Reabrir a janela/processo: o estado em memória some, o disco permanece.
  await page.navigate(`${base}/`);
  await page.waitForApp();
  await page.waitFor("document.querySelector('#draftsDialog').open", 10000);
  const listed = await page.evaluate(
    "document.querySelector('#draftsList').textContent",
  );
  assert(listed.includes('Echo'), 'lista de rascunhos deve citar a requisição');
  assert(listed.includes('arquivo na revisão base'), 'estado atual esperado');
  await page.clickText('#draftsList button', 'Restaurar');
  await page.waitFor(
    "document.querySelector('#url').value === '{{baseUrl}}/rascunho-restaurado'",
  );
  const state = await page.evaluate("document.querySelector('#saveState').textContent");
  assert(state.includes('Alterações não salvas'), 'editor deve ficar sujo após restaurar');
  const canonical = await api(base, `/api/requests/${ECHO}`);
  assertEqual(
    canonical.data.payload.request.http.url,
    '{{baseUrl}}/echo?origem=reqlivo',
    'YAML canônico intacto',
  );
  const calls = await page.calls();
  assert(
    !calls.some((call) => String(call.path || '').startsWith('/api/send')),
    'nenhuma requisição HTTP deve ser enviada',
  );
  await page.screenshot('R-02-reabertura-rascunho');
}

async function caseR05(env) {
  const { page, base } = env;
  await page.navigate(`${base}/`);
  await page.waitForApp();
  await page.selectRequest(ECHO);
  await page.waitFor("document.querySelector('#url').value.includes('/echo')");
  const originalUrl = await page.evaluate("document.querySelector('#url').value");
  void originalUrl;

  await page.setInput('#url', '{{baseUrl}}/editado-guardiao');
  await page.selectRequest(STATUS);
  await page.waitFor("document.querySelector('#navGuardDialog').open");
  await sleep(200);
  assertEqual(
    await page.evaluate('document.activeElement.id'),
    'navSaveBtn',
    'foco inicial no botão Salvar',
  );
  await page.click('#navCancelBtn');
  await page.waitFor("!document.querySelector('#navGuardDialog').open");
  assertEqual(
    await page.evaluate("document.querySelector('#url').value"),
    '{{baseUrl}}/editado-guardiao',
    'Cancelar conserva a edição',
  );
  assert(
    (await page.evaluate("document.querySelector('#requestPath').textContent")).includes('Echo'),
    'Cancelar conserva a requisição',
  );

  await page.selectRequest(STATUS);
  await page.waitFor("document.querySelector('#navGuardDialog').open");
  await page.click('#navKeepBtn');
  await page.waitFor("document.querySelector('#url').value.includes('/status/404')");
  const drafts = await api(base, '/api/drafts');
  assert(
    drafts.data.drafts.some((draft) => draft.requestId === ECHO && draft.generation >= 1),
    'Manter deve proteger o rascunho antes de navegar',
  );

  await page.selectRequest(ECHO);
  await page.waitFor("document.querySelector('#url').value.includes('/echo')");
  await page.setInput('#url', '{{baseUrl}}/salvo-guardiao');
  await page.selectRequest(STATUS);
  await page.waitFor("document.querySelector('#navGuardDialog').open");
  await page.click('#navSaveBtn');
  await page.waitFor("document.querySelector('#url').value.includes('/status/404')");
  const saved = await api(base, `/api/requests/${ECHO}`);
  assertEqual(
    saved.data.payload.request.http.url,
    '{{baseUrl}}/salvo-guardiao',
    'Salvar altera o YAML antes de navegar',
  );

  await page.selectRequest(ECHO);
  await page.waitFor("document.querySelector('#url').value.includes('/salvo-guardiao')");
  await page.setInput('#url', '{{baseUrl}}/descartado-guardiao');
  await sleep(2400);
  await page.selectRequest(STATUS);
  await page.waitFor("document.querySelector('#navGuardDialog').open");
  await page.click('#navDiscardBtn');
  await page.waitFor("document.querySelector('#url').value.includes('/status/404')");
  const afterDiscard = await api(base, '/api/drafts');
  assert(
    !afterDiscard.data.drafts.some((draft) => draft.requestId === ECHO),
    'Descartar remove o rascunho confirado',
  );
  const untouched = await api(base, `/api/requests/${ECHO}`);
  assertEqual(
    untouched.data.payload.request.http.url,
    '{{baseUrl}}/salvo-guardiao',
    'Descartar não altera o YAML',
  );
  await page.screenshot('R-05-guardiao-navegacao');
}

async function caseR09(env) {
  const { page, base } = env;
  await page.navigate(`${base}/`);
  await page.waitForApp();
  await page.selectRequest(ECHO);
  await page.waitFor("document.querySelector('#url').value.includes('/echo')");

  await page.evaluate('window.__FAIL_DRAFTS__ = true; true');
  await page.setInput('#url', '{{baseUrl}}/r09-falha');
  await page.waitFor(
    "document.querySelector('#saveState').textContent.includes('Falha ao proteger rascunho')",
    6000,
  );
  assertEqual(
    await page.evaluate("document.querySelector('#url').value"),
    '{{baseUrl}}/r09-falha',
    'edição preservada após falha de proteção',
  );
  assert(
    (await page.calls()).some((call) => call.failed),
    'falha de transporte do rascunho registrada',
  );

  await page.evaluate('window.__FAIL_DRAFTS__ = false; true');
  await page.setInput('#url', '{{baseUrl}}/r09-recuperado');
  await page.waitFor(
    "document.querySelector('#saveState').textContent.includes('rascunho protegido')",
    6000,
  );
  const recovered = await api(base, `/api/drafts/${ECHO}`);
  assertEqual(recovered.status, 200, 'rascunho recuperado');
  assertEqual(
    recovered.data.record.request.http.url,
    '{{baseUrl}}/r09-recuperado',
    'conteúdo recuperado após a falha',
  );

  // Digitação contínua: o limite de cinco segundos deve disparar a proteção
  // antes da pausa de dois segundos, e a última edição deve ser protegida.
  await page.evaluate('window.__CALLS__ = []; true');
  const start = Date.now();
  for (let index = 0; index < 10; index += 1) {
    await page.setInput('#url', `{{baseUrl}}/r09-continuo-${index}`);
    await sleep(600);
  }
  await sleep(2800);
  const puts = (await page.calls()).filter(
    (call) => call.method === 'PUT' && String(call.path || '').startsWith('/api/drafts/'),
  );
  assert(
    puts.length >= 2,
    `duas gravações esperadas durante digitação contínua; obtidas ${puts.length}`,
  );
  const firstDelay = puts[0].at - start;
  assert(
    firstDelay >= 4200 && firstDelay <= 6400,
    `prazo máximo de 5 s respeitado; primeira gravação em ${firstDelay} ms`,
  );
  assert(
    puts[1].at - start > firstDelay,
    'gravação posterior após a pausa na digitação',
  );
  assert(
    (await page.evaluate("document.querySelector('#saveState').textContent")).includes('protegido'),
    'estado final deve indicar rascunho protegido',
  );
  const draft = await api(base, `/api/drafts/${ECHO}`);
  assertEqual(
    draft.data.record.request.http.url,
    '{{baseUrl}}/r09-continuo-9',
    'última edição contínua protegida',
  );
  await page.screenshot('R-09-falha-e-prazo');
}

async function caseE03(env) {
  const { page, base, root } = env;
  await page.navigate(`${base}/`);
  await page.waitForApp();
  await page.selectRequest(ECHO);
  await page.waitFor("document.querySelector('#url').value.includes('/echo')");
  const echoFile = path.join(root, 'basicos/echo.yml');

  await page.setInput('#url', '{{baseUrl}}/local-conflito');
  let text = await readFile(echoFile, 'utf8');
  text = text.replace('{{baseUrl}}/echo?origem=reqlivo', '{{baseUrl}}/externo');
  await writeFile(echoFile, text);
  await page.evaluate("window.dispatchEvent(new Event('focus')); true");
  await page.waitFor(
    "!document.querySelector('#externalBanner').hidden && document.querySelector('#externalBannerText').textContent.includes('Alteração externa')",
  );
  await page.click('#externalCompareBtn');
  await page.waitFor("document.querySelector('#conflictDialog').open");
  const local = await page.evaluate("document.querySelector('#conflictLocal').value");
  const disk = await page.evaluate("document.querySelector('#conflictDisk').value");
  assert(local.includes('/local-conflito'), 'comparação deve mostrar a edição local');
  assert(disk.includes('/externo'), 'comparação deve mostrar a versão atual do disco');

  await page.click('#keepMineBtn');
  await page.waitFor("!document.querySelector('#conflictDialog').open");
  assertEqual(
    await page.evaluate("document.querySelector('#url').value"),
    '{{baseUrl}}/local-conflito',
    'Manter preserva a edição',
  );
  assert(
    (await readFile(echoFile, 'utf8')).includes('/externo'),
    'Manter não sobrescreve o disco',
  );

  await page.click('#externalCompareBtn');
  await page.waitFor("document.querySelector('#conflictDialog').open");
  await page.click('#applyOverBtn');
  await page.waitFor("!document.querySelector('#conflictDialog').open");
  await sleep(800);
  const applied = await api(base, `/api/requests/${ECHO}`);
  assertEqual(
    applied.data.payload.request.http.url,
    '{{baseUrl}}/local-conflito',
    'Aplicar deve gravar a edição sobre a nova base',
  );

  await page.setInput('#url', '{{baseUrl}}/local-dois');
  await sleep(2400);
  text = await readFile(echoFile, 'utf8');
  await writeFile(echoFile, text.replace('/local-conflito', '/externo-dois'));
  await page.evaluate("window.dispatchEvent(new Event('focus')); true");
  await page.waitFor(
    "!document.querySelector('#externalBanner').hidden && document.querySelector('#externalBannerText').textContent.includes('Alteração externa')",
  );
  await page.click('#externalCompareBtn');
  await page.waitFor("document.querySelector('#conflictDialog').open");
  await page.click('#useDiskBtn');
  await page.waitFor("document.querySelector('#url').value === '{{baseUrl}}/externo-dois'");
  const diskView = await api(base, `/api/requests/${ECHO}`);
  assertEqual(
    diskView.data.payload.request.http.url,
    '{{baseUrl}}/externo-dois',
    'Usar disco mantém a versão do arquivo',
  );
  const drafts = await api(base, '/api/drafts');
  assert(
    !drafts.data.drafts.some((draft) => draft.requestId === ECHO),
    'Usar disco descarta o rascunho da edição',
  );
  await page.screenshot('E-03-comparacao-conflito');
}

async function caseE06(env) {
  const { page, base } = env;
  await page.navigate(`${base}/`);
  await page.waitForApp();
  await page.selectRequest(ECHO);
  await page.waitFor("document.querySelector('#url').value.includes('/echo')");
  await page.evaluate('window.__CALLS__ = []; true');
  await page.setInput('#url', '{{baseUrl}}/salvo-ui');
  await sleep(2400);
  const before = Date.now();
  await page.click('#saveBtn');
  await page.waitFor(
    "document.querySelector('#saveState').textContent.trim().startsWith('salvo · revisão')",
  );
  await sleep(3400);
  const calls = await page.calls();
  const reloads = calls.filter((call) => String(call.path || '').startsWith('/api/collection/reload'));
  assertEqual(reloads.length, 0, 'salvar pela UI não deve recarregar a coleção');
  const selfChanges = calls.filter(
    (call) => call.at >= before && call.changed === true,
  );
  assertEqual(selfChanges.length, 0, 'escrita própria não deve parecer alteração externa');
  const saved = await api(base, `/api/requests/${ECHO}`);
  assertEqual(
    saved.data.payload.request.http.url,
    '{{baseUrl}}/salvo-ui',
    'YAML salvo pela UI',
  );
  await page.screenshot('E-06-salvamento-sem-recarga');
}

async function caseE01(env) {
  const { page, base, root } = env;
  await page.navigate(`${base}/`);
  await page.waitForApp();
  await page.selectRequest(ECHO);
  await page.waitFor("document.querySelector('#url').value.includes('/echo')");
  const echoFile = path.join(root, 'basicos/echo.yml');
  const echoText = await readFile(echoFile, 'utf8');
  const externalText = echoText
    .replace(ECHO, 'req_00000000000000ab')
    .replace('name: Echo', 'name: Externo');
  await writeFile(path.join(root, 'basicos/externo.yml'), externalText);
  await sleep(3400);
  assert(
    (await page.evaluate("document.querySelector('#tree').textContent")).includes('Externo'),
    'arquivo criado externamente deve aparecer na árvore',
  );

  const atomic = echoText.replace(
    '{{baseUrl}}/echo?origem=reqlivo',
    '{{baseUrl}}/externo-atomico',
  );
  const temporary = path.join(root, 'basicos/.echo-tmp.yml');
  await writeFile(temporary, atomic);
  await rename(temporary, echoFile);
  await page.waitFor(
    "document.querySelector('#url').value === '{{baseUrl}}/externo-atomico'",
    9000,
  );

  await rename(
    path.join(root, 'basicos/externo.yml'),
    path.join(root, 'segredos/externo.yml'),
  );
  await sleep(3400);
  assert(
    (await page.evaluate("document.querySelector('#tree').textContent")).includes('Externo'),
    'arquivo movido deve continuar na árvore',
  );

  await rm(path.join(root, 'segredos/externo.yml'));
  await sleep(3400);
  assert(
    !(await page.evaluate("document.querySelector('#tree').textContent")).includes('Externo'),
    'arquivo removido deve sair da árvore',
  );
  await page.screenshot('E-01-arvore-externa');
}

async function caseE05(env) {
  const { page, base, root } = env;
  await page.navigate(`${base}/`);
  await page.waitForApp();
  await page.selectRequest(ECHO);
  await page.waitFor("document.querySelector('#url').value.includes('/echo')");
  const echoFile = path.join(root, 'basicos/echo.yml');
  const original = await readFile(echoFile, 'utf8');
  const generation = (await api(base, '/api/collection')).data.generation;

  await writeFile(echoFile, 'info: [\n');
  await page.evaluate("window.dispatchEvent(new Event('focus')); true");
  await page.waitFor("!document.querySelector('#externalBanner').hidden", 10000);
  const banner = await page.evaluate("document.querySelector('#externalBannerText').textContent");
  assert(
    banner.includes('YAML inválido') || banner.includes('Não foi possível recarregar'),
    `aviso de YAML inválido esperado, obtido: ${banner}`,
  );
  assert(
    (await page.evaluate("document.querySelector('#url').value")).includes('/echo'),
    'editor deve continuar utilizável com o último snapshot',
  );
  assertEqual(
    (await api(base, '/api/collection')).data.generation,
    generation,
    'snapshot confirmado não muda com YAML inválido',
  );

  await writeFile(echoFile, original);
  await page.evaluate("window.dispatchEvent(new Event('focus')); true");
  await page.waitFor("document.querySelector('#externalBanner').hidden", 12000);
  await page.setInput('#url', '{{baseUrl}}/pos-correcao');
  await sleep(2400);
  await page.click('#saveBtn');
  await page.waitFor(
    "document.querySelector('#saveState').textContent.trim().startsWith('salvo · revisão')",
  );
  const fixed = await api(base, `/api/requests/${ECHO}`);
  assertEqual(
    fixed.data.payload.request.http.url,
    '{{baseUrl}}/pos-correcao',
    'reconciliação posterior deve voltar a salvar',
  );
  await page.screenshot('E-05-yaml-invalido-recuperado');
}

async function caseE08(env) {
  const { page, base, root } = env;
  await page.navigate(`${base}/`);
  await page.waitForApp();
  await page.selectRequest(ECHO);
  await page.waitFor("document.querySelector('#url').value.includes('/echo')");
  const originalUrl = await page.evaluate("document.querySelector('#url').value");
  const echoFile = path.join(root, 'basicos/echo.yml');
  const renamed = path.join(root, 'basicos/renomeado.yml');
  await rename(echoFile, renamed);
  await sleep(3400);
  assert(
    (await page.evaluate("document.querySelector('#requestPath').textContent")).includes('Echo'),
    'seleção preservada após renomear o arquivo',
  );
  assertEqual(
    await page.evaluate("document.querySelector('#url').value"),
    originalUrl,
    'conteúdo preservado após renomear o arquivo',
  );
  const view = await api(base, `/api/requests/${ECHO}`);
  assertEqual(view.data.payload.path, 'basicos/renomeado.yml', 'caminho atualizado');

  await writeFile(path.join(root, 'basicos/duplicado.yml'), await readFile(renamed, 'utf8'));
  await page.evaluate("window.dispatchEvent(new Event('focus')); true");
  await page.waitFor(
    "!document.querySelector('#externalBanner').hidden && document.querySelector('#externalBannerText').textContent.includes('ID duplicado')",
    10000,
  );
  assertEqual(
    await page.evaluate(`document.querySelectorAll('[data-tree-main][data-id="${ECHO}"]').length`),
    1,
    'árvore anterior permanece com uma única entrada',
  );
  assertEqual(
    await page.evaluate("document.querySelector('#url').value"),
    originalUrl,
    'editor continua utilizável durante o conflito',
  );
  await rm(path.join(root, 'basicos/duplicado.yml'));
  await page.evaluate("window.dispatchEvent(new Event('focus')); true");
  await page.waitFor("document.querySelector('#externalBanner').hidden", 12000);
  await page.screenshot('E-08-renomear-e-id-duplicado');
}

async function caseE09(env) {
  const { page, base, root } = env;
  await page.navigate(`${base}/`);
  await page.waitForApp();
  await page.click('#environmentsBtn');
  await page.waitFor("document.querySelector('#envDialog').open");
  await page.waitFor("document.querySelector('#envVarsEditor input[aria-label=\"Valor da variável\"]')");
  await page.evaluate(
    `(() => {
      const input = document.querySelector('#envVarsEditor input[aria-label="Valor da variável"]');
      input.value = 'form-local';
      input.dispatchEvent(new Event('input', { bubbles: true }));
      return true;
    })()`,
  );

  const manifest = path.join(root, 'opencollection.yml');
  const text = await readFile(manifest, 'utf8');
  await writeFile(manifest, text.replace('value: token-demo\n', 'value: token-externo\n'));
  await page.evaluate("window.dispatchEvent(new Event('focus')); true");
  await page.waitFor(
    "!document.querySelector('#externalBanner').hidden && document.querySelector('#externalBannerText').textContent.includes('O manifesto mudou')",
  );
  assert(
    await page.evaluate("document.querySelector('#envDialog').open"),
    'formulário de ambiente deve permanecer aberto',
  );
  assertEqual(
    await page.evaluate(
      "document.querySelector('#envVarsEditor input[aria-label=\"Valor da variável\"]').value",
    ),
    'form-local',
    'valor digitado preservado',
  );

  await page.click('#saveEnvBtn');
  await page.waitFor(
    "document.querySelector('#toast').textContent.includes('Conflito de revisão')",
    10000,
  );
  const stale = await api(base, '/api/environments');
  const staleDev = stale.data.environments.find((item) => item.name === 'dev');
  assert(
    staleDev.variables.some((variable) => variable.name === 'token' && variable.value === 'token-demo'),
    'snapshot antigo não recarrega sozinho',
  );
  assert(!JSON.stringify(stale.data).includes('form-local'), 'corpo rejeitado não vaza para o estado');
  assert(
    (await readFile(manifest, 'utf8')).includes('token-externo'),
    'manifesto externo permanece no disco',
  );

  await page.click('#externalReloadBtn');
  await page.waitFor("document.querySelector('#externalBanner').hidden", 12000);
  await page.waitFor(
    "(async () => (await (await fetch('/api/environments')).json()).environments.find((item) => item.name === 'dev').variables.some((variable) => variable.value === 'token-externo'))()",
    10000,
  );
  assertEqual(
    await page.evaluate(
      "document.querySelector('#envVarsEditor input[aria-label=\"Valor da variável\"]').value",
    ),
    'form-local',
    'formulário preservado após recarregar',
  );

  await page.click('#saveEnvBtn');
  await page.waitFor(
    "document.querySelector('#toast').textContent.includes('Ambiente salvo')",
    10000,
  );
  const fresh = await api(base, '/api/environments');
  const dev = fresh.data.environments.find((item) => item.name === 'dev');
  assert(
    dev.variables.some((variable) => variable.name === 'baseUrl' && variable.value === 'form-local'),
    'salvamento explícito após recarregar aplica o formulário',
  );
  await page.screenshot('E-09-conflito-manifesto');
}

async function caseE07(env) {
  const { page, base } = env;
  await page.navigate(`${base}/`);
  await page.waitForApp();
  await page.selectRequest(ECHO);
  await page.waitFor("document.querySelector('#url').value.includes('/echo')");
  await page.evaluate('window.__CALLS__ = []; true');
  await page.evaluate("state.snapshot.generation = 'cursor-de-outra-colecao'; true");
  await page.evaluate("window.dispatchEvent(new Event('focus')); true");
  await sleep(1500);
  const calls = await page.calls();
  assertEqual(
    calls.filter((call) => String(call.path || '').startsWith('/api/collection/reload')).length,
    0,
    'cursor antigo não pode recarregar a árvore',
  );
  assertEqual(
    await page.evaluate("state.snapshot.generation === 'cursor-de-outra-colecao'"),
    false,
    'cursor antigo deve ser apenas realinhado',
  );
  assert(
    (await page.evaluate("document.querySelector('#requestPath').textContent")).includes('Echo'),
    'seleção preservada',
  );
  await page.screenshot('E-07-cursor-antigo');
}

async function caseG05(env) {
  const { page, base } = env;
  await page.navigate(`${base}/`);
  await page.waitForApp();
  await page.selectRequest(ECHO);
  await page.waitFor("document.querySelector('#url').value.includes('/echo')");
  await page.setInput('#url', '{{baseUrl}}/acessivel');
  await page.selectRequest(STATUS);
  await page.waitFor("document.querySelector('#navGuardDialog').open");
  await sleep(200);
  assertEqual(
    await page.evaluate('document.activeElement.id'),
    'navSaveBtn',
    'foco inicial no primeiro botão do guardião',
  );
  assertEqual(
    await page.evaluate("document.querySelector('#saveState').getAttribute('role')"),
    'status',
    'estado de salvamento anunciado',
  );
  assertEqual(
    await page.evaluate("document.querySelector('#saveState').getAttribute('aria-live')"),
    'polite',
    'aria-live no estado de salvamento',
  );
  assertEqual(
    await page.evaluate("document.querySelector('#navGuardDialog').getAttribute('aria-labelledby')"),
    'navGuardTitle',
    'diálogo rotulado',
  );

  const key = (type, keyName, code, keyCode) =>
    page.client.send(
      'Input.dispatchKeyEvent',
      {
        type,
        key: keyName,
        code,
        windowsVirtualKeyCode: keyCode,
        nativeVirtualKeyCode: keyCode,
      },
      page.sessionId,
    );
  await key('keyDown', 'Tab', 'Tab', 9);
  await key('keyUp', 'Tab', 'Tab', 9);
  await sleep(150);
  assertEqual(
    await page.evaluate('document.activeElement.id'),
    'navKeepBtn',
    'Tab percorre os botões do diálogo',
  );

  await key('keyDown', 'Escape', 'Escape', 27);
  await key('keyUp', 'Escape', 'Escape', 27);
  await page.waitFor("!document.querySelector('#navGuardDialog').open");
  assertEqual(
    await page.evaluate("document.querySelector('#url').value"),
    '{{baseUrl}}/acessivel',
    'Escape equivale a Cancelar',
  );
  assert(
    (await page.evaluate("document.querySelector('#requestPath').textContent")).includes('Echo'),
    'Escape conserva a requisição',
  );
  await page.screenshot('G-05-foco-e-escape');
}

// ---------------------------------------------------------------------------
// Execução
// ---------------------------------------------------------------------------

const CASES = [
  ['R-02', 'reabertura oferece recuperação integral', caseR02],
  ['R-05', 'guardião de navegação', caseR05],
  ['R-09', 'falha visível e prazo de digitação contínua', caseR09],
  ['E-01', 'criar/mover/renomear/remover externo', caseE01],
  ['E-03', 'comparação e escolha explícita', caseE03],
  ['E-05', 'YAML inválido e recuperação', caseE05],
  ['E-06', 'salvar sem recarga em cascata', caseE06],
  ['E-07', 'cursor de coleção anterior', caseE07],
  ['E-08', 'renomear mantendo ID e duplicar ID', caseE08],
  ['E-09', 'manifesto alterado durante edição', caseE09],
  ['G-05', 'foco, teclado e anúncios', caseG05],
];

async function main() {
  buildHarness();
  await mkdir(EVIDENCE, { recursive: true });
  const chrome = await launchChrome();
  const client = await connect(chrome.url);
  const sessionId = await openPage(client);
  const page = pageApi(client, sessionId);
  const results = [];
  let failures = 0;

  const selected = process.env.UI_ONLY
    ? CASES.filter(([id]) => id === process.env.UI_ONLY)
    : CASES;
  for (const [id, name, run] of selected) {
    const root = await prepareCollection();
    const harness = await startHarness(root);
    const base = `http://127.0.0.1:${harness.port}`;
    const env = { page, base, root, harness };
    const started = Date.now();
    try {
      await run(env);
      results.push(`[ok] ${id} — ${name} (${Date.now() - started} ms)`);
      console.log(results.at(-1));
    } catch (error) {
      failures += 1;
      results.push(`[falha] ${id} — ${name}: ${error.message}`);
      console.error(results.at(-1));
      await page.screenshot(`falha-${id}`).catch(() => {});
    } finally {
      harness.child.kill('SIGTERM');
      await sleep(200);
      await rm(root, { recursive: true, force: true, maxRetries: 5, retryDelay: 100 }).catch(
        () => {},
      );
      await page.navigate('about:blank').catch(() => {});
    }
  }

  client.close();
  chrome.child.kill('SIGTERM');
  await sleep(600);
  await rm(chrome.profile, { recursive: true, force: true, maxRetries: 5, retryDelay: 200 }).catch(
    () => {},
  );

  const report = [
    '# Validação de interface — Chrome headless',
    `Data: ${new Date().toISOString()}`,
    `Chrome: ${process.env.CHROME || 'google-chrome'}`,
    `Casos: ${CASES.length}, falhas: ${failures}`,
    '',
    ...results,
    '',
  ].join('\n');
  await writeFile(path.join(EVIDENCE, 'ui-acceptance.txt'), report);
  console.log(`\n${CASES.length - failures}/${CASES.length} casos aprovados`);
  if (failures) process.exitCode = 1;
}

main().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
