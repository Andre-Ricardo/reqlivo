'use strict';

/* ------------------------------------------------------------------------- */
/* Utilidades                                                                */
/* ------------------------------------------------------------------------- */

const $ = (selector, root = document) => root.querySelector(selector);
const $$ = (selector, root = document) => Array.from(root.querySelectorAll(selector));

const LS = {
  environment: 'reqlivo.environment',
  request: 'reqlivo.request',
  expanded: 'reqlivo.expanded',
};

const state = {
  snapshot: null,
  environments: null,
  health: null,
  current: null,
  dirty: false,
  environment: null,
  selectedFolder: null,
  expanded: new Set(),
  treeFilter: '',
  treeFocus: null,
  response: null,
  responseTab: 'body',
  rawBody: false,
  sendId: null,
  runId: null,
  runPoll: null,
  lastReport: null,
  exportContent: null,
  envEditing: null,
  envRevision: null,
  settingsRevision: null,
  edits: 0,
  draft: {
    generation: 0,
    exists: false,
    status: 'idle',
    timer: null,
    maxTimer: null,
    saving: false,
  },
  watcher: {
    timer: null,
    busy: false,
    pending: null,
    started: false,
  },
  conflict: null,
  navResolver: null,
  pending: null,
  previousActive: false,
  drafts: [],
};

const DRAFT_DEBOUNCE_MS = 2000;
const DRAFT_MAX_WAIT_MS = 5000;
const WATCH_INTERVAL_MS = 1500;

function node(tag, attrs = {}, ...children) {
  const element = document.createElement(tag);
  for (const [key, value] of Object.entries(attrs || {})) {
    if (value === undefined || value === null) continue;
    if (key === 'class') element.className = value;
    else if (key === 'dataset') Object.assign(element.dataset, value);
    else if (key === 'style') element.setAttribute('style', value);
    else if (key === 'value') element.value = value;
    else if (['checked', 'selected', 'disabled', 'hidden', 'readOnly'].includes(key)) {
      element[key] = Boolean(value);
    } else if (key.startsWith('on') && typeof value === 'function') {
      element.addEventListener(key.slice(2).toLowerCase(), value);
    } else if (value === true) {
      element.setAttribute(key, '');
    } else {
      element.setAttribute(key, String(value));
    }
  }
  for (const child of children.flat()) {
    if (child === null || child === undefined || child === false) continue;
    element.append(child instanceof Node ? child : document.createTextNode(String(child)));
  }
  return element;
}

function clear(element) {
  element.replaceChildren();
}

function option(value, label) {
  return node('option', { value, selected: false }, label);
}

function storeGet(key) {
  try { return localStorage.getItem(key); } catch (_) { return null; }
}

function storeSet(key, value) {
  try {
    if (value === null || value === undefined) localStorage.removeItem(key);
    else localStorage.setItem(key, String(value));
  } catch (_) { /* armazenamento opcional */ }
}

let toastTimer = null;
function toast(message, kind = 'ok', timeout = 4200) {
  const element = $('#toast');
  element.textContent = message;
  element.className = `toast${kind === 'error' ? ' is-error' : kind === 'warn' ? ' is-warn' : ''}`;
  element.hidden = false;
  clearTimeout(toastTimer);
  toastTimer = setTimeout(() => { element.hidden = true; }, timeout);
}

async function api(path, options = {}) {
  if (!window.__TAURI__) throw new Error('Abra o Reqlivo pelo aplicativo desktop.');
  const { status, data } = await window.__TAURI__.core.invoke('api_request', {
    path, method: options.method || 'GET', body: options.body ?? null,
  });
  if (status < 200 || status >= 300) {
    const info = data && data.error ? data.error : null;
    const error = new Error(info ? info.message : `${status}: ${data || 'operação recusada'}`);
    error.info = info;
    error.status = status;
    throw error;
  }
  return data;
}

async function confirmAction(message) {
  return window.__TAURI__.core.invoke('confirm_action', { message });
}

function hostInvoke(command, args = {}) {
  if (!window.__TAURI__) return Promise.reject(new Error('Operação disponível apenas no aplicativo desktop.'));
  return window.__TAURI__.core.invoke(command, args);
}

function encode(value) {
  return encodeURIComponent(value);
}

function clone(value) {
  return value === undefined ? undefined : JSON.parse(JSON.stringify(value));
}

function shortRevision(revision) {
  return revision ? revision.slice(0, 8) : '—';
}

function splitComma(value) {
  return value.split(',').map((item) => item.trim()).filter(Boolean);
}

function defaultSettings() {
  return {
    default_environment: null,
    default_timeout_ms: 30000,
    follow_redirects: true,
    max_redirects: 5,
    offline_only: false,
    allow_process_env: false,
    max_response_bytes: 10485760,
    script_timeout_ms: 2000,
  };
}

function currentSettings() {
  const extension = state.snapshot?.manifest?.extensions?.reqlivo;
  return { ...defaultSettings(), ...(extension?.settings || {}) };
}

/* ------------------------------------------------------------------------- */
/* Inicialização                                                             */
/* ------------------------------------------------------------------------- */

document.addEventListener('DOMContentLoaded', () => {
  bindStaticEvents();
  $('#openCollectionBtn').addEventListener('click', openCollection);
  window.__TAURI__.window.getCurrentWindow().onCloseRequested(async (event) => {
    event.preventDefault();
    if (!state.dirty) {
      await window.__TAURI__.core.invoke('close_window').catch((error) => toast(String(error), 'error'));
      return;
    }
    if (await guardNavigation('Fechar o Reqlivo com alterações não salvas?')) {
      await window.__TAURI__.core.invoke('close_window').catch((error) => toast(String(error), 'error'));
    }
  }).catch((error) => toast(String(error), 'error'));
  init().catch((error) => {
    showFatal(error);
  });
});

async function init() {
  let status = null;
  try {
    status = await hostInvoke('collection_status');
  } catch (error) {
    showFatal(error);
    return;
  }
  if (status?.status === 'pending') {
    state.pending = status;
    state.previousActive = Boolean(status.previousActive);
    if (!state.previousActive) {
      renderMigrationScreen(status);
      showDialog($('#migrationDialog'));
      return;
    }
    await loadAll();
    startWatcher();
    renderMigrationScreen(status);
    showDialog($('#migrationDialog'));
    return;
  }
  await loadAll();
  startWatcher();
}

async function loadAll() {
  setStatus('carregando coleção…');
  const [health, snapshot, environments] = await Promise.all([
    api('/api/health'),
    api('/api/collection'),
    api('/api/environments'),
  ]);
  state.health = health;
  state.snapshot = snapshot;
  state.environments = environments;
  state.environment = storeGet(LS.environment);
  if (state.environment && !environments.environments.some((env) => env.name === state.environment)) {
    state.environment = null;
  }
  if (!state.environment) state.environment = snapshot.default_environment || null;
  ensureExpanded();
  applyCollectionMeta();
  renderEnvironmentSelectors();
  renderTree();
  renderFolderSelects();
  renderWarnings();
  setStatus('');

  const last = storeGet(LS.request);
  if (last && treeHasRequest(last)) {
    await openRequest(last, true).catch(() => {});
  } else {
    showEmpty();
  }
  if (state.health.offline) {
    $('#offlineBadge').hidden = false;
  }
  await refreshDraftBadge();
  const pendingDrafts = state.drafts.filter((draft) => draft.state !== 'absent');
  if (pendingDrafts.length) {
    renderDraftsDialog();
    showDialog($('#draftsDialog'));
    toast(`${pendingDrafts.length} rascunho(s) protegido(s) aguardando decisão.`, 'warn', 6000);
  }
}

function applySnapshot(snapshot) {
  state.snapshot = snapshot;
  if (state.environment && snapshot.manifest.config.environments.every((env) => env.name !== state.environment)) {
    state.environment = snapshot.default_environment || null;
    storeSet(LS.environment, state.environment);
  }
  applyCollectionMeta();
  renderEnvironmentSelectors();
  renderTree();
  renderFolderSelects();
  renderWarnings();
  renderSaveState();
}

function showFatal(error) {
  setStatus('');
  $('#collectionName').textContent = 'erro ao carregar';
  $('#emptyState').hidden = false;
  $('#requestPanel').hidden = true;
  const card = $('.empty-card');
  clear(card);
  card.append(
    node('p', { class: 'eyebrow' }, 'Falha ao abrir a coleção'),
    node('h1', {}, 'Não foi possível carregar o Reqlivo'),
    node('p', {}, error.message),
    node('p', { class: 'muted' }, 'Verifique se o diretório contém opencollection.yml e abra outra coleção pelo botão Abrir coleção.'),
    node('div', { class: 'empty-actions' },
      node('button', { class: 'btn btn-primary', type: 'button', onclick: () => window.location.reload() }, 'Recarregar'),
    ),
  );
  toast(error.message, 'error');
}

function applyCollectionMeta() {
  const snapshot = state.snapshot;
  $('#collectionName').textContent = snapshot.manifest.info.name;
  $('#collectionRoot').textContent = snapshot.root;
  $('#collectionRoot').title = snapshot.root;
  document.title = `${snapshot.manifest.info.name} · Reqlivo`;
  $('#offlineBadge').hidden = !state.health?.offline && !currentSettings().offline_only;
}

function ensureExpanded() {
  if (!state.snapshot) return;
  let saved = null;
  try {
    saved = JSON.parse(storeGet(LS.expanded) || 'null');
  } catch (_) { saved = null; }
  if (Array.isArray(saved)) {
    state.expanded = new Set(saved);
  } else {
    state.expanded = new Set();
    walkFolders(state.snapshot.tree, (folder) => state.expanded.add(folder.id));
  }
}

function persistExpanded() {
  storeSet(LS.expanded, JSON.stringify(Array.from(state.expanded)));
}

function walkFolders(nodes, callback) {
  for (const item of nodes || []) {
    if (item.kind === 'folder') {
      callback(item);
      walkFolders(item.children, callback);
    }
  }
}

function allFolders() {
  const folders = [];
  if (!state.snapshot) return folders;
  walkFolders(state.snapshot.tree, (folder) => folders.push(folder));
  return folders;
}

function allRequests() {
  const requests = [];
  const walk = (nodes) => {
    for (const item of nodes || []) {
      if (item.kind === 'folder') walk(item.children);
      else requests.push(item);
    }
  };
  if (state.snapshot) walk(state.snapshot.tree);
  return requests;
}

function treeHasRequest(id) {
  return allRequests().some((request) => request.id === id);
}

function setStatus(text) {
  $('#collectionName').textContent = text || state.snapshot?.manifest?.info?.name || 'Reqlivo';
}

/* ------------------------------------------------------------------------- */
/* Ambientes e seletores                                                     */
/* ------------------------------------------------------------------------- */

function renderEnvironmentSelectors() {
  const environments = state.environments?.environments || [];
  const targets = [
    ['#environment', 'nenhum', state.environment],
    ['#runEnvironment', 'nenhum', state.environment],
    ['#setDefaultEnv', 'nenhum', state.snapshot?.default_environment || ''],
  ];
  for (const [selector, emptyLabel, selected] of targets) {
    const select = $(selector);
    if (!select) continue;
    clear(select);
    select.append(option('', emptyLabel));
    for (const environment of environments) {
      select.append(option(environment.name, environment.name));
    }
    select.value = selected || '';
  }
}

function renderFolderSelects() {
  const folders = allFolders();
  const pairs = [
    ['#runFolder', 'coleção inteira'],
    ['#importParent', 'raiz'],
  ];
  for (const [selector, rootLabel] of pairs) {
    const select = $(selector);
    if (!select) continue;
    const previous = select.value;
    clear(select);
    select.append(option('', rootLabel));
    for (const folder of folders) {
      select.append(option(folder.id, folder.name));
    }
    if (previous && folders.some((folder) => folder.id === previous)) select.value = previous;
  }
  renderImportParents();
}

/* ------------------------------------------------------------------------- */
/* Árvore                                                                    */
/* ------------------------------------------------------------------------- */

function filteredTree(nodes, term) {
  if (!term) return nodes || [];
  const result = [];
  for (const item of nodes || []) {
    if (item.kind === 'folder') {
      const children = filteredTree(item.children, term);
      if (item.name.toLowerCase().includes(term) || children.length > 0) {
        result.push({ ...item, children });
      }
    } else if (
      item.name.toLowerCase().includes(term)
      || (item.method || '').toLowerCase().includes(term)
      || (item.url || '').toLowerCase().includes(term)
    ) {
      result.push(item);
    }
  }
  return result;
}

function renderTree() {
  const tree = $('#tree');
  clear(tree);
  if (!state.snapshot) return;
  const term = state.treeFilter.trim().toLowerCase();
  const nodes = filteredTree(state.snapshot.tree, term);
  if (!nodes.length) {
    tree.append(node('p', { class: 'tree-empty' }, term ? 'Nenhum item corresponde ao filtro.' : 'Nenhuma requisição. Crie a primeira.'));
    return;
  }
  const appendNodes = (items, depth) => {
    for (const item of items) {
      if (item.kind === 'folder') {
        const expanded = term ? true : state.expanded.has(item.id);
        const row = node('div', {
          class: `tree-row${state.selectedFolder === item.id && !state.current ? ' is-selected' : ''}`,
          style: `padding-left:${depth * 0.85 + 0.2}rem`,
          role: 'none',
        });
        row.dataset.id = item.id;
        row.dataset.type = 'folder';
        row.append(
          node('button', {
            class: 'tree-caret',
            type: 'button',
            'aria-label': expanded ? `Recolher pasta ${item.name}` : `Expandir pasta ${item.name}`,
            'aria-expanded': expanded ? 'true' : 'false',
            onclick: (event) => {
              event.stopPropagation();
              toggleFolder(item.id);
            },
          }, expanded ? '▾' : '▸'),
          node('button', {
            class: 'tree-main',
            type: 'button',
            role: 'treeitem',
            'aria-level': String(depth + 1),
            dataset: { treeMain: 'true', id: item.id, type: 'folder' },
            onclick: () => {
              state.selectedFolder = item.id;
              state.current = null;
              toggleFolder(item.id, true);
              renderTree();
              showEmpty();
            },
          },
          node('span', { class: 'tree-name' }, item.name)),
          node('span', { class: 'tree-actions' },
            node('button', { class: 'tree-action', type: 'button', title: 'Nova requisição nesta pasta', 'aria-label': `Nova requisição em ${item.name}`, onclick: (event) => { event.stopPropagation(); createRequest(item.id); } }, '+R'),
            node('button', { class: 'tree-action', type: 'button', title: 'Nova subpasta', 'aria-label': `Nova subpasta em ${item.name}`, onclick: (event) => { event.stopPropagation(); createFolder(item.id); } }, '+F'),
            node('button', { class: 'tree-action', type: 'button', title: 'Renomear pasta', 'aria-label': `Renomear pasta ${item.name}`, onclick: (event) => { event.stopPropagation(); renameFolder(item); } }, '✎'),
            node('button', { class: 'tree-action', type: 'button', title: 'Excluir pasta', 'aria-label': `Excluir pasta ${item.name}`, onclick: (event) => { event.stopPropagation(); deleteFolder(item); } }, '🗑'),
          ),
        );
        tree.append(row);
        if (expanded) appendNodes(item.children || [], depth + 1);
      } else {
        const selected = state.current?.id === item.id;
        const row = node('div', {
          class: `tree-row${selected ? ' is-selected' : ''}`,
          style: `padding-left:${depth * 0.85 + 0.2}rem`,
          role: 'none',
        });
        row.dataset.id = item.id;
        row.dataset.type = 'request';
        row.append(
          node('span', { class: 'tree-caret', 'aria-hidden': 'true' }, ''),
          node('button', {
            class: 'tree-main',
            type: 'button',
            role: 'treeitem',
            'aria-level': String(depth + 1),
            dataset: { treeMain: 'true', id: item.id, type: 'request' },
            onclick: () => openRequest(item.id),
          },
          node('span', { class: 'tree-method' }, item.method || 'GET'),
          node('span', { class: 'tree-name' }, item.name)),
          node('span', { class: 'tree-actions' },
            node('button', { class: 'tree-action', type: 'button', title: 'Excluir requisição', 'aria-label': `Excluir requisição ${item.name}`, onclick: (event) => { event.stopPropagation(); deleteRequest(item.id); } }, '🗑'),
          ),
        );
        tree.append(row);
      }
    }
  };
  appendNodes(nodes, 0);
  if (state.treeFocus) {
    requestAnimationFrame(() => {
      const target = $(`[data-tree-main][data-id="${CSS.escape(state.treeFocus)}"]`);
      if (target) target.focus();
      state.treeFocus = null;
    });
  }
}

function toggleFolder(id, forceExpand = false) {
  if (forceExpand) state.expanded.add(id);
  else if (state.expanded.has(id)) state.expanded.delete(id);
  else state.expanded.add(id);
  persistExpanded();
  renderTree();
}

function handleTreeKeydown(event) {
  const focusables = $$('[data-tree-main]', $('#tree'));
  if (!focusables.length) return;
  const index = focusables.indexOf(document.activeElement);
  if (event.key === 'ArrowDown') {
    event.preventDefault();
    focusables[Math.min(focusables.length - 1, index + 1)]?.focus();
  } else if (event.key === 'ArrowUp') {
    event.preventDefault();
    focusables[Math.max(0, index - 1)]?.focus();
  } else if (event.key === 'Home') {
    event.preventDefault();
    focusables[0]?.focus();
  } else if (event.key === 'End') {
    event.preventDefault();
    focusables[focusables.length - 1]?.focus();
  } else if (event.key === 'ArrowRight' || event.key === 'ArrowLeft') {
    const target = document.activeElement;
    if (!target?.dataset?.id || target.dataset.type !== 'folder') return;
    event.preventDefault();
    if (event.key === 'ArrowRight') state.expanded.add(target.dataset.id);
    else state.expanded.delete(target.dataset.id);
    persistExpanded();
    state.treeFocus = target.dataset.id;
    renderTree();
  }
}

/* ------------------------------------------------------------------------- */
/* Requisição                                                                */
/* ------------------------------------------------------------------------- */

async function openRequest(id, force = false) {
  if (!force && !await guardNavigation('Abrir outra requisição?')) {
    return;
  }
  await flushDraft();
  const view = await api(`/api/requests/${encode(id)}`);
  state.current = {
    id,
    revision: view.payload.revision,
    path: view.payload.path,
    request: view.payload.request,
    tags: view.tags || [],
    folderPath: view.folder_path || [],
  };
  state.selectedFolder = null;
  state.dirty = false;
  state.edits = 0;
  state.draft = { generation: 0, exists: false, status: 'idle', timer: null, maxTimer: null, saving: false };
  state.response = null;
  state.rawBody = false;
  state.responseTab = 'body';
  storeSet(LS.request, id);
  hideDraftBanner();
  showRequest();
  renderRequest();
  renderTree();
  activateTab('params');
  renderResponseTabs();
  checkDraftForCurrent().catch(() => {});
}

function showEmpty() {
  $('#emptyState').hidden = false;
  $('#requestPanel').hidden = true;
}

function showRequest() {
  $('#emptyState').hidden = true;
  $('#requestPanel').hidden = false;
}

function ensureRequestArrays() {
  const request = state.current?.request;
  if (!request) return;
  request.http.params = request.http.params || [];
  request.http.headers = request.http.headers || [];
  request.runtime = request.runtime || {};
  request.runtime.scripts = request.runtime.scripts || [];
  request.runtime.assertions = request.runtime.assertions || [];
  request.info.tags = request.info.tags || [];
}

function renderRequest() {
  if (!state.current) return;
  ensureRequestArrays();
  const request = state.current.request;
  $('#requestName').value = request.info.name || '';
  $('#method').value = (request.http.method || 'GET').toUpperCase();
  if (!$('#method').value) $('#method').value = 'GET';
  $('#url').value = request.http.url || '';
  $('#tagsInput').value = (request.info.tags || []).join(', ');
  $('#docsInput').value = request.docs || '';
  renderPath();
  renderSaveState();
  renderParams();
  renderHeaders();
  renderBody();
  renderAuth();
  renderScripts();
  renderAssertions();
  renderResponsePlaceholder();
  updateLocalWarnings();
}

function renderPath() {
  if (!state.current) return;
  const parts = [...(state.current.folderPath || []), state.current.request.info.name || 'sem nome'];
  $('#requestPath').textContent = parts.join(' / ');
}

function renderSaveState() {
  const element = $('#saveState');
  if (!state.current) {
    element.textContent = '';
    element.className = 'save-state';
    return;
  }
  element.className = 'save-state';
  let label;
  if (state.draft.status === 'saving') {
    label = 'Protegendo rascunho…';
    element.classList.add('is-saving');
  } else if (state.draft.status === 'failed') {
    label = 'Falha ao proteger rascunho';
    element.classList.add('is-error');
  } else if (state.dirty) {
    label = state.draft.exists && state.draft.status === 'saved'
      ? 'Alterações não salvas · rascunho protegido'
      : 'Alterações não salvas';
    element.classList.add('is-dirty');
  } else if (state.draft.exists) {
    label = 'Rascunho protegido · ainda não salvo no YAML';
    element.classList.add('is-dirty');
  } else {
    label = `salvo · revisão ${shortRevision(state.current.revision)}`;
    element.classList.add('is-saved');
  }
  element.textContent = label;
  document.title = `${state.dirty ? '● ' : ''}${state.current.request.info.name || 'Requisição'} · Reqlivo`;
}

function markDirty(options = {}) {
  state.dirty = true;
  state.edits += 1;
  renderSaveState();
  updateLocalWarnings();
  scheduleDraft(Boolean(options.immediate));
}

function draftSnapshot() {
  return {
    requestId: state.current?.id || null,
    generation: state.draft.generation,
    baseRevision: state.current?.revision || null,
    dirty: state.dirty,
    status: state.draft.status,
  };
}

function clearDraftTimers() {
  if (state.draft.timer) clearTimeout(state.draft.timer);
  if (state.draft.maxTimer) clearTimeout(state.draft.maxTimer);
  state.draft.timer = null;
  state.draft.maxTimer = null;
}

function scheduleDraft(immediate = false) {
  if (!state.current) return;
  state.draft.status = 'dirty';
  renderSaveState();
  clearTimeout(state.draft.timer);
  if (immediate) {
    state.draft.timer = null;
    flushDraft();
    return;
  }
  state.draft.timer = setTimeout(() => flushDraft(), DRAFT_DEBOUNCE_MS);
  if (!state.draft.maxTimer) {
    state.draft.maxTimer = setTimeout(() => flushDraft(), DRAFT_MAX_WAIT_MS);
  }
}

async function flushDraft() {
  if (!state.current || !state.dirty) return !state.dirty;
  if (state.draft.saving) return false;
  const id = state.current.id;
  const generation = state.draft.generation;
  const baseRevision = state.current.revision;
  const editsAtStart = state.edits;
  clearDraftTimers();
  state.draft.saving = true;
  state.draft.status = 'saving';
  renderSaveState();
  try {
    const record = await api(`/api/drafts/${encode(id)}`, {
      method: 'PUT',
      body: {
        request: clone(state.current.request),
        baseRevision,
        expectedGeneration: generation || null,
      },
    });
    if (state.current?.id === id) {
      state.draft.generation = record.generation;
      state.draft.exists = true;
      state.draft.status = state.edits === editsAtStart ? 'saved' : 'dirty';
    }
    await refreshDraftBadge();
    return true;
  } catch (error) {
    state.draft.status = 'failed';
    toast(`Falha ao proteger rascunho: ${error.message}`, 'error', 8000);
    return false;
  } finally {
    state.draft.saving = false;
    renderSaveState();
    if (state.current?.id === id && state.edits !== editsAtStart && state.dirty) {
      scheduleDraft();
    }
  }
}

async function discardDraft({ deleteRecord = true, generation } = {}) {
  if (!state.current) return true;
  clearDraftTimers();
  if (deleteRecord && state.draft.exists) {
    try {
      await api(`/api/drafts/${encode(state.current.id)}?generation=${encode(generation ?? state.draft.generation)}`, { method: 'DELETE' });
    } catch (error) {
      toast(`Não foi possível descartar o rascunho: ${error.message}`, 'error', 7000);
      return false;
    }
  }
  state.draft.generation = 0;
  state.draft.exists = false;
  state.draft.status = 'idle';
  await refreshDraftBadge();
  return true;
}

async function refreshDraftBadge() {
  try {
    const data = await api('/api/drafts');
    state.drafts = data.drafts || [];
  } catch (_) {
    state.drafts = [];
  }
  const count = state.drafts.filter((draft) => draft.state !== 'absent').length;
  const button = $('#draftsBtn');
  if (!button) return;
  button.hidden = count === 0;
  button.textContent = `Rascunhos (${count})`;
}

function showDraftBanner(detail) {
  const banner = $('#draftBanner');
  const text = $('#draftBannerText');
  if (!detail || !detail.record) {
    banner.hidden = true;
    return;
  }
  const diverged = detail.summary.state === 'diverged';
  text.textContent = diverged
    ? 'Há um rascunho protegido e o arquivo mudou no disco. Compare antes de salvar.'
    : `Rascunho protegido em ${detail.record.updated_at}. Ele ainda não alterou o YAML.`;
  $('#restoreDraftBtn').hidden = diverged;
  $('#compareDraftBtn').hidden = !diverged;
  banner.hidden = false;
}

function hideDraftBanner() {
  $('#draftBanner').hidden = true;
  state.draftDetail = null;
}

async function checkDraftForCurrent() {
  if (!state.current) return;
  const detail = await api(`/api/drafts/${encode(state.current.id)}`);
  if (detail.summary?.state === 'absent' || !detail.record) {
    hideDraftBanner();
    return;
  }
  state.draft.generation = detail.summary.generation;
  state.draft.exists = true;
  state.draftDetail = detail;
  showDraftBanner(detail);
  renderSaveState();
}

function restoreDraft(detail) {
  if (!state.current || !detail?.record) return;
  state.current.request = clone(detail.record.request);
  state.draft.generation = detail.summary.generation;
  state.draft.exists = true;
  state.draft.status = 'saved';
  state.dirty = true;
  state.edits += 1;
  hideDraftBanner();
  renderRequest();
  renderTree();
  // Não agenda gravação: o rascunho já está protegido nesta geração.
  clearDraftTimers();
  renderSaveState();
}

/* ------------------------------------------------------------------------- */
/* Guardião único de navegação                                               */
/* ------------------------------------------------------------------------- */

function showNavGuard(message) {
  return new Promise((resolve) => {
    state.navResolver = resolve;
    $('#navGuardText').textContent = message || 'Escolha como continuar antes de sair desta edição.';
    $('#navGuardHint').textContent = '';
    $('#navSaveBtn').disabled = false;
    $('#navKeepBtn').disabled = false;
    showDialog($('#navGuardDialog'));
    $('#navSaveBtn').focus();
  });
}

function resolveNav(choice) {
  const resolve = state.navResolver;
  state.navResolver = null;
  if ($('#navGuardDialog').open) $('#navGuardDialog').close();
  if (resolve) resolve(choice);
}

async function guardNavigation(reason) {
  if (!state.dirty) return true;
  const choice = await showNavGuard(reason);
  if (choice === 'cancel' || !choice) return false;
  if (choice === 'save') {
    return saveCurrent();
  }
  if (choice === 'discard') {
    if (!await confirmAction('Descartar as alterações e remover o rascunho protegido? O YAML do disco não será alterado.')) {
      return false;
    }
    const ok = await discardDraft();
    if (!ok) return false;
    state.dirty = false;
    state.edits = 0;
    state.draft.status = 'idle';
    hideDraftBanner();
    renderSaveState();
    return true;
  }
  if (choice === 'keep') {
    const ok = await flushDraft();
    if (!ok && state.draft.status !== 'saved') {
      $('#navGuardHint').textContent = 'O rascunho não pôde ser protegido. Corrija o erro ou use Salvar/Cancelar.';
      return false;
    }
    return true;
  }
  return false;
}

/* ------------------------------------------------------------------------- */
/* Conflitos e comparação                                                    */
/* ------------------------------------------------------------------------- */

async function openConflictForCurrent() {
  if (!state.current) return;
  const id = state.current.id;
  let detail = null;
  let disk = null;
  try {
    detail = await api(`/api/drafts/${encode(id)}`);
  } catch (_) { detail = null; }
  try {
    // Comparação sempre com a versão atual do disco, não com o snapshot em
    // memória que o envio/runner continua usando.
    disk = await api(`/api/requests/${encode(id)}/disk`);
  } catch (error) {
    if (error.status === 404) {
      showExternalBanner('O arquivo desta requisição foi removido no disco. Sua edição continua em rascunho.', {
        compare: false,
        reload: true,
      });
      toast('Arquivo removido. Use “Rascunhos” para criar uma nova requisição a partir da edição.', 'warn', 8000);
      return;
    }
    throw error;
  }
  state.conflict = {
    requestId: id,
    local: clone(state.current.request),
    disk,
    generation: detail?.summary?.generation || state.draft.generation,
    baseRevision: state.current.revision,
    paths: state.watcher.pending?.paths || [],
  };
  $('#conflictSummary').textContent = 'O arquivo mudou fora do Reqlivo. Escolha explicitamente o que fazer; nada é sobrescrito automaticamente.';
  $('#conflictPaths').textContent = [
    `revisão base ${shortRevision(state.current.revision)}`,
    `revisão do disco ${shortRevision(disk.payload.revision)}`,
    state.conflict.paths.length ? `caminhos: ${state.conflict.paths.join(', ')}` : '',
  ].filter(Boolean).join(' · ');
  $('#conflictLocal').value = JSON.stringify(state.conflict.local, null, 2);
  $('#conflictDisk').value = JSON.stringify(disk.payload.request, null, 2);
  $('#useDiskBtn').hidden = false;
  $('#applyOverBtn').hidden = false;
  showDialog($('#conflictDialog'));
}

/// Alinha o snapshot ativo com a revisão atual do disco antes de uma escrita
/// explícita. Preserva a edição local suja; só atualiza revisão/caminho.
async function alignActiveSnapshot() {
  const changes = await api('/api/collection/changes');
  if (!changes.changed) return true;
  const snapshot = await reloadCollection(changes.generation, { silent: true });
  return Boolean(snapshot);
}

async function useDiskVersion() {
  if (!state.conflict) return;
  if (!await confirmAction('Descartar a edição local e usar exatamente a versão do disco?')) return;
  const { generation } = state.conflict;
  if (!await alignActiveSnapshot()) return;
  let disk;
  try {
    disk = await api(`/api/requests/${encode(state.current.id)}/disk`);
  } catch (error) {
    toast(`A versão do disco não pôde ser relida: ${error.message}`, 'error', 8000);
    return;
  }
  state.current.request = clone(disk.payload.request);
  state.current.revision = disk.payload.revision;
  state.current.path = disk.payload.path;
  state.current.tags = disk.payload.request.info?.tags || [];
  state.dirty = false;
  state.edits = 0;
  clearDraftTimers();
  if (generation) {
    await api(`/api/drafts/${encode(state.current.id)}?generation=${encode(generation)}`, { method: 'DELETE' }).catch(() => {});
  }
  state.draft = { generation: 0, exists: false, status: 'idle', timer: null, maxTimer: null, saving: false };
  state.conflict = null;
  hideDraftBanner();
  hideExternalBanner();
  $('#conflictDialog').close();
  renderRequest();
  renderTree();
  await refreshDraftBadge();
  toast('Versão do disco carregada.');
}

async function keepLocalVersion() {
  if (!state.conflict) return;
  state.conflict = null;
  $('#conflictDialog').close();
  if (state.dirty && !state.draft.exists) {
    scheduleDraft(true);
  }
  toast('Sua edição foi mantida e continua protegida por rascunho.', 'warn');
}

async function applyOverDisk() {
  if (!state.conflict) return;
  if (!await confirmAction('Aplicar sua edição sobre a revisão atual do disco? A versão do disco será substituída pelo YAML salvo.')) return;
  state.conflict = null;
  $('#conflictDialog').close();
  try {
    // A precondição do salvamento precisa ser a revisão recém-lida do disco.
    if (!await alignActiveSnapshot()) return;
    await saveCurrent();
  } catch (error) {
    toast(error.message, 'error', 8000);
  }
}

/* ------------------------------------------------------------------------- */
/* Alterações externas                                                       */
/* ------------------------------------------------------------------------- */

function showExternalBanner(text, options = {}) {
  $('#externalBannerText').textContent = text;
  $('#externalCompareBtn').hidden = options.compare === false;
  $('#externalReloadBtn').hidden = options.reload === false;
  $('#externalDismissBtn').hidden = options.dismiss === false;
  $('#externalBanner').hidden = false;
}

function hideExternalBanner() {
  $('#externalBanner').hidden = true;
}

function startWatcher() {
  if (state.watcher.started) return;
  state.watcher.started = true;
  const tick = async () => {
    await checkExternalChanges().catch(() => {});
    state.watcher.timer = setTimeout(tick, WATCH_INTERVAL_MS);
  };
  state.watcher.timer = setTimeout(tick, WATCH_INTERVAL_MS);
  window.addEventListener('focus', () => {
    checkExternalChanges(true).catch(() => {});
  });
  document.addEventListener('visibilitychange', () => {
    if (document.visibilityState === 'visible') checkExternalChanges(true).catch(() => {});
  });
}

async function checkExternalChanges(force = false) {
  if (!state.snapshot || state.watcher.busy) return;
  if (document.visibilityState === 'hidden' && !force) return;
  const checkedGeneration = state.snapshot.generation;
  state.watcher.busy = true;
  try {
    const data = await api(`/api/collection/changes?generation=${encode(checkedGeneration)}`);
    // Uma resposta em voo não pode reabrir aviso nem recarregar depois que o
    // snapshot mudou (recarga, salvamento ou troca de coleção).
    if (!state.snapshot || state.snapshot.generation !== checkedGeneration) return;
    if (!data.reconcile) {
      state.watcher.pending = null;
      hideExternalBanner();
      // Cursor de outra coleção ou de um snapshot já substituído: apenas
      // alinha a geração, sem recarregar a árvore nem perder a seleção.
      if (state.snapshot && data.generation) state.snapshot.generation = data.generation;
      return;
    }
    state.watcher.pending = data;
    const paths = (data.paths || []).join(', ') || 'arquivos da coleção';
    if (state.dirty && state.current) {
      showExternalBanner(`Alteração externa em ${paths}. Sua edição local foi preservada; compare antes de salvar.`);
      return;
    }
    if ($('#envDialog').open || $('#settingsDialog').open) {
      showExternalBanner(`O manifesto mudou (${paths}) enquanto um formulário está aberto. Nada foi substituído.`, {
        compare: false,
        reload: true,
      });
      return;
    }
    if (state.current && !treeHasRequest(state.current.id)) {
      showExternalBanner('O arquivo da requisição aberta foi removido no disco. A seleção foi preservada.', {
        compare: false,
        reload: true,
      });
      return;
    }
    await reloadCollection(data.generation, { silent: true });
    toast('Coleção atualizada a partir do disco.', 'ok', 2500);
  } catch (error) {
    showExternalBanner(`Falha ao reconciliar a coleção: ${error.message}`, { compare: false, reload: true });
  } finally {
    state.watcher.busy = false;
  }
}

async function reloadCollection(generation = null, options = {}) {
  const query = generation ? `?generation=${encode(generation)}` : '';
  try {
    const snapshot = await api(`/api/collection/reload${query}`, { method: 'POST' });
    await afterReload(snapshot, options);
    return snapshot;
  } catch (error) {
    showExternalBanner(`Não foi possível recarregar: ${error.message}`, { compare: false, reload: true });
    if (!options.silent) toast(error.message, 'error', 8000);
    return null;
  }
}

async function afterReload(snapshot, options = {}) {
  const currentId = state.current?.id || null;
  applySnapshot(snapshot);
  state.health = await api('/api/health').catch(() => state.health);
  state.environments = await api('/api/environments').catch(() => state.environments);
  state.watcher.pending = null;
  hideExternalBanner();
  // O formulário aberto foi preservado, mas passa a usar a revisão recém
  // recarregada como precondição do próximo salvamento explícito.
  if ($('#envDialog').open) state.envRevision = snapshot.manifest_revision || null;
  if ($('#settingsDialog').open) state.settingsRevision = snapshot.manifest_revision || null;
  if (currentId && treeHasRequest(currentId)) {
    try {
      const view = await api(`/api/requests/${encode(currentId)}`);
      // A base de comparação passa a ser a revisão atual do disco, mesmo com
      // edição local preservada; o conteúdo local só é trocado sem sujeira.
      state.current.revision = view.payload.revision;
      state.current.path = view.payload.path;
      if (!state.dirty && !state.draft.exists) {
        state.current.request = view.payload.request;
        state.current.tags = view.tags || [];
      }
      renderRequest();
      renderTree();
      if (state.dirty && !options.silent) {
        toast('A edição local continua aberta; compare com a nova versão do disco.', 'warn', 6000);
      }
    } catch (_) { /* a árvore já reflete a remoção */ }
  } else if (currentId) {
    state.current = null;
    showEmpty();
  }
  if (!options.silent) toast('Coleção recarregada.');
}

/* ------------------------------------------------------------------------- */
/* Rascunhos: lista e adoção                                                 */
/* ------------------------------------------------------------------------- */

function renderDraftsDialog() {
  const list = $('#draftsList');
  clear(list);
  const drafts = (state.drafts || []).filter((draft) => draft.state !== 'absent');
  if (!drafts.length) {
    list.append(node('p', { class: 'muted' }, 'Nenhum rascunho protegido nesta coleção.'));
    return;
  }
  for (const draft of drafts) {
    const requestName = allRequests().find((request) => request.id === draft.requestId)?.name;
    const labels = {
      current: 'arquivo na revisão base',
      diverged: 'arquivo mudou no disco',
      orphan: 'arquivo excluído',
      corrupt: 'registro ilegível',
      future: 'versão futura de rascunho',
    };
    const row = node('div', { class: 'draft-row', dataset: { draftId: draft.requestId } },
      node('div', { class: 'draft-info' },
        node('strong', {}, requestName || draft.requestId),
        node('span', { class: 'muted' }, `${labels[draft.state] || draft.state} · geração ${draft.generation}${draft.updatedAt ? ` · ${draft.updatedAt}` : ''}`),
        draft.error ? node('span', { class: 'error-text' }, draft.error) : null,
      ),
      node('div', { class: 'draft-actions' },
        draft.state === 'current' ? node('button', {
          type: 'button', class: 'btn btn-small',
          onclick: () => restoreDraftFromList(draft),
        }, 'Restaurar') : null,
        draft.state === 'diverged' ? node('button', {
          type: 'button', class: 'btn btn-small',
          onclick: () => compareDraftFromList(draft),
        }, 'Comparar…') : null,
        draft.state === 'orphan' ? node('button', {
          type: 'button', class: 'btn btn-small btn-primary',
          onclick: () => adoptDraftFromList(draft),
        }, 'Criar requisição') : null,
        draft.state !== 'corrupt' && draft.state !== 'future' ? node('button', {
          type: 'button', class: 'btn btn-small btn-danger',
          onclick: () => discardDraftFromList(draft),
        }, 'Descartar') : null,
      ),
    );
    list.append(row);
  }
}

async function restoreDraftFromList(draft) {
  try {
    const detail = await api(`/api/drafts/${encode(draft.requestId)}`);
    if (!detail.record) throw new Error('rascunho sem conteúdo recuperável');
    $('#draftsDialog').close();
    await openRequest(draft.requestId, true);
    restoreDraft(detail);
    toast('Rascunho restaurado no editor; o YAML não foi alterado.');
  } catch (error) {
    toast(error.message, 'error', 7000);
  }
}

async function compareDraftFromList(draft) {
  $('#draftsDialog').close();
  await openRequest(draft.requestId, true);
  await checkDraftForCurrent();
  await openConflictForCurrent();
}

async function adoptDraftFromList(draft) {
  const name = window.prompt('Nome da nova requisição a partir do rascunho:', 'Recuperada');
  if (name === null || !name.trim()) return;
  try {
    const payload = await api(`/api/drafts/${encode(draft.requestId)}/adopt`, {
      method: 'POST',
      body: { parentId: state.selectedFolder || null, name: name.trim() },
    });
    $('#draftsDialog').close();
    await refreshSnapshot();
    await openRequest(payload.id, true);
    await refreshDraftBadge();
    toast('Nova requisição criada a partir do rascunho.');
  } catch (error) {
    toast(error.message, 'error', 7000);
  }
}

async function discardDraftFromList(draft) {
  if (!await confirmAction(`Descartar o rascunho de “${draft.requestId}”? O YAML do disco não será alterado.`)) return;
  try {
    await api(`/api/drafts/${encode(draft.requestId)}?generation=${encode(draft.generation)}`, { method: 'DELETE' });
    if (state.current?.id === draft.requestId && state.draft.generation === draft.generation) {
      state.draft = { generation: 0, exists: false, status: 'dirty', timer: null, maxTimer: null, saving: false };
      hideDraftBanner();
      renderSaveState();
    }
    await refreshDraftBadge();
    renderDraftsDialog();
    toast('Rascunho descartado.');
  } catch (error) {
    toast(error.message, 'error', 7000);
  }
}

async function openDrafts() {
  await refreshDraftBadge();
  renderDraftsDialog();
  showDialog($('#draftsDialog'));
}

async function discardCurrentDraft() {
  if (!state.current) return;
  if (!await confirmAction('Descartar o rascunho protegido e voltar à versão do disco? As alterações não salvas serão perdidas.')) return;
  const ok = await discardDraft();
  if (!ok) return;
  try {
    const view = await api(`/api/requests/${encode(state.current.id)}`);
    state.current.request = view.payload.request;
    state.current.revision = view.payload.revision;
    state.current.path = view.payload.path;
    state.current.tags = view.tags || [];
  } catch (_) { /* arquivo removido: mantém a edição visível */ }
  state.dirty = false;
  state.edits = 0;
  hideDraftBanner();
  renderRequest();
  renderSaveState();
  toast('Rascunho descartado.');
}

/* ------------------------------------------------------------------------- */
/* Lixeira                                                                   */
/* ------------------------------------------------------------------------- */

async function openTrash() {
  $('#trashResult').hidden = true;
  showDialog($('#trashDialog'));
  renderFolderSelects();
  await loadTrash();
}

async function loadTrash() {
  const list = $('#trashList');
  clear(list);
  try {
    state.trash = await api('/api/trash');
  } catch (error) {
    list.append(node('p', { class: 'error-text' }, error.message));
    return;
  }
  if (!state.trash.length) {
    list.append(node('p', { class: 'muted' }, 'A lixeira está vazia.'));
    return;
  }
  state.trash.forEach((entry, index) => {
    const id = `trash-${entry.id}`;
    const row = node('label', { class: 'trash-row', for: id },
      node('input', { type: 'radio', name: 'trashEntry', id, value: entry.id, checked: index === 0 }),
      node('span', { class: 'trash-info' },
        node('strong', {}, entry.name || '(sem nome)'),
        node('span', { class: 'muted' }, `${entry.kind} · ${entry.origin || 'origem desconhecida'}${entry.deletedAt ? ` · ${entry.deletedAt}` : ''}${entry.legacy ? ' · legado' : ''}`),
        entry.error ? node('span', { class: 'error-text' }, entry.error) : null,
      ),
    );
    list.append(row);
  });
}

async function restoreSelectedTrash() {
  const selected = $('input[name="trashEntry"]:checked');
  const button = $('#restoreTrashBtn');
  const result = $('#trashResult');
  result.hidden = true;
  if (!selected) {
    toast('Selecione uma entrada da lixeira.', 'warn');
    return;
  }
  const entry = state.trash?.find((item) => item.id === selected.value);
  button.disabled = true;
  try {
    const outcome = await api(`/api/trash/${encode(selected.value)}/restore`, {
      method: 'POST',
      body: {
        revision: entry?.entryRevision || null,
        destinationParentId: $('#trashParent').value || null,
        name: $('#trashName').value.trim() || null,
      },
    });
    result.hidden = false;
    clear(result);
    result.append(node('p', {}, `Restaurado: ${outcome.path} (ID ${outcome.itemId}).`));
    $('#trashName').value = '';
    await loadTrash();
    await reloadCollection(null, { silent: true });
    toast('Item restaurado da lixeira.');
  } catch (error) {
    result.hidden = false;
    clear(result);
    result.append(node('p', { class: 'error-text' }, error.message));
    toast(error.message, 'error', 8000);
    await loadTrash();
  } finally {
    button.disabled = false;
  }
}

/* ------------------------------------------------------------------------- */
/* Migração e rollback                                                       */
/* ------------------------------------------------------------------------- */

function renderMigrationScreen(status) {
  state.pending = status;
  state.previousActive = Boolean(status.previousActive);
  const inspection = status.inspection;
  const summary = $('#migrationSummary');
  const files = $('#migrationFiles');
  clear(summary);
  clear(files);
  files.hidden = true;
  $('#migrationResult').hidden = true;
  $('#migrationCloseBtn').hidden = !state.previousActive;
  $('#migrationCancelBtn').textContent = state.previousActive ? 'Cancelar e manter coleção atual' : 'Fechar';
  const planId = inspection?.planId || null;
  const revision = inspection?.revision || null;
  state.pendingPlan = planId ? { planId, revision } : null;
  $('#migrationApplyBtn').disabled = !planId;
  if (status.error) {
    summary.append(node('p', { class: 'error-text' }, status.error));
  }
  if (status.root) {
    summary.append(node('p', { class: 'muted' }, `Coleção: ${status.root}`));
  }
  if (!inspection) {
    summary.append(node('p', {}, 'Não foi possível inspecionar a coleção. Use “Inspecionar novamente” ou abra outra coleção.'));
    $('#migrationRollbackBtn').hidden = true;
    return;
  }
  summary.append(node('p', {}, `Schema atual ${inspection.currentSchema} → destino ${inspection.targetSchema}.`));
  summary.append(node('p', {}, inspection.required
    ? 'Nenhum arquivo do projeto foi alterado pela inspeção. Confirme para aplicar com backup completo.'
    : 'A coleção está compatível; nenhuma migração é necessária.'));
  if (inspection.planId) {
    summary.append(node('p', { class: 'hint' }, `Plano ${inspection.planId} · backup previsto em .reqlivo/backups/${inspection.planId}`));
  }
  if (inspection.reasons?.length) {
    summary.append(node('h4', {}, 'Motivos'), node('ul', { class: 'plain-list' }, ...inspection.reasons.map((reason) => node('li', {}, reason))));
  }
  if (inspection.files?.length) {
    files.hidden = false;
    files.append(node('h3', {}, `Arquivos afetados (${inspection.files.length})`));
    files.append(node('ul', { class: 'plain-list' }, ...inspection.files.map((file) => node('li', {}, `${file.action} · ${file.path}`))));
  }
  const rollback = (status.operations || inspection.operations || []).find((operation) => operation.state === 'applied' && operation.rollbackEligible);
  $('#migrationRollbackBtn').hidden = !rollback;
  state.pendingRollback = rollback || null;
  if (rollback) {
    summary.append(node('p', { class: 'hint' }, `Rollback disponível para ${rollback.id}.`));
  }
}

async function applyPendingMigration() {
  if (!state.pendingPlan) {
    toast('Nenhum plano de migração disponível.', 'error');
    return;
  }
  if (!await confirmAction('Aplicar a migração agora? Um backup completo será gravado antes da primeira escrita.')) return;
  const button = $('#migrationApplyBtn');
  button.disabled = true;
  const result = $('#migrationResult');
  result.hidden = false;
  clear(result);
  result.append(node('p', {}, 'Aplicando migração…'));
  try {
    await hostInvoke('migration_apply', {
      planId: state.pendingPlan.planId,
      revision: state.pendingPlan.revision,
    });
    result.append(node('p', {}, 'Migração aplicada e verificada. Recarregando…'));
    window.location.reload();
  } catch (error) {
    clear(result);
    result.append(node('p', { class: 'error-text' }, String(error)));
    toast(String(error), 'error', 9000);
    button.disabled = false;
  }
}

async function rollbackPendingMigration() {
  const operation = state.pendingRollback;
  if (!operation) {
    toast('Nenhum rollback elegível.', 'warn');
    return;
  }
  if (!await confirmAction(`Reverter a migração ${operation.id}? Edições posteriores bloqueiam o rollback.`)) return;
  const result = $('#migrationResult');
  result.hidden = false;
  clear(result);
  result.append(node('p', {}, 'Revertendo…'));
  try {
    await hostInvoke('migration_rollback', {
      planId: operation.id,
      revision: operation.revision,
    });
    window.location.reload();
  } catch (error) {
    clear(result);
    result.append(node('p', { class: 'error-text' }, String(error)));
    toast(String(error), 'error', 9000);
  }
}

async function openMigrations() {
  try {
    const status = await hostInvoke('collection_status');
    if (status?.status === 'empty') {
      toast('Nenhuma coleção ativa. Use “Abrir coleção” para selecionar uma pasta.', 'warn');
      return;
    }
    const detail = await hostInvoke('migration_inspect');
    const operations = detail.operations || [];
    if (status?.status === 'pending') {
      renderMigrationScreen({ ...status, inspection: detail.inspection || status.inspection, operations });
      showDialog($('#migrationDialog'));
      return;
    }
    renderMigrationScreen({
      status: 'ready',
      root: detail.root,
      inspection: detail.inspection || null,
      operations,
      error: null,
      previousActive: true,
    });
    const summary = $('#migrationSummary');
    summary.append(node('h3', {}, 'Operações registradas'));
    if (!operations.length) {
      summary.append(node('p', { class: 'muted' }, 'Nenhuma operação de migração registrada.'));
    } else {
      summary.append(node('ul', { class: 'plain-list' }, ...operations.map((operation) => node('li', {},
        `${operation.id} · ${operation.state} · schema ${operation.currentSchema ?? '-'} → ${operation.targetSchema ?? '-'}${operation.blockedPaths?.length ? ` · bloqueado por edição em ${operation.blockedPaths.join(', ')}` : ''}`,
      ))));
    }
    showDialog($('#migrationDialog'));
  } catch (error) {
    toast(String(error), 'error', 8000);
  }
}

async function cancelMigration() {
  if (state.pending?.status !== 'pending') {
    $('#migrationDialog').close();
    return;
  }
  try {
    const status = await hostInvoke('cancel_pending');
    $('#migrationDialog').close();
    window.location.reload();
  } catch (error) {
    toast(String(error), 'error');
  }
}

function updateLocalWarnings() {
  const box = $('#requestWarnings');
  if (!state.current) {
    box.hidden = true;
    return;
  }
  const messages = [];
  const request = state.current.request;
  if (!request.http.url?.trim()) messages.push('A URL está vazia.');
  else if (!request.http.url.includes('://')) messages.push('A URL precisa incluir o esquema (por exemplo, http://).');
  if (request.http.body?.type === 'json' && request.http.body.data?.trim()) {
    try { JSON.parse(request.http.body.data); } catch (_) {
      messages.push('O corpo JSON ainda é inválido e o servidor recusará o envio.');
    }
  }
  if (messages.length) {
    box.hidden = false;
    box.textContent = messages.join(' ');
  } else {
    box.hidden = true;
    box.textContent = '';
  }
}

async function saveCurrent() {
  if (!state.current) return false;
  if (!state.current.request.info.name?.trim()) {
    toast('Dê um nome à requisição antes de salvar.', 'error');
    return false;
  }
  const button = $('#saveBtn');
  button.disabled = true;
  const editsAtStart = state.edits;
  const draftGenerationAtSave = state.draft.generation;
  const requestAtSave = clone(state.current.request);
  const revisionAtSave = state.current.revision;
  try {
    const payload = await api(`/api/requests/${encode(state.current.id)}`, {
      method: 'PUT',
      body: {
        request: requestAtSave,
        expectedRevision: revisionAtSave,
      },
    });
    state.current.revision = payload.revision;
    state.current.path = payload.path;
    if (state.edits === editsAtStart) {
      state.current.request = payload.request;
      state.current.tags = payload.request.info.tags || [];
      state.dirty = false;
      state.draft.status = 'idle';
      state.draft.exists = false;
      state.draft.generation = 0;
      clearDraftTimers();
      if (draftGenerationAtSave) {
        await api(`/api/drafts/${encode(state.current.id)}?generation=${encode(draftGenerationAtSave)}`, { method: 'DELETE' })
          .catch((error) => toast(`Requisição salva, mas o rascunho não pôde ser removido: ${error.message}`, 'warn', 7000));
      }
      await refreshDraftBadge();
    } else {
      // Novas edições durante o salvamento permanecem protegidas por rascunho.
      state.dirty = true;
      state.draft.status = 'dirty';
      scheduleDraft();
    }
    renderSaveState();
    await refreshSnapshot();
    toast(state.dirty ? 'Requisição salva; edições posteriores seguem em rascunho.' : 'Requisição salva.');
    return true;
  } catch (error) {
    state.draft.status = 'failed';
    renderSaveState();
    if (error.status === 409 || error.info?.category === 'storage' || error.info?.category === 'conflict') {
      toast(`${error.message} Sua edição e o rascunho foram preservados.`, 'error', 8000);
      openConflictForCurrent().catch(() => {});
    } else {
      toast(error.message, 'error', 6000);
    }
    return false;
  } finally {
    button.disabled = false;
  }
}

async function sendCurrent() {
  if (!state.current) return;
  if (!state.current.request.http.url?.trim()) {
    toast('Informe a URL antes de enviar.', 'error');
    return;
  }
  const sendButton = $('#sendBtn');
  const cancelButton = $('#cancelSendBtn');
  sendButton.disabled = true;
  sendButton.textContent = 'Enviando…';
  cancelButton.hidden = false;
  state.sendId = makeSendId();
  try {
    const data = await api('/api/send', {
      method: 'POST',
      body: {
        sendId: state.sendId,
        requestId: state.current.id,
        request: state.current.request,
        environment: state.environment || null,
      },
    });
    renderResponse(data.run, data.response);
    if (data.run.status === 'error' || data.run.status === 'failed' || data.run.status === 'cancelled') {
      const detail = data.run.error?.message || data.run.tests?.find((test) => !test.passed)?.message;
      toast(detail || 'A requisição não passou.', 'warn', 6000);
    }
  } catch (error) {
    toast(error.message, 'error', 7000);
  } finally {
    sendButton.disabled = false;
    sendButton.textContent = 'Enviar';
    cancelButton.hidden = true;
    state.sendId = null;
  }
}

function makeSendId() {
  const random = globalThis.crypto?.randomUUID?.().replace(/-/g, '').slice(0, 16)
    || Array.from({ length: 16 }, () => Math.floor(Math.random() * 16).toString(16)).join('');
  return `send_${random}`;
}

async function cancelSend() {
  if (!state.sendId) return;
  try {
    await api(`/api/send/${encode(state.sendId)}/cancel`, { method: 'POST' });
    toast('Cancelamento solicitado.', 'warn');
  } catch (error) {
    toast(error.message, 'error');
  }
}

function renderParams() {
  const container = $('#paramsEditor');
  clear(container);
  const items = state.current.request.http.params;
  if (!items.length) {
    container.append(node('p', { class: 'kv-empty' }, 'Nenhum parâmetro. Use “Adicionar” para criar.'));
    return;
  }
  for (const [index, item] of items.entries()) {
    container.append(node('div', { class: 'kv-row' },
      node('input', {
        type: 'checkbox',
        checked: !item.disabled,
        'aria-label': `Ativar parâmetro ${item.name || index + 1}`,
        onchange: (event) => { item.disabled = !event.target.checked; markDirty(); },
      }),
      node('select', {
        'aria-label': 'Tipo do parâmetro',
        onchange: (event) => { item.type = event.target.value; markDirty(); },
      },
      node('option', { value: 'query', selected: (item.type || 'query') === 'query' }, 'query'),
      node('option', { value: 'path', selected: item.type === 'path' }, 'path')),
      node('input', {
        value: item.name || '',
        placeholder: 'nome',
        'aria-label': 'Nome do parâmetro',
        oninput: (event) => { item.name = event.target.value; markDirty(); },
      }),
      node('input', {
        value: item.value || '',
        placeholder: 'valor',
        'aria-label': 'Valor do parâmetro',
        oninput: (event) => { item.value = event.target.value; markDirty(); },
      }),
      node('button', {
        class: 'kv-del',
        type: 'button',
        'aria-label': `Remover parâmetro ${item.name || index + 1}`,
        onclick: () => { items.splice(index, 1); markDirty(); renderParams(); },
      }, '✕'),
    ));
  }
}

function renderHeaders() {
  const container = $('#headersEditor');
  clear(container);
  const items = state.current.request.http.headers;
  if (!items.length) {
    container.append(node('p', { class: 'kv-empty' }, 'Nenhum cabeçalho explícito. Herança e auth continuam valendo.'));
    return;
  }
  for (const [index, item] of items.entries()) {
    container.append(node('div', { class: 'kv-row kv-row-simple' },
      node('input', {
        type: 'checkbox',
        checked: !item.disabled,
        'aria-label': `Ativar cabeçalho ${item.name || index + 1}`,
        onchange: (event) => { item.disabled = !event.target.checked; markDirty(); },
      }),
      node('input', {
        value: item.name || '',
        placeholder: 'Nome',
        'aria-label': 'Nome do cabeçalho',
        oninput: (event) => { item.name = event.target.value; markDirty(); },
      }),
      node('input', {
        value: item.value || '',
        placeholder: 'valor (aceita {{variavel}})',
        'aria-label': 'Valor do cabeçalho',
        oninput: (event) => { item.value = event.target.value; markDirty(); },
      }),
      node('button', {
        class: 'kv-del',
        type: 'button',
        'aria-label': `Remover cabeçalho ${item.name || index + 1}`,
        onclick: () => { items.splice(index, 1); markDirty(); renderHeaders(); },
      }, '✕'),
    ));
  }
}

/* ------------------------------- Corpo ---------------------------------- */

function bodyKind() {
  return state.current?.request?.http?.body?.type || 'none';
}

function renderBody() {
  const select = $('#bodyType');
  const dataArea = $('#bodyData');
  const formArea = $('#bodyForm');
  const kind = bodyKind();
  select.value = kind;
  const body = state.current.request.http.body;
  const isForm = kind === 'form-urlencoded' || kind === 'multipart-form';
  if (isForm) {
    dataArea.hidden = true;
    formArea.hidden = false;
    renderBodyForm();
  } else {
    formArea.hidden = true;
    dataArea.hidden = false;
    dataArea.value = body && typeof body.data === 'string' ? body.data : '';
    dataArea.placeholder = kind === 'json'
      ? '{\n  "exemplo": true\n}'
      : kind === 'none'
        ? 'Nenhum corpo será enviado.'
        : 'Conteúdo do corpo';
    dataArea.disabled = kind === 'none';
  }
  $('#formatJsonBtn').hidden = kind !== 'json';
}

function renderBodyForm() {
  const container = $('#bodyForm');
  clear(container);
  const kind = bodyKind();
  const items = state.current.request.http.body?.data || [];
  container.append(node('div', { class: 'panel-head' },
    node('div', {},
      node('h3', {}, kind === 'multipart-form' ? 'Campos multipart' : 'Campos do formulário'),
      node('p', { class: 'hint' }, kind === 'multipart-form'
        ? 'Campos do tipo arquivo usam um caminho local; o conteúdo é lido na execução.'
        : 'Pares nome/valor enviados como application/x-www-form-urlencoded.'),
    ),
    node('button', {
      class: 'btn btn-small',
      type: 'button',
      onclick: () => {
        items.push({ name: '', value: '', type: kind === 'multipart-form' ? 'text' : undefined, disabled: false });
        state.current.request.http.body.data = items;
        markDirty();
        renderBodyForm();
      },
    }, 'Adicionar campo'),
  ));
  if (!items.length) {
    container.append(node('p', { class: 'kv-empty' }, 'Nenhum campo.'));
    return;
  }
  for (const [index, item] of items.entries()) {
    const row = node('div', { class: 'kv-row' },
      node('input', {
        type: 'checkbox',
        checked: !item.disabled,
        'aria-label': `Ativar campo ${item.name || index + 1}`,
        onchange: (event) => { item.disabled = !event.target.checked; markDirty(); },
      }),
      node('input', {
        value: item.name || '',
        placeholder: 'nome',
        'aria-label': 'Nome do campo',
        oninput: (event) => { item.name = event.target.value; markDirty(); },
      }),
      node('input', {
        value: item.value || '',
        placeholder: kind === 'multipart-form' ? 'valor ou caminho do arquivo' : 'valor',
        'aria-label': 'Valor do campo',
        oninput: (event) => { item.value = event.target.value; markDirty(); },
      }),
      kind === 'multipart-form'
        ? node('select', {
            'aria-label': 'Tipo do campo multipart',
            onchange: (event) => { item.type = event.target.value; markDirty(); },
          },
          node('option', { value: 'text', selected: (item.type || 'text') === 'text' }, 'texto'),
          node('option', { value: 'file', selected: item.type === 'file' }, 'arquivo'))
        : node('span', { class: 'hint' }, ''),
      node('button', {
        class: 'kv-del',
        type: 'button',
        'aria-label': `Remover campo ${item.name || index + 1}`,
        onclick: () => { items.splice(index, 1); markDirty(); renderBodyForm(); },
      }, '✕'),
    );
    container.append(row);
  }
}

function changeBodyType(value) {
  const request = state.current.request;
  if (value === 'none') {
    request.http.body = null;
    markDirty();
    renderBody();
    return;
  }
  const old = request.http.body;
  const oldKind = old?.type;
  if (oldKind === value) return;
  const stringKinds = ['json', 'text', 'xml', 'sparql'];
  if (stringKinds.includes(value)) {
    request.http.body = { type: value, data: typeof old?.data === 'string' ? old.data : '' };
  } else {
    request.http.body = { type: value, data: Array.isArray(old?.data) ? old.data : [] };
  }
  markDirty();
  renderBody();
}

/* -------------------------------- Auth ---------------------------------- */

function authMode(auth) {
  if (!auth) return 'none';
  if (auth === 'inherit') return 'inherit';
  return auth.type || 'none';
}

function setAuthMode(mode) {
  const request = state.current.request;
  const old = request.http.auth;
  if (mode === 'none') request.http.auth = null;
  else if (mode === 'inherit') request.http.auth = 'inherit';
  else if (mode === 'basic') request.http.auth = { type: 'basic', username: old?.username || '', password: old?.password || '' };
  else if (mode === 'bearer') request.http.auth = { type: 'bearer', token: old?.token || '' };
  else if (mode === 'apikey') request.http.auth = { type: 'apikey', key: old?.key || '', value: old?.value || '', placement: old?.placement || 'header' };
  markDirty();
  renderAuth();
}

function renderAuth() {
  const container = $('#authEditor');
  clear(container);
  const request = state.current.request;
  const auth = request.http.auth;
  const mode = authMode(auth);
  container.append(node('label', {}, 'Modo',
    node('select', {
      onchange: (event) => setAuthMode(event.target.value),
    },
    node('option', { value: 'inherit', selected: mode === 'inherit' }, 'herdar da pasta/coleção'),
    node('option', { value: 'none', selected: mode === 'none' }, 'sem autenticação'),
    node('option', { value: 'basic', selected: mode === 'basic' }, 'Basic'),
    node('option', { value: 'bearer', selected: mode === 'bearer' }, 'Bearer'),
    node('option', { value: 'apikey', selected: mode === 'apikey' }, 'API key'))));

  if (mode === 'inherit') {
    container.append(node('p', { class: 'hint' }, 'A autenticação efetiva será resolvida da pasta folha até a coleção.'));
    return;
  }
  if (mode === 'none') {
    container.append(node('p', { class: 'hint' }, 'Esta requisição não envia autenticação própria e não herda auth.'));
    return;
  }
  const update = (key, value) => { request.http.auth[key] = value; markDirty(); renderAuthHint(); };
  if (mode === 'basic') {
    container.append(
      authField('Usuário', auth.username, (value) => update('username', value)),
      authField('Senha', auth.password, (value) => update('password', value)),
    );
  } else if (mode === 'bearer') {
    container.append(authField('Token', auth.token, (value) => update('token', value)));
  } else if (mode === 'apikey') {
    container.append(
      authField('Nome da chave', auth.key, (value) => update('key', value)),
      authField('Valor', auth.value, (value) => update('value', value)),
      node('label', {}, 'Local',
        node('select', {
          onchange: (event) => { request.http.auth.placement = event.target.value; markDirty(); },
        },
        node('option', { value: 'header', selected: (auth.placement || 'header') === 'header' }, 'cabeçalho'),
        node('option', { value: 'query', selected: auth.placement === 'query' }, 'query string'))),
    );
  }
  const hint = node('p', { class: 'hint', id: 'authHint' });
  container.append(hint);
  renderAuthHint();
}

function authField(label, value, oninput) {
  return node('label', {}, label,
    node('input', {
      value: value || '',
      spellcheck: 'false',
      autocomplete: 'off',
      oninput: (event) => oninput(event.target.value),
    }));
}

function renderAuthHint() {
  if (!state.current) return;
  const auth = state.current.request.http.auth;
  if (!auth || auth === 'inherit') return;
  const values = [auth.username, auth.password, auth.token, auth.value].filter((value) => value && !value.includes('{{'));
  const hint = $('#authHint');
  if (!hint) return;
  hint.textContent = values.length
    ? 'Há valores literais nesta auth. Prefira {{variavel}} ou um segredo referenciado para não gravar credenciais no projeto.'
    : 'Valores com {{variavel}} serão resolvidos em tempo de execução.';
  hint.classList.toggle('warn-text', values.length > 0);
}

/* ------------------------------ Scripts -------------------------------- */

function renderScripts() {
  const container = $('#scriptsEditor');
  clear(container);
  const scripts = state.current.request.runtime.scripts;
  if (!scripts.length) {
    container.append(node('p', { class: 'script-empty' }, 'Nenhum script. Use a API rq e as funções test/expect; o limite de execução é configurável.'));
    return;
  }
  for (const [index, script] of scripts.entries()) {
    const phase = script.type || 'before-request';
    const block = node('div', { class: 'script-block' },
      node('div', { class: 'script-head' },
        node('label', {}, 'Fase',
          node('select', {
            onchange: (event) => { script.type = event.target.value; markDirty(); },
          },
          node('option', { value: 'before-request', selected: phase === 'before-request' }, 'antes da requisição'),
          node('option', { value: 'after-response', selected: phase === 'after-response' }, 'após a resposta'),
          node('option', { value: 'tests', selected: phase === 'tests' }, 'testes'))),
        node('button', {
          class: 'btn btn-small btn-danger',
          type: 'button',
          onclick: () => { scripts.splice(index, 1); markDirty(); renderScripts(); },
        }, 'Remover')),
      node('textarea', {
        class: 'code-editor',
        spellcheck: 'false',
        placeholder: "test('status 200', () => { expect(rq.response.status).toBe(200); });",
        oninput: (event) => { script.code = event.target.value; markDirty(); },
      }, script.code || ''),
    );
    container.append(block);
  }
}

/* ------------------------------- Testes -------------------------------- */

const OPERATORS = [
  'eq', 'neq', 'gt', 'gte', 'lt', 'lte', 'contains', 'notContains',
  'startsWith', 'endsWith', 'matches', 'in', 'notIn', 'isString',
  'isNumber', 'isBoolean', 'isArray', 'isObject', 'isNull', 'exists',
  'notExists', 'lengthEq',
];

function renderAssertions() {
  const container = $('#testsEditor');
  clear(container);
  const assertions = state.current.request.runtime.assertions;
  if (!assertions.length) {
    container.append(node('p', { class: 'kv-empty' }, 'Nenhuma asserção declarativa. Scripts de teste também são executados.'));
    return;
  }
  for (const [index, assertion] of assertions.entries()) {
    container.append(node('div', { class: 'kv-row kv-row-assertion' },
      node('input', {
        type: 'checkbox',
        checked: !assertion.disabled,
        'aria-label': `Ativar asserção ${index + 1}`,
        onchange: (event) => { assertion.disabled = !event.target.checked; markDirty(); },
      }),
      node('input', {
        value: assertion.expression || '',
        placeholder: 'res.status',
        'aria-label': 'Expressão da asserção',
        oninput: (event) => { assertion.expression = event.target.value; markDirty(); },
      }),
      node('select', {
        'aria-label': 'Operador da asserção',
        onchange: (event) => { assertion.operator = event.target.value; markDirty(); },
      }, ...OPERATORS.map((operator) => node('option', { value: operator, selected: (assertion.operator || 'eq') === operator }, operator))),
      node('input', {
        value: assertion.value ?? '',
        placeholder: 'valor esperado',
        'aria-label': 'Valor esperado',
        oninput: (event) => { assertion.value = event.target.value === '' ? null : event.target.value; markDirty(); },
      }),
      node('button', {
        class: 'kv-del',
        type: 'button',
        'aria-label': `Remover asserção ${index + 1}`,
        onclick: () => { assertions.splice(index, 1); markDirty(); renderAssertions(); },
      }, '✕'),
    ));
  }
}

/* ----------------------------- Resposta -------------------------------- */

function renderResponsePlaceholder() {
  state.response = null;
  $('#responseStatus').textContent = 'sem resposta';
  $('#responseStatus').className = 'status-pill status-idle';
  $('#responseMeta').textContent = '';
  renderResponseTabs();
}

function renderResponse(run, response) {
  state.response = { run, response };
  const status = $('#responseStatus');
  if (run.error && !response) {
    status.textContent = `erro · ${run.error.code}`;
    status.className = 'status-pill status-error';
  } else if (response) {
    status.textContent = `${response.status} ${response.status_text || ''}`.trim();
    status.className = `status-pill ${response.status >= 200 && response.status < 400 ? 'status-ok' : response.status < 500 ? 'status-warn' : 'status-error'}`;
  } else {
    status.textContent = run.status || 'concluída';
    status.className = 'status-pill status-idle';
  }
  const parts = [];
  if (response) {
    parts.push(`${run.duration_ms} ms`);
    parts.push(`${response.size_bytes} bytes`);
    if (response.content_type) parts.push(response.content_type);
    if (response.truncated) parts.push('resposta truncada');
    if (response.redirected) {
      parts.push('redirecionada');
      if (response.final_url) parts.push(`final: ${response.final_url}`);
    }
  } else if (run.error) {
    parts.push(run.error.message);
  }
  $('#responseMeta').textContent = parts.join(' · ');
  renderResponseTabs();
}

function renderResponseTabs() {
  for (const tab of $$('[data-response-tab]')) {
    const active = tab.dataset.responseTab === state.responseTab;
    tab.classList.toggle('is-active', active);
    tab.setAttribute('aria-selected', active ? 'true' : 'false');
  }
  const container = $('#responseBody');
  clear(container);
  if (!state.response) {
    container.append(node('p', { class: 'muted' }, 'Envie a requisição para ver a resposta, os metadados, os testes e os logs redigidos.'));
    return;
  }
  const { run, response } = state.response;
  if (state.responseTab === 'body') renderResponseBody(container, run, response);
  else if (state.responseTab === 'headers') renderResponseHeaders(container, response);
  else if (state.responseTab === 'tests') renderResponseTests(container, run);
  else renderResponseLogs(container, run);
}

function prettyJson(text) {
  try {
    return JSON.stringify(JSON.parse(text), null, 2);
  } catch (_) {
    return text;
  }
}

function renderResponseBody(container, run, response) {
  if (run.error && !response) {
    container.append(node('div', { class: 'report-box' },
      node('h3', {}, `Erro ${run.error.code || ''}`.trim()),
      node('p', {}, run.error.message),
      node('p', { class: 'hint' }, `categoria: ${run.error.category} · tentar novamente: ${run.error.retryable ? 'sim' : 'não'}`),
    ));
    return;
  }
  if (!response) {
    container.append(node('p', { class: 'muted' }, 'A execução não produziu resposta.'));
    return;
  }
  const toolbar = node('div', { class: 'body-toolbar' },
    node('button', {
      class: 'btn btn-small',
      type: 'button',
      onclick: () => {
        state.rawBody = !state.rawBody;
        renderResponseTabs();
      },
    }, state.rawBody ? 'Mostrar formatado' : 'Mostrar bruto'),
    node('button', {
      class: 'btn btn-small',
      type: 'button',
      onclick: async () => {
        const text = response.body_text ?? '';
        try {
          await navigator.clipboard.writeText(text);
          toast('Corpo copiado.');
        } catch (_) {
          toast('Não foi possível copiar o corpo.', 'error');
        }
      },
    }, 'Copiar'),
  );
  container.append(toolbar);
  if (response.truncated) {
    container.append(node('p', { class: 'warn-text' }, 'A resposta excedeu o limite configurado e foi truncada.'));
  }
  if (response.body_text !== null && response.body_text !== undefined) {
    const text = state.rawBody ? response.body_text : prettyJson(response.body_text);
    container.append(node('pre', {}, text));
  } else if (response.is_binary) {
    container.append(
      node('p', { class: 'muted' }, `Corpo binário (${response.content_type || 'tipo desconhecido'}).`),
      node('pre', {}, (response.body_base64 || '').slice(0, 4000)),
    );
  } else {
    container.append(node('p', { class: 'muted' }, 'Corpo vazio.'));
  }
}

function renderResponseHeaders(container, response) {
  if (!response) {
    container.append(node('p', { class: 'muted' }, 'Sem cabeçalhos.'));
    return;
  }
  const table = node('table', { class: 'headers-table' });
  for (const [name, value] of response.headers || []) {
    table.append(node('tr', {}, node('td', {}, name), node('td', {}, value)));
  }
  container.append(table);
}

function renderResponseTests(container, run) {
  const tests = run.tests || [];
  if (!tests.length) {
    container.append(node('p', { class: 'muted' }, 'Nenhum teste ou asserção executado. A requisição é classificada como executada, não como aprovada.'));
    return;
  }
  const list = node('ul', { class: 'test-list' });
  for (const test of tests) {
    const detail = [];
    if (test.message) detail.push(test.message);
    if (test.expected != null) detail.push(`esperado: ${test.expected}`);
    if (test.actual != null) detail.push(`obtido: ${test.actual}`);
    list.append(node('li', { class: `test-item ${test.passed ? 'is-pass' : 'is-fail'}` },
      node('strong', {}, `${test.passed ? '✓' : '✕'} ${test.name}`),
      node('div', { class: 'detail' }, detail.join(' · ')),
      node('div', { class: 'hint' }, `origem: ${test.source}`),
    ));
  }
  container.append(list);
}

function renderResponseLogs(container, run) {
  const logs = run.logs || [];
  if (!logs.length) {
    container.append(node('p', { class: 'muted' }, 'Nenhum log de script.'));
    return;
  }
  const list = node('ul', { class: 'log-list' });
  for (const log of logs) {
    list.append(node('li', { class: 'log-item' },
      node('span', { class: 'level' }, log.level),
      node('span', {}, log.message),
      node('span', { class: 'hint' }, ` · ${log.phase}/${log.origin}`),
    ));
  }
  container.append(list);
}

/* ------------------------------------------------------------------------- */
/* Persistência estrutural                                                   */
/* ------------------------------------------------------------------------- */

async function refreshSnapshot() {
  const snapshot = await api('/api/collection');
  applySnapshot(snapshot);
  if (state.current && !treeHasRequest(state.current.id)) {
    state.current = null;
    state.dirty = false;
    showEmpty();
  }
}

function renderWarnings() {
  const button = $('#warningsBtn');
  const warnings = state.snapshot?.warnings || [];
  const unsupported = state.snapshot?.unsupported || [];
  if (!warnings.length && !unsupported.length) {
    button.hidden = true;
    return;
  }
  button.hidden = false;
  button.textContent = `${warnings.length + unsupported.length} aviso(s)`;
  button.onclick = () => {
    const lines = [
      ...warnings.map((warning) => `aviso: ${warning}`),
      ...unsupported.map((item) => `não suportado: ${item.path} (${item.kind}) · ${item.reason}`),
    ];
    toast(lines.join('\n'), 'warn', 9000);
  };
}

async function createRequest(parentId = state.selectedFolder) {
  if (!await guardNavigation('Criar uma nova requisição com alterações não salvas?')) return;
  const name = window.prompt('Nome da nova requisição:', 'Nova requisição');
  if (name === null) return;
  if (!name.trim()) {
    toast('Informe um nome.', 'error');
    return;
  }
  try {
    const payload = await api('/api/requests', {
      method: 'POST',
      body: {
        parentId: parentId || null,
        name: name.trim(),
        method: 'GET',
        url: 'http://127.0.0.1:18080/echo',
      },
    });
    await refreshSnapshot();
    await openRequest(payload.id, true);
    toast('Requisição criada.');
  } catch (error) {
    toast(error.message, 'error');
  }
}

async function createFolder(parentId = state.selectedFolder) {
  const name = window.prompt('Nome da nova pasta:', 'Nova pasta');
  if (name === null) return;
  if (!name.trim()) {
    toast('Informe um nome.', 'error');
    return;
  }
  try {
    const payload = await api('/api/folders', {
      method: 'POST',
      body: { parentId: parentId || null, name: name.trim() },
    });
    state.expanded.add(payload.id);
    persistExpanded();
    await refreshSnapshot();
    toast('Pasta criada.');
  } catch (error) {
    toast(error.message, 'error');
  }
}

async function renameFolder(folder) {
  const name = window.prompt('Novo nome da pasta:', folder.name);
  if (name === null || !name.trim() || name === folder.name) return;
  try {
    await api(`/api/folders/${encode(folder.id)}`, {
      method: 'PUT',
      body: { name: name.trim() },
    });
    await refreshSnapshot();
    toast('Pasta renomeada.');
  } catch (error) {
    toast(error.message, 'error');
  }
}

async function deleteFolder(folder) {
  if (!await confirmAction(`Mover a pasta “${folder.name}” e seu conteúdo para a lixeira?`)) return;
  try {
    await api(`/api/folders/${encode(folder.id)}`, { method: 'DELETE' });
    await refreshSnapshot();
    toast('Pasta movida para a lixeira.');
  } catch (error) {
    toast(error.message, 'error');
  }
}

async function deleteRequest(id) {
  if (!await confirmAction('Mover a requisição para a lixeira? O arquivo permanece recuperável em .reqlivo/trash.')) return;
  try {
    const revision = state.current?.id === id ? state.current.revision : null;
    const query = revision ? `?expectedRevision=${encode(revision)}` : '';
    await api(`/api/requests/${encode(id)}${query}`, { method: 'DELETE' });
    if (state.current?.id === id) {
      state.current = null;
      state.dirty = false;
      showEmpty();
    }
    await refreshSnapshot();
    toast('Requisição movida para a lixeira.');
  } catch (error) {
    toast(error.message, 'error');
  }
}

/* ------------------------------------------------------------------------- */
/* Runner                                                                    */
/* ------------------------------------------------------------------------- */

function openRunner() {
  renderEnvironmentSelectors();
  renderFolderSelects();
  $('#runEnvironment').value = state.environment || '';
  $('#runCurrentOnly').disabled = !state.current;
  showDialog($('#runnerDialog'));
}

async function startRun() {
  const tags = splitComma($('#runTags').value);
  const folderId = $('#runFolder').value || null;
  const requestIds = $('#runCurrentOnly').checked && state.current ? [state.current.id] : [];
  try {
    $('#startRunBtn').disabled = true;
    $('#cancelRunBtn').hidden = false;
    const data = await api('/api/run', {
      method: 'POST',
      body: {
        environment: $('#runEnvironment').value || null,
        filters: { tags, requestIds, folderId },
      },
    });
    state.runId = data.runId;
    $('#runResult').replaceChildren(node('p', { class: 'muted' }, 'Executando…'));
    pollRun();
  } catch (error) {
    toast(error.message, 'error');
    $('#startRunBtn').disabled = false;
    $('#cancelRunBtn').hidden = true;
  }
}

function pollRun() {
  if (!state.runId) return;
  clearTimeout(state.runPoll);
  const tick = async () => {
    try {
      const data = await api(`/api/run/${encode(state.runId)}`);
      if (data.status === 'running') {
        state.runPoll = setTimeout(tick, 600);
        return;
      }
      if (data.status === 'finished') {
        renderRunReport(data.report);
        toast('Execução concluída.');
      } else if (data.status === 'failed') {
        $('#runResult').replaceChildren(node('div', { class: 'report-box' },
          node('h3', {}, 'Falha na execução'),
          node('p', {}, data.error.message),
        ));
        toast(data.error.message, 'error');
      }
      state.runId = null;
      $('#startRunBtn').disabled = false;
      $('#cancelRunBtn').hidden = true;
    } catch (error) {
      state.runId = null;
      $('#startRunBtn').disabled = false;
      $('#cancelRunBtn').hidden = true;
      toast(error.message, 'error');
    }
  };
  tick();
}

async function cancelRun() {
  if (!state.runId) return;
  try {
    await api(`/api/run/${encode(state.runId)}/cancel`, { method: 'POST' });
    toast('Cancelamento solicitado.', 'warn');
  } catch (error) {
    toast(error.message, 'error');
  }
}

function renderRunReport(report) {
  state.lastReport = report;
  const box = $('#runResult');
  const summary = report.summary || {};
  const chips = [
    `total ${summary.total}`,
    `executadas ${summary.executed}`,
    `passaram ${summary.passed}`,
    `falharam ${summary.failed}`,
    `ignoradas ${summary.skipped}`,
    `erros ${summary.errors}`,
    `canceladas ${summary.cancelled}`,
  ];
  clear(box);
  box.append(
    node('h3', {}, `Execução ${report.id}`),
    node('p', { class: 'hint' }, `ambiente: ${report.environment || 'nenhum'} · ${report.duration_ms} ms · hash ${report.deterministic_hash}`),
    node('div', { class: 'run-summary' }, ...chips.map((chip) => node('span', {}, chip))),
  );
  const list = node('div', {});
  for (const request of report.requests || []) {
    const status = request.status || 'executed';
    const statusLabel = {
      executed: 'executada', passed: 'passou', failed: 'falhou',
      skipped: 'ignorada', cancelled: 'cancelada', error: 'erro',
    }[status] || status;
    const details = [];
    for (const test of (request.tests || []).filter((test) => !test.passed)) {
      details.push(`teste: ${test.name} · ${test.message}`);
    }
    if (request.error) details.push(`${request.error.code}: ${request.error.message}`);
    if (request.skip_reason) details.push(request.skip_reason);
    list.append(node('div', {},
      node('div', { class: 'run-row' },
        node('span', { class: 'mono' }, request.http_status ?? '—'),
        node('span', {}, request.name),
        node('span', { class: `status ${status}` }, statusLabel),
        node('span', { class: 'muted' }, `${request.duration_ms} ms`),
      ),
      details.length ? node('div', { class: 'hint', style: 'padding:0 0.3rem 0.45rem' }, details.join(' · ')) : null,
    ));
  }
  box.append(list);
  loadHistory();
}

async function loadHistory() {
  try {
    const data = await api('/api/history?limit=20');
    const list = $('#runHistory');
    clear(list);
    list.hidden = false;
    list.append(node('h3', {}, 'Histórico local'));
    if (!data.runs?.length) {
      list.append(node('p', { class: 'muted' }, 'Sem execuções registradas.'));
      return;
    }
    for (const run of data.runs.slice().reverse()) {
      const summary = run.summary || {};
      list.append(node('div', { class: 'history-item' },
        node('span', { class: 'mono' }, run.runId || ''),
        ` · ${run.environment || 'sem ambiente'} · `,
        `passaram ${summary.passed ?? 0} · falharam ${summary.failed ?? 0} · erros ${summary.errors ?? 0}`,
        node('span', { class: 'hint' }, ` · ${run.ts || ''}`),
      ));
    }
  } catch (_) {
    // histórico é opcional
  }
}

/* ------------------------------------------------------------------------- */
/* Importação e exportação                                                   */
/* ------------------------------------------------------------------------- */

function renderImportParents() {
  const select = $('#importParent');
  const previous = select.value;
  clear(select);
  select.append(option('', 'raiz'));
  for (const folder of allFolders()) select.append(option(folder.id, folder.name));
  if (previous) select.value = previous;
}

function openImport() {
  renderImportParents();
  $('#importResult').hidden = true;
  showDialog($('#importDialog'));
}

async function runImport() {
  const format = $('#importFormat').value;
  const content = $('#importContent').value;
  const path = $('#importPath').value.trim();
  if (!content.trim() && !path) {
    toast('Informe o conteúdo ou um caminho para importar.', 'error');
    return;
  }
  try {
    const report = await api('/api/import', {
      method: 'POST',
      body: {
        format,
        content: content.trim() ? content : null,
        path: path || null,
        parentId: $('#importParent').value || null,
        filename: null,
      },
    });
    const box = $('#importResult');
    box.hidden = false;
    renderReport(box, report, 'Importação');
    await refreshSnapshot();
    if (report.imported_ids?.length) {
      const first = report.imported_ids.find((id) => treeHasRequest(id));
      if (first) await openRequest(first, true);
    }
    toast('Importação concluída.');
  } catch (error) {
    toast(error.message, 'error', 7000);
  }
}

function openExport() {
  const select = $('#exportRequest');
  clear(select);
  select.append(option('', 'requisição aberta'));
  for (const request of allRequests()) {
    select.append(option(request.id, `${request.method} ${request.name}`));
  }
  if (state.current) select.value = state.current.id;
  $('#exportResult').hidden = true;
  $('#exportContent').value = '';
  $('#downloadExportBtn').hidden = true;
  state.exportContent = null;
  showDialog($('#exportDialog'));
}

async function runExport() {
  const format = $('#exportFormat').value;
  const requestId = $('#exportRequest').value || state.current?.id || null;
  try {
    const report = await api('/api/export', {
      method: 'POST',
      body: {
        format,
        requestId,
        destination: $('#exportDestination').value.trim() || null,
      },
    });
    const box = $('#exportResult');
    box.hidden = false;
    renderReport(box, report, 'Exportação');
    state.exportContent = report.content || null;
    $('#exportContent').value = report.content || '';
    $('#downloadExportBtn').hidden = !report.content;
  } catch (error) {
    toast(error.message, 'error', 7000);
  }
}

async function downloadExport() {
  if (!state.exportContent) return;
  try {
    const saved = await window.__TAURI__.core.invoke('save_export', { content: state.exportContent });
    if (saved) toast('Exportação salva.');
  } catch (error) { toast(String(error), 'error'); }
}

async function openCollection() {
  if (state.sendId || state.runId) {
    toast('Conclua ou cancele a execução antes de trocar de coleção.', 'warn');
    return;
  }
  if (!await guardNavigation('Abrir outra coleção com alterações não salvas?')) return;
  try {
    if (await window.__TAURI__.core.invoke('open_collection')) window.location.reload();
  } catch (error) { toast(String(error), 'error', 7000); }
}

function renderReport(box, report, title) {
  clear(box);
  box.append(node('h3', {}, `${title} · ${report.format}`));
  if (report.destination) {
    box.append(node('p', {}, `destino: ${report.destination}`));
  }
  const counts = [];
  if (report.requests_imported !== undefined) counts.push(`requisições ${report.requests_imported}`);
  if (report.folders_imported !== undefined) counts.push(`pastas ${report.folders_imported}`);
  if (report.environments_imported !== undefined) counts.push(`ambientes ${report.environments_imported}`);
  if (report.requests !== undefined) counts.push(`requisições ${report.requests}`);
  if (report.folders !== undefined) counts.push(`pastas ${report.folders}`);
  if (report.files !== undefined) counts.push(`arquivos ${report.files}`);
  box.append(node('p', {}, counts.join(' · ')));
  if (report.original_saved) {
    box.append(node('p', { class: 'hint' }, `original preservado em ${report.original_saved}`));
  }
  if (report.warnings?.length) {
    box.append(node('h3', {}, 'Avisos'), node('ul', {}, ...report.warnings.map((warning) => node('li', {}, warning))));
  }
  if (report.losses?.length) {
    box.append(node('h3', {}, 'Perdas declaradas'), node('ul', {}, ...report.losses.map((loss) => node('li', {}, `${loss.item} · ${loss.field}: ${loss.reason}`))));
  }
}

/* ------------------------------------------------------------------------- */
/* Ambientes                                                                 */
/* ------------------------------------------------------------------------- */

async function openEnvironments() {
  try {
    state.environments = await api('/api/environments');
    state.envRevision = state.snapshot?.manifest_revision || null;
    renderEnvDialog();
    showDialog($('#envDialog'));
  } catch (error) {
    toast(error.message, 'error');
  }
}

function renderEnvDialog() {
  const environments = state.environments?.environments || [];
  const select = $('#envSelect');
  clear(select);
  for (const environment of environments) select.append(option(environment.name, environment.name));
  const preferred = state.envEditing?.name || state.environment || environments[0]?.name || '';
  if (preferred) select.value = preferred;
  editEnvironment(preferred);
}

function editEnvironment(name) {
  const environments = state.environments?.environments || [];
  const existing = environments.find((environment) => environment.name === name);
  const refs = state.environments?.secretRefs || [];
  const variables = (existing?.variables || []).map((variable) => ({
    name: variable.name,
    value: variable.secret ? '' : variableText(variable.value),
    secret: Boolean(variable.secret),
    disabled: Boolean(variable.disabled),
    key: (refs.find((reference) => reference.name === variable.name && reference.environment === name) || {}).key || '',
    available: (refs.find((reference) => reference.name === variable.name && reference.environment === name) || {}).available,
  }));
  state.envEditing = {
    originalName: existing ? existing.name : null,
    name: existing ? existing.name : '',
    extends: existing?.extends || '',
    variables,
    makeDefault: state.snapshot?.default_environment === name,
  };
  renderEnvEditor();
}

function variableText(value) {
  if (value === null || value === undefined) return '';
  if (typeof value === 'string') return value;
  if (typeof value === 'object') {
    if (typeof value.data === 'string') return value.data;
    if (typeof value.value === 'string') return value.value;
  }
  return String(value);
}

function renderEnvEditor() {
  const editing = state.envEditing;
  if (!editing) return;
  const isNew = !editing.originalName;
  $('#envSelect').value = editing.originalName || '';
  $('#envName').value = editing.name || '';
  $('#envName').disabled = !isNew;
  $('#envExtends').value = editing.extends || '';
  $('#envDefault').checked = Boolean(editing.makeDefault);
  const container = $('#envVarsEditor');
  clear(container);
  if (!editing.variables.length) {
    container.append(node('p', { class: 'kv-empty' }, 'Nenhuma variável neste ambiente.'));
  }
  for (const [index, variable] of editing.variables.entries()) {
    container.append(node('div', { class: 'kv-row' },
      node('input', {
        type: 'checkbox',
        checked: !variable.disabled,
        'aria-label': `Ativar variável ${variable.name || index + 1}`,
        onchange: (event) => { variable.disabled = !event.target.checked; },
      }),
      node('input', {
        value: variable.name || '',
        placeholder: 'NOME',
        'aria-label': 'Nome da variável',
        oninput: (event) => { variable.name = event.target.value; },
      }),
      node('input', {
        value: variable.secret ? '' : (variable.value || ''),
        placeholder: variable.secret ? 'valor fornecido pelo processo' : 'valor',
        disabled: variable.secret,
        'aria-label': 'Valor da variável',
        oninput: (event) => { variable.value = event.target.value; },
      }),
      node('label', { class: 'check-row', title: 'Marcar como segredo' },
        node('input', {
          type: 'checkbox',
          checked: variable.secret,
          onchange: (event) => { variable.secret = event.target.checked; renderEnvEditor(); },
        }),
        'segredo'),
      node('button', {
        class: 'kv-del',
        type: 'button',
        'aria-label': `Remover variável ${variable.name || index + 1}`,
        onclick: () => { editing.variables.splice(index, 1); renderEnvEditor(); },
      }, '✕'),
    ));
    if (variable.secret) {
      const reference = node('div', { class: 'kv-row kv-row-simple' },
        node('span', { class: 'muted', 'aria-hidden': 'true' }, '↳'),
        node('input', {
          value: variable.key || '',
          placeholder: 'variável do processo, ex.: REQLIVO_API_TOKEN',
          'aria-label': 'Chave da referência do segredo',
          oninput: (event) => { variable.key = event.target.value; },
        }),
        node('span', { class: variable.available ? 'badge badge-ok' : 'badge badge-secret' }, variable.available ? 'definido' : 'não definido'),
        node('span', { class: 'hint' }, 'provider: env'),
      );
      container.append(reference);
    }
  }
}

function newEnvironment() {
  state.envEditing = {
    originalName: null,
    name: '',
    extends: '',
    variables: [],
    makeDefault: false,
  };
  renderEnvEditor();
  $('#envName').focus();
}

async function saveEnvironment() {
  const editing = state.envEditing;
  if (!editing) return;
  if (!editing.name.trim()) {
    toast('Informe o nome do ambiente.', 'error');
    return;
  }
  if (editing.variables.some((variable) => !variable.name.trim())) {
    toast('Toda variável precisa de nome.', 'error');
    return;
  }
  const secretVariables = editing.variables.filter((variable) => variable.secret);
  for (const variable of secretVariables) {
    if (!variable.key?.trim()) {
      toast(`A variável secreta ${variable.name} precisa de uma chave de referência.`, 'error');
      return;
    }
  }
  const body = {
    name: editing.name.trim(),
    variables: editing.variables.map((variable) => ({
      name: variable.name.trim(),
      value: variable.secret ? '' : (variable.value || ''),
      secret: Boolean(variable.secret),
      disabled: Boolean(variable.disabled),
    })),
    secretRefs: secretVariables.map((variable) => ({
      name: variable.name.trim(),
      key: variable.key.trim(),
      provider: 'env',
    })),
    extends: editing.extends.trim() || null,
    makeDefault: $('#envDefault').checked,
    expectedRevision: state.envRevision,
  };
  try {
    const path = editing.originalName
      ? `/api/environments/${encode(editing.originalName)}`
      : '/api/environments';
    await api(path, { method: editing.originalName ? 'PUT' : 'POST', body });
    state.environment = body.name;
    storeSet(LS.environment, body.name);
    state.environments = await api('/api/environments');
    await refreshSnapshot();
    state.envRevision = state.snapshot?.manifest_revision || null;
    renderEnvDialog();
    toast('Ambiente salvo.');
  } catch (error) {
    if (error.status === 409) {
      showExternalBanner('O manifesto mudou no disco enquanto o ambiente era editado. Seu formulário foi preservado; recarregue e refaça a alteração.', { compare: false, reload: true });
      toast('Conflito de revisão: o ambiente não foi salvo. O formulário foi preservado.', 'error', 9000);
      return;
    }
    toast(error.message, 'error', 7000);
  }
}

async function deleteEnvironment() {
  const editing = state.envEditing;
  if (!editing?.originalName) {
    toast('Selecione um ambiente existente.', 'warn');
    return;
  }
  if (!await confirmAction(`Excluir o ambiente “${editing.originalName}”?`)) return;
  try {
    const revisionQuery = state.envRevision ? `?expectedRevision=${encode(state.envRevision)}` : '';
    await api(`/api/environments/${encode(editing.originalName)}${revisionQuery}`, { method: 'DELETE' });
    if (state.environment === editing.originalName) {
      state.environment = null;
      storeSet(LS.environment, null);
    }
    state.environments = await api('/api/environments');
    await refreshSnapshot();
    state.envRevision = state.snapshot?.manifest_revision || null;
    state.envEditing = null;
    renderEnvDialog();
    toast('Ambiente excluído.');
  } catch (error) {
    toast(error.message, 'error');
  }
}

/* ------------------------------------------------------------------------- */
/* Configurações                                                             */
/* ------------------------------------------------------------------------- */

function openSettings() {
  const settings = currentSettings();
  state.settingsRevision = state.snapshot?.manifest_revision || null;
  $('#setTimeout').value = settings.default_timeout_ms;
  $('#setScriptTimeout').value = settings.script_timeout_ms;
  $('#setRedirects').value = settings.max_redirects;
  $('#setMaxResponse').value = settings.max_response_bytes;
  $('#setFollowRedirects').checked = settings.follow_redirects;
  $('#setOffline').checked = settings.offline_only;
  $('#setAllowProcessEnv').checked = settings.allow_process_env;
  renderEnvironmentSelectors();
  $('#setDefaultEnv').value = state.snapshot?.default_environment || settings.default_environment || '';
  $('#settingsResult').hidden = true;
  showDialog($('#settingsDialog'));
}

async function saveSettings() {
  const settings = {
    ...currentSettings(),
    default_timeout_ms: Number($('#setTimeout').value) || 0,
    script_timeout_ms: Number($('#setScriptTimeout').value) || 2000,
    max_redirects: Number($('#setRedirects').value) || 0,
    max_response_bytes: Number($('#setMaxResponse').value) || 10485760,
    follow_redirects: $('#setFollowRedirects').checked,
    offline_only: $('#setOffline').checked,
    allow_process_env: $('#setAllowProcessEnv').checked,
  };
  try {
    await api('/api/settings', {
      method: 'PUT',
      body: {
        settings,
        defaultEnvironment: $('#setDefaultEnv').value || null,
        expectedRevision: state.settingsRevision,
      },
    });
    await refreshSnapshot();
    state.settingsRevision = state.snapshot?.manifest_revision || null;
    $('#offlineBadge').hidden = !state.health?.offline && !settings.offline_only;
    toast('Configurações salvas.');
  } catch (error) {
    if (error.status === 409) {
      showExternalBanner('O manifesto mudou no disco enquanto as configurações eram editadas. Seus valores foram preservados; recarregue e refaça a alteração.', { compare: false, reload: true });
      toast('Conflito de revisão: as configurações não foram salvas. O formulário foi preservado.', 'error', 9000);
      return;
    }
    toast(error.message, 'error', 7000);
  }
}

/* ------------------------------------------------------------------------- */
/* Diálogos e eventos estáticos                                               */
/* ------------------------------------------------------------------------- */

async function openValidate() {
  const result = $('#validateResult');
  clear(result);
  result.append(node('p', { class: 'muted' }, 'Validando…'));
  showDialog($('#validateDialog'));
  try {
    const data = await api('/api/validate', {
      method: 'POST',
      body: {},
    });
    clear(result);
    result.append(node('h3', {}, data.valid ? 'Coleção válida' : 'Foram encontrados problemas'));
    if (data.issues?.length) {
      result.append(node('h4', {}, 'Problemas'));
      result.append(node('ul', {}, ...data.issues.map((issue) => node('li', {}, issue))));
    }
    if (data.warnings?.length) {
      result.append(node('h4', {}, 'Avisos'));
      result.append(node('ul', {}, ...data.warnings.map((warning) => node('li', {}, warning))));
    }
    if (data.unsupported?.length) {
      result.append(node('h4', {}, 'Não suportado'));
      result.append(node('ul', {}, ...data.unsupported.map((item) => node('li', {}, `${item.path}: ${item.reason}`))));
    }
    if (data.valid && !data.warnings?.length) {
      result.append(node('p', {}, 'Nenhum problema ou aviso encontrado.'));
    }
  } catch (error) {
    clear(result);
    result.append(node('p', { class: 'error-text' }, error.message));
  }
}

function showDialog(dialog) {
  if (!dialog.open) dialog.showModal();
  requestAnimationFrame(() => {
    const focusable = $('input:not([disabled]), select:not([disabled]), textarea:not([disabled]), button', dialog);
    if (focusable) focusable.focus();
  });
}

function activateTab(name) {
  for (const tab of $$('[data-tab]')) {
    const active = tab.dataset.tab === name;
    tab.classList.toggle('is-active', active);
    tab.setAttribute('aria-selected', active ? 'true' : 'false');
  }
  for (const panel of $$('[data-panel]')) {
    const active = panel.dataset.panel === name;
    panel.classList.toggle('is-active', active);
    panel.hidden = !active;
  }
}

function bindStaticEvents() {
  $('#newRequestBtn').addEventListener('click', () => createRequest());
  $('#emptyNewRequest').addEventListener('click', () => createRequest());
  $('#newFolderBtn').addEventListener('click', () => createFolder());
  $('#emptyImport').addEventListener('click', openImport);
  $('#importBtn').addEventListener('click', openImport);
  $('#exportBtn').addEventListener('click', openExport);
  $('#runnerBtn').addEventListener('click', openRunner);
  $('#settingsBtn').addEventListener('click', openSettings);
  $('#validateBtn').addEventListener('click', openValidate);
  $('#environmentsBtn').addEventListener('click', openEnvironments);
  $('#helpBtn').addEventListener('click', () => showDialog($('#helpDialog')));
  $('#reloadBtn').addEventListener('click', async () => {
    if (!await guardNavigation('Recarregar a coleção com alterações não salvas?')) return;
    try {
      await reloadCollection(null);
    } catch (error) {
      toast(error.message, 'error');
    }
  });
  $('#draftsBtn').addEventListener('click', openDrafts);
  $('#trashBtn').addEventListener('click', openTrash);
  $('#migrationsBtn').addEventListener('click', openMigrations);

  $('#externalCompareBtn').addEventListener('click', () => openConflictForCurrent().catch((error) => toast(error.message, 'error')));
  $('#externalReloadBtn').addEventListener('click', async () => {
    if (!await guardNavigation('Recarregar a coleção com alterações não salvas?')) return;
    await reloadCollection(state.watcher.pending?.generation || null);
  });
  $('#externalDismissBtn').addEventListener('click', hideExternalBanner);

  $('#restoreDraftBtn').addEventListener('click', () => {
    if (state.draftDetail) restoreDraft(state.draftDetail);
  });
  $('#compareDraftBtn').addEventListener('click', () => openConflictForCurrent().catch((error) => toast(error.message, 'error')));
  $('#discardDraftBtn').addEventListener('click', discardCurrentDraft);

  $('#navSaveBtn').addEventListener('click', () => resolveNav('save'));
  $('#navKeepBtn').addEventListener('click', () => resolveNav('keep'));
  $('#navDiscardBtn').addEventListener('click', () => resolveNav('discard'));
  $('#navCancelBtn').addEventListener('click', () => resolveNav('cancel'));
  // Escape no guardião equivale a Cancelar; sem isso a navegação ficaria
  // pendente e o editor perderia o vínculo com a decisão.
  $('#navGuardDialog').addEventListener('cancel', (event) => {
    event.preventDefault();
    resolveNav('cancel');
  });

  $('#useDiskBtn').addEventListener('click', () => useDiskVersion().catch((error) => toast(error.message, 'error')));
  $('#keepMineBtn').addEventListener('click', keepLocalVersion);
  $('#applyOverBtn').addEventListener('click', () => applyOverDisk().catch((error) => toast(error.message, 'error')));
  $('#cancelConflictBtn').addEventListener('click', () => {
    state.conflict = null;
    $('#conflictDialog').close();
  });
  $('#conflictDialog').addEventListener('cancel', () => {
    state.conflict = null;
  });

  $('#refreshDraftsBtn').addEventListener('click', async () => {
    await refreshDraftBadge();
    renderDraftsDialog();
  });
  $('#refreshTrashBtn').addEventListener('click', loadTrash);
  $('#restoreTrashBtn').addEventListener('click', restoreSelectedTrash);

  $('#migrationApplyBtn').addEventListener('click', applyPendingMigration);
  $('#migrationRollbackBtn').addEventListener('click', rollbackPendingMigration);
  $('#migrationCancelBtn').addEventListener('click', cancelMigration);
  $('#migrationCloseBtn').addEventListener('click', () => $('#migrationDialog').close());
  $('#migrationInspectBtn').addEventListener('click', openMigrations);

  $('#treeSearch').addEventListener('input', (event) => {
    state.treeFilter = event.target.value;
    renderTree();
  });
  $('#treeSearch').addEventListener('keydown', (event) => {
    if (event.key === 'Escape') {
      event.target.value = '';
      state.treeFilter = '';
      renderTree();
    }
  });
  $('#tree').addEventListener('keydown', handleTreeKeydown);

  $('#environment').addEventListener('change', (event) => {
    state.environment = event.target.value || null;
    storeSet(LS.environment, state.environment);
  });

  $('#saveBtn').addEventListener('click', saveCurrent);
  $('#sendBtn').addEventListener('click', sendCurrent);
  $('#cancelSendBtn').addEventListener('click', cancelSend);
  $('#deleteRequestBtn').addEventListener('click', () => {
    if (state.current) deleteRequest(state.current.id);
  });

  for (const tab of $$('[data-tab]')) {
    tab.addEventListener('click', () => activateTab(tab.dataset.tab));
  }
  for (const tab of $$('[data-response-tab]')) {
    tab.addEventListener('click', () => {
      state.responseTab = tab.dataset.responseTab;
      renderResponseTabs();
    });
  }

  $('#requestName').addEventListener('input', (event) => {
    state.current.request.info.name = event.target.value;
    renderPath();
    markDirty();
  });
  $('#method').addEventListener('change', (event) => {
    state.current.request.http.method = event.target.value;
    markDirty();
  });
  $('#url').addEventListener('input', (event) => {
    state.current.request.http.url = event.target.value;
    markDirty();
  });
  $('#tagsInput').addEventListener('input', (event) => {
    state.current.request.info.tags = splitComma(event.target.value);
    markDirty();
  });
  $('#docsInput').addEventListener('input', (event) => {
    state.current.request.docs = event.target.value || null;
    markDirty();
  });

  $('#addParamBtn').addEventListener('click', () => {
    state.current.request.http.params.push({ name: '', value: '', type: 'query', disabled: false });
    markDirty();
    renderParams();
  });
  $('#addHeaderBtn').addEventListener('click', () => {
    state.current.request.http.headers.push({ name: '', value: '', disabled: false });
    markDirty();
    renderHeaders();
  });
  $('#bodyType').addEventListener('change', (event) => changeBodyType(event.target.value));
  $('#bodyData').addEventListener('input', (event) => {
    const body = state.current.request.http.body;
    if (body) body.data = event.target.value;
    markDirty();
  });
  $('#formatJsonBtn').addEventListener('click', () => {
    const body = state.current.request.http.body;
    if (!body) return;
    try {
      body.data = JSON.stringify(JSON.parse(body.data || '{}'), null, 2);
      $('#bodyData').value = body.data;
      markDirty();
      toast('JSON formatado.');
    } catch (_) {
      toast('O JSON ainda é inválido.', 'error');
    }
  });
  $('#addScriptBtn').addEventListener('click', () => {
    state.current.request.runtime.scripts.push({ type: 'before-request', code: '' });
    markDirty();
    renderScripts();
  });
  $('#addAssertionBtn').addEventListener('click', () => {
    state.current.request.runtime.assertions.push({
      expression: 'res.status', operator: 'eq', value: '200', disabled: false,
    });
    markDirty();
    renderAssertions();
  });

  $('#startRunBtn').addEventListener('click', startRun);
  $('#cancelRunBtn').addEventListener('click', cancelRun);
  $('#loadHistoryBtn').addEventListener('click', loadHistory);

  $('#importFile').addEventListener('change', async (event) => {
    const file = event.target.files?.[0];
    if (!file) return;
    $('#importContent').value = await file.text();
    if (file.name.endsWith('.yml') || file.name.endsWith('.yaml')) $('#importFormat').value = 'opencollection';
    else if (file.name.endsWith('.json')) $('#importFormat').value = 'postman';
  });
  $('#doImportBtn').addEventListener('click', runImport);
  $('#doExportBtn').addEventListener('click', runExport);
  $('#downloadExportBtn').addEventListener('click', downloadExport);

  $('#envSelect').addEventListener('change', (event) => editEnvironment(event.target.value));
  $('#addEnvVarBtn').addEventListener('click', () => {
    if (!state.envEditing) return;
    const index = state.envEditing.variables.length + 1;
    state.envEditing.variables.push({
      name: `NOVA_VARIAVEL_${index}`,
      value: '',
      secret: false,
      disabled: false,
      key: '',
    });
    renderEnvEditor();
  });
  $('#saveEnvBtn').addEventListener('click', saveEnvironment);
  $('#newEnvBtn').addEventListener('click', newEnvironment);
  $('#deleteEnvBtn').addEventListener('click', deleteEnvironment);
  $('#envName').addEventListener('input', (event) => { if (state.envEditing) state.envEditing.name = event.target.value; });
  $('#envExtends').addEventListener('input', (event) => { if (state.envEditing) state.envEditing.extends = event.target.value; });
  $('#envDefault').addEventListener('change', (event) => { if (state.envEditing) state.envEditing.makeDefault = event.target.checked; });

  $('#saveSettingsBtn').addEventListener('click', saveSettings);

  document.addEventListener('keydown', (event) => {
    const target = event.target;
    const typing = target instanceof HTMLInputElement || target instanceof HTMLTextAreaElement || target instanceof HTMLSelectElement || target.isContentEditable;
    const command = event.ctrlKey || event.metaKey;
    if (command && event.key.toLowerCase() === 's') {
      event.preventDefault();
      if (state.current) saveCurrent();
    } else if (command && event.key === 'Enter') {
      event.preventDefault();
      if (state.current) sendCurrent();
    } else if (command && event.key.toLowerCase() === 'k') {
      event.preventDefault();
      $('#treeSearch').focus();
    } else if (event.altKey && event.key.toLowerCase() === 'n') {
      event.preventDefault();
      if (state.current || !state.dirty) createRequest();
    } else if (event.altKey && event.key.toLowerCase() === 'f') {
      event.preventDefault();
      createFolder();
    } else if (event.altKey && event.key.toLowerCase() === 'r') {
      event.preventDefault();
      openRunner();
    } else if (!typing && event.key === '?') {
      event.preventDefault();
      showDialog($('#helpDialog'));
    }
  });



  // Acesso adicional às configurações a partir do diálogo de ambientes.
  const envDialogBody = $('#envDialog .dialog-body');
  if (envDialogBody && !$('#openCollectionSettings')) {
    envDialogBody.append(node('div', { class: 'dialog-actions' },
      node('button', {
        type: 'button',
        id: 'openCollectionSettings',
        class: 'btn',
        onclick: () => {
          $('#envDialog').close();
          openSettings();
        },
      }, 'Configurações da coleção'),
    ));
  }
}

/* Mantém a função de importação disponível no escopo global para depuração. */
window.Reqlivo = { state, api, refreshSnapshot };
