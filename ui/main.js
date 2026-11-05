// Moonshine 会议助手 — 前端逻辑（Tauri 2）

const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

// ============================================================
// STATE
// ============================================================
const S = {
  isRunning: false,
  micReady: false,
  loopFound: false,
  micCount: 0,
  loopCount: 0,
  chars: 0,
  entries: [],
  startTs: null,
  timer: null,
  volumeHistory: new Array(40).fill(0),
  waveformAnimFrame: null,
};

// ============================================================
// DOM REFS
// ============================================================
const $ = (id) => document.getElementById(id);
const els = {
  engineSelect: $('engineSelect'),
  modelInput: $('modelInput'),
  langInput: $('langInput'),
  engineStatus: $('engineStatus'),
  micSelect: $('micSelect'),
  loopSelect: $('loopSelect'),
  micStatus: $('micStatus'),
  loopStatus: $('loopStatus'),
  btnRefresh: $('btnRefresh'),
  vadToggle: $('vadToggle'),
  dualToggle: $('dualToggle'),
  btnRecord: $('btnRecord'),
  btnExport: $('btnExport'),
  btnClear: $('btnClear'),
  btnSummarize: $('btnSummarize'),
  tlog: $('transcriptLog'),
  emptyState: $('emptyState'),
  envAlert: $('envAlert'),
  statusPill: $('statusPill'),
  statusText: $('statusText'),
  logPanel: $('logPanel'),
  logContent: $('logContent'),
  panelToggleBtn: $('panelToggleBtn'),
  aiPanel: $('aiPanel'),
  waveformCanvas: $('waveformCanvas'),
  waveformIdle: $('waveformIdle'),
  vadLabel: $('vadLabel'),
  vadText: $('vadText'),
  waveformLabel: $('waveformLabel'),
  summaryCard: $('summaryCard'),
  versionTag: $('versionTag'),
  btnAsrSettings: $('btnAsrSettings'),
  asrModal: $('asrModal'),
  asrProviderList: $('asrProviderList'),
  asrFields: $('asrFields'),
  asrTestResult: $('asrTestResult'),
  btnAsrTest: $('btnAsrTest'),
  btnAsrSave: $('btnAsrSave'),
  asrModalClose: $('asrModalClose'),
};

// ============================================================
// WAVEFORM
// ============================================================
function setupCanvas() {
  const c = els.waveformCanvas;
  if (!c) return;
  const dpr = window.devicePixelRatio || 1;
  const rect = c.getBoundingClientRect();
  c.width = Math.max(1, rect.width * dpr);
  c.height = Math.max(1, rect.height * dpr);
  const ctx = c.getContext('2d');
  ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
}

function drawWaveform(volume) {
  const c = els.waveformCanvas;
  if (!c) return;
  const ctx = c.getContext('2d');
  const rect = c.getBoundingClientRect();
  const W = rect.width, H = rect.height;
  S.volumeHistory.push(Math.min(1, Math.max(0, volume)));
  S.volumeHistory.shift();
  ctx.clearRect(0, 0, W, H);
  const barW = W / S.volumeHistory.length - 1;
  S.volumeHistory.forEach((v, i) => {
    const barH = Math.max(2, v * H * 0.9);
    const x = i * (barW + 1);
    const y = (H - barH) / 2;
    const alpha = 0.4 + v * 0.6;
    ctx.fillStyle = S.isRunning
      ? `rgba(124,106,245,${alpha})`
      : `rgba(86,87,112,${alpha * 0.5})`;
    ctx.beginPath();
    if (ctx.roundRect) ctx.roundRect(x, y, barW, barH, 2); else ctx.rect(x, y, barW, barH);
    ctx.fill();
  });
}

function stopWaveform() {
  if (S.waveformAnimFrame) { cancelAnimationFrame(S.waveformAnimFrame); S.waveformAnimFrame = null; }
  S.volumeHistory.fill(0);
  const c = els.waveformCanvas;
  if (c) c.getContext('2d').clearRect(0, 0, c.getBoundingClientRect().width, c.getBoundingClientRect().height);
}

// ============================================================
// DEVICE LOADING
// ============================================================
async function loadDevices() {
  setStatusPill('loading', '加载设备…');
  els.btnRefresh.disabled = true;
  els.micSelect.innerHTML = '<option>加载中…</option>'; els.micSelect.disabled = true;
  els.loopSelect.innerHTML = '<option>加载中…</option>'; els.loopSelect.disabled = true;

  let devs;
  try {
    devs = await invoke('list_devices');
  } catch (e) {
    devs = { error: String(e) };
  }
  els.btnRefresh.disabled = false;

  if (devs.error || !Array.isArray(devs)) {
    const msg = devs.error || '无法枚举设备';
    setDeviceStatus(els.micStatus, 'err', msg);
    showAlert('⚠️ ' + msg + '（请确认系统音频权限）');
    setStatusPill('error', '设备加载失败');
    addLog('[ERROR] ' + msg, 'err');
    return;
  }

  els.envAlert.classList.remove('visible');
  const inputs = devs.filter((d) => d.maxInputChannels > 0);

  if (inputs.length === 0) {
    showAlert('⚠️ 未检测到任何音频输入设备，请检查麦克风连接或系统权限');
    setStatusPill('error', '无设备');
    return;
  }

  els.micSelect.innerHTML = '';
  els.loopSelect.innerHTML = '';
  S.loopFound = false;
  let micDevice = null;

  inputs.forEach((d) => {
    const label = `${d.index}: ${d.name}`;
    const optMic = new Option(label, d.index);
    const optLoop = new Option(label, d.index);
    els.micSelect.add(optMic);
    if (d.isLoopback && !S.loopFound) {
      optLoop.text = label + ' ⭐';
      els.loopSelect.add(optLoop, 0);
      S.loopFound = true;
      setDeviceStatus(els.loopStatus, 'ok', `检测到内录设备 ${d.name}`);
    } else {
      els.loopSelect.add(optLoop);
    }
    if (!micDevice && !d.isLoopback) micDevice = d;
  });

  if (micDevice) {
    els.micSelect.disabled = false;
    S.micReady = true;
    setDeviceStatus(els.micStatus, 'ok', `就绪: ${micDevice.name}`);
  }

  if (S.loopFound) {
    els.loopSelect.disabled = false;
  } else {
    els.loopSelect.disabled = false;
    setDeviceStatus(els.loopStatus, 'warn', '未检测到 Loopback，内录不可用');
    els.loopSelect.value = '';
    $('loopGuide').classList.add('visible');
    $('guideToggle').classList.add('visible');
  }

  els.btnRecord.disabled = !S.micReady;
  setStatusPill(S.micReady ? 'idle' : 'warn', S.micReady ? '就绪' : '请选择设备');
}

// ============================================================
// HELPERS
// ============================================================
function setStatusPill(type, text) {
  els.statusPill.className = 'status-pill ' + type;
  els.statusText.textContent = text;
}
function setDeviceStatus(el, state, text) {
  el.className = 'device-status ' + state;
  el.innerHTML = state ? `<div class="dot"></div>${text}` : text;
}
function showAlert(msg) {
  els.envAlert.textContent = msg;
  els.envAlert.classList.add('visible');
}
function addLog(txt, type) {
  els.logPanel.classList.add('visible');
  const div = document.createElement('div');
  div.className = type ? `log-${type}` : '';
  div.textContent = '[' + new Date().toLocaleTimeString() + '] ' + txt;
  els.logContent.appendChild(div);
  els.logContent.scrollTop = els.logContent.scrollHeight;
}
function escHtml(s) {
  return String(s ?? '').replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
}

// ============================================================
// TRANSCRIPT
// ============================================================
function addTranscriptEntry(msg) {
  if (els.emptyState) els.emptyState.style.display = 'none';
  const src = msg.source || 'mic';
  const icon = src === 'mic' ? '🎤' : src === 'loopback' ? '🔊' : '💬';
  const color = src === 'mic' ? 'mic' : src === 'loopback' ? 'loop' : 'sys';
  const label = msg.speaker || (src === 'mic' ? '麦克风' : src === 'loopback' ? '系统内录' : '混合');
  const d = new Date();
  const timeStr = [d.getHours(), d.getMinutes(), d.getSeconds()].map((x) => String(x).padStart(2, '0')).join(':');

  S.entries.push({ source: src, text: msg.text, ts: msg.ts || Date.now() });
  if (src === 'mic') S.micCount++; else if (src === 'loopback') S.loopCount++;
  S.chars += (msg.text || '').length;

  const el = document.createElement('div');
  el.className = 't-card latest';
  el.innerHTML = `
    <div class="t-icon ${color}">${icon}</div>
    <div class="t-body">
      <div class="t-meta">
        <span class="t-src ${color}">${label}</span>
        <span class="t-time">${timeStr}</span>
      </div>
      <div class="t-text">${escHtml(msg.text)}</div>
    </div>`;

  const prev = els.tlog.querySelector('.t-card.latest:not(:last-child)');
  if (prev) prev.classList.remove('latest');
  els.tlog.appendChild(el);
  els.tlog.scrollTop = els.tlog.scrollHeight;
  updateStats();
  updateAIPanel();
}

function updateStats() {
  $('totalBadge').textContent = S.entries.length + ' 条';
  $('statMic').textContent = S.micCount;
  $('statLoop').textContent = S.loopCount;
  $('statChars').textContent = S.chars;
}

function updateAIPanel() {
  const total = S.micCount + S.loopCount;
  if (total > 0) {
    const micPct = Math.round((S.micCount / total) * 100);
    $('ratioMic').style.width = micPct + '%';
    $('ratioLoop').style.width = (100 - micPct) + '%';
    $('ratioMicPct').textContent = micPct + '%';
    $('ratioLoopPct').textContent = (100 - micPct) + '%';
  }
  if (S.startTs) {
    const elapsedMin = (Date.now() - S.startTs) / 60000;
    if (elapsedMin > 0.1) {
      const wpm = Math.round(S.chars / elapsedMin);
      $('speedVal').textContent = wpm;
      $('speedFill').style.width = Math.min(100, wpm / 5) + '%';
    }
  }
}

// ============================================================
// RECORDING
// ============================================================
async function startRecording() {
  if (S.isRunning) return;
  if (!S.micReady && !S.loopFound) { addLog('[ERROR] 请先选择音频设备', 'err'); return; }

  const config = {
    micDeviceIndex: parseInt(els.micSelect.value) || -1,
    loopbackDeviceIndex: els.dualToggle.checked ? (parseInt(els.loopSelect.value) || -1) : -1,
    engine: els.engineSelect.value,
    model: els.modelInput.value.trim() || 'large-v3-turbo',
    language: els.langInput.value.trim() || 'zh',
    vadEnabled: els.vadToggle.checked,
    dualEnabled: els.dualToggle.checked,
  };

  addLog('启动: ' + config.engine + ' @ ' + config.model, 'info');
  try {
    await invoke('start_transcription', { config });
  } catch (e) {
    addLog('[ERROR] 启动失败: ' + e, 'err');
    showAlert('⚠️ 启动失败: ' + e);
    return;
  }
  _startUI();
}

async function stopRecording() {
  if (!S.isRunning) return;
  try { await invoke('stop_transcription'); } catch (e) { addLog('[ERROR] ' + e, 'err'); }
  addLog('已请求停止…', 'info');
}

function _startUI() {
  S.isRunning = true;
  S.startTs = Date.now();
  S.micCount = 0; S.loopCount = 0; S.chars = 0;
  S.entries = [];

  els.btnRecord.className = 'record-btn stop';
  $('recordIcon').textContent = '⏹';
  $('recordLabel').textContent = '停止录制';
  els.btnExport.disabled = true;
  els.btnSummarize.disabled = true;
  els.micSelect.disabled = true;
  els.loopSelect.disabled = true;
  els.btnRefresh.disabled = true;
  els.engineSelect.disabled = true;
  els.modelInput.disabled = true;
  els.langInput.disabled = true;
  els.vadToggle.disabled = true;
  els.dualToggle.disabled = true;

  setStatusPill('running', '录制中');
  els.waveformIdle.classList.add('hidden');
  els.waveformLabel.textContent = '● 录制中';

  S.timer = setInterval(() => {
    const e = Date.now() - S.startTs;
    const ts = [Math.floor(e / 3600000), Math.floor((e % 3600000) / 60000), Math.floor((e % 60000) / 1000)]
      .map((x) => String(x).padStart(2, '0')).join(':');
    $('totalTime').textContent = ts;
    $('statDuration').textContent = ts.slice(3);
    $('aiDuration').textContent = ts;
  }, 1000);
}

function _stopUI() {
  S.isRunning = false;
  clearInterval(S.timer);
  stopWaveform();

  els.btnRecord.className = 'record-btn start';
  $('recordIcon').textContent = '⏺';
  $('recordLabel').textContent = '开始录制';
  els.btnRecord.disabled = !S.micReady;
  els.btnExport.disabled = S.entries.length === 0;
  els.btnSummarize.disabled = S.entries.length === 0;
  els.micSelect.disabled = false;
  els.loopSelect.disabled = false;
  els.btnRefresh.disabled = false;
  els.engineSelect.disabled = false;
  els.modelInput.disabled = false;
  els.langInput.disabled = false;
  els.vadToggle.disabled = false;
  els.dualToggle.disabled = false;

  setStatusPill(S.micReady ? 'idle' : 'error', S.micReady ? '就绪' : '设备异常');
  els.waveformIdle.classList.remove('hidden');
  els.waveformLabel.textContent = '待机';
  els.vadLabel.classList.remove('speaking');
  els.vadText.textContent = '等待输入';
}

function clearTranscript() {
  if (S.isRunning) return;
  S.entries = []; S.micCount = 0; S.loopCount = 0; S.chars = 0;
  els.tlog.querySelectorAll('.t-card').forEach((el) => el.remove());
  if (els.emptyState) els.emptyState.style.display = '';
  updateStats();
  $('totalBadge').textContent = '0 条';
  $('totalTime').textContent = '00:00:00';
  $('statDuration').textContent = '00:00';
  $('aiDuration').textContent = '00:00:00';
  $('speedVal').textContent = '0';
  $('speedFill').style.width = '0%';
  $('ratioMic').style.width = '50%';
  $('ratioLoop').style.width = '50%';
  els.summaryCard.innerHTML = '<span class="placeholder">停止录制后点击「生成摘要」…</span>';
}

// ============================================================
// EXPORT
// ============================================================
async function exportTranscript() {
  if (S.entries.length === 0) { addLog('[WARN] 暂无内容可导出', 'warn'); return; }
  const fmt = $('exportFormat').value;
  let content;
  try {
    content = await invoke('export_session', { format: fmt });
  } catch (e) {
    addLog('[ERROR] 导出失败: ' + e, 'err');
    return;
  }
  const d = new Date();
  const ts = `${d.getFullYear()}${String(d.getMonth() + 1).padStart(2, '0')}${String(d.getDate()).padStart(2, '0')}`;
  try {
    const path = await invoke('save_transcript', {
      request: { filename: `会议记录_${ts}.${fmt}`, content },
    });
    if (path) addLog('已导出 → ' + path, 'info');
  } catch (e) {
    addLog('[ERROR] 保存失败: ' + e, 'err');
  }
}

// ============================================================
// SUMMARY
// ============================================================
async function generateSummary() {
  els.btnSummarize.disabled = true;
  els.summaryCard.innerHTML = '<span class="placeholder">摘要生成中…（默认调用本地 Ollama）</span>';
  try {
    const text = await invoke('summarize', {
      config: {
        endpoint: 'http://localhost:11434/v1/chat/completions',
        model: 'qwen2.5:7b',
        api_key: null,
      },
    });
    els.summaryCard.textContent = text;
  } catch (e) {
    els.summaryCard.innerHTML = '<span class="placeholder">摘要失败：' + escHtml(e) + '（请确认已启动本地 Ollama 或配置端点）</span>';
    addLog('[ERROR] 摘要失败: ' + e, 'err');
  }
  els.btnSummarize.disabled = false;
}

// ============================================================
// CLOUD ASR SETTINGS
// ============================================================
let ASR_PROVIDERS = [];
let ASR_SETTINGS = { volcengine: null, tencent: null, alicloud: null };
let ASR_SELECTED_PROVIDER = 'volcengine';
let ASR_MODELS = {};

async function loadAsrProviders() {
  try {
    ASR_PROVIDERS = await invoke('get_asr_providers');
  } catch (e) {
    ASR_PROVIDERS = [];
    addLog('[ERROR] 加载 ASR 服务商失败: ' + e, 'err');
  }
}

async function loadAsrSettings() {
  try {
    ASR_SETTINGS = (await invoke('load_asr_settings')) || {};
  } catch (e) {
    ASR_SETTINGS = {};
  }
  try {
    ASR_MODELS = JSON.parse(localStorage.getItem('asr_models') || '{}');
  } catch (e) {
    ASR_MODELS = {};
  }
}

function openAsrSettings() {
  if (!ASR_PROVIDERS.length) return;
  if (!ASR_PROVIDERS.find((p) => p.id === ASR_SELECTED_PROVIDER)) {
    ASR_SELECTED_PROVIDER = ASR_PROVIDERS[0].id;
  }
  els.asrTestResult.textContent = '';
  els.asrTestResult.className = 'asr-test-result';
  renderAsrProviders();
  renderAsrFields(ASR_SELECTED_PROVIDER);
  els.asrModal.style.display = 'flex';
}

function closeAsrSettings() {
  els.asrModal.style.display = 'none';
}

function renderAsrProviders() {
  els.asrProviderList.innerHTML = ASR_PROVIDERS.map((p) => `
    <label class="asr-provider-option ${ASR_SELECTED_PROVIDER === p.id ? 'selected' : ''}">
      <input type="radio" name="asr-provider" value="${p.id}" ${ASR_SELECTED_PROVIDER === p.id ? 'checked' : ''}/>
      <span>${p.name}</span>
    </label>
  `).join('');
  els.asrProviderList.querySelectorAll('input[name="asr-provider"]').forEach((r) => {
    r.addEventListener('change', () => {
      ASR_SELECTED_PROVIDER = r.value;
      els.asrTestResult.textContent = '';
      els.asrTestResult.className = 'asr-test-result';
      renderAsrProviders();
      renderAsrFields(ASR_SELECTED_PROVIDER);
    });
  });
}

function renderAsrFields(providerId) {
  const p = ASR_PROVIDERS.find((x) => x.id === providerId);
  if (!p) { els.asrFields.innerHTML = ''; return; }
  const creds = (ASR_SETTINGS && ASR_SETTINGS[providerId]) || {};
  const fields = p.keyFields.map((f) => `
    <label class="asr-field">
      <span class="asr-field-label">${f.label}</span>
      <input type="${f.type || 'text'}" data-cred-key="${f.key}" placeholder="${f.placeholder || ''}" value="${escHtml(creds[f.key] || '')}" autocomplete="off"/>
    </label>
  `).join('');
  const model = ASR_MODELS[providerId] || p.models[0].id;
  const models = p.models.map((m) => `<option value="${m.id}" ${model === m.id ? 'selected' : ''}>${m.name}</option>`).join('');
  els.asrFields.innerHTML = `
    <h4 class="asr-section-title">${p.name} 配置</h4>
    ${fields}
    <label class="asr-field">
      <span class="asr-field-label">识别模型</span>
      <select data-asr-model>${models}</select>
    </label>
    <p class="asr-help">💡 在 <a href="${p.console}" target="_blank" rel="noopener">${p.name}控制台</a> 开通语音识别并创建应用获取 Key</p>
  `;
}

function gatherAsrCreds(providerId) {
  const p = ASR_PROVIDERS.find((x) => x.id === providerId);
  if (!p) return null;
  const creds = {};
  els.asrFields.querySelectorAll('[data-cred-key]').forEach((inp) => {
    creds[inp.dataset.credKey] = inp.value.trim();
  });
  return creds;
}

async function persistAsrSettings() {
  const creds = gatherAsrCreds(ASR_SELECTED_PROVIDER);
  if (!creds) return;
  const merged = {
    volcengine: (ASR_SETTINGS && ASR_SETTINGS.volcengine) || null,
    tencent: (ASR_SETTINGS && ASR_SETTINGS.tencent) || null,
    alicloud: (ASR_SETTINGS && ASR_SETTINGS.alicloud) || null,
  };
  merged[ASR_SELECTED_PROVIDER] = creds;
  await invoke('save_asr_settings', { settings: merged });
  ASR_SETTINGS = merged;

  const modelSel = els.asrFields.querySelector('[data-asr-model]');
  if (modelSel) ASR_MODELS[ASR_SELECTED_PROVIDER] = modelSel.value;
  localStorage.setItem('asr_models', JSON.stringify(ASR_MODELS));

  if (els.engineSelect.value === ASR_SELECTED_PROVIDER) {
    els.modelInput.value = ASR_MODELS[ASR_SELECTED_PROVIDER];
  }
}

async function saveAsrSettings() {
  try {
    await persistAsrSettings();
    els.asrTestResult.textContent = '✓ 已保存';
    els.asrTestResult.className = 'asr-test-result ok';
    addLog('云 ASR 凭证已保存（' + ASR_SELECTED_PROVIDER + '）', 'info');
  } catch (e) {
    els.asrTestResult.textContent = '✗ 保存失败: ' + e;
    els.asrTestResult.className = 'asr-test-result err';
  }
}

async function testAsrConnection() {
  els.asrTestResult.textContent = '连接测试中…';
  els.asrTestResult.className = 'asr-test-result';
  els.btnAsrTest.disabled = true;
  try {
    await persistAsrSettings();
    const def = ASR_PROVIDERS.find((p) => p.id === ASR_SELECTED_PROVIDER)?.models[0].id;
    const model = ASR_MODELS[ASR_SELECTED_PROVIDER] || def || '';
    await invoke('test_asr_connection', { engine: ASR_SELECTED_PROVIDER, model });
    els.asrTestResult.textContent = '✓ 连接成功，凭证有效';
    els.asrTestResult.className = 'asr-test-result ok';
  } catch (e) {
    els.asrTestResult.textContent = '✗ ' + e;
    els.asrTestResult.className = 'asr-test-result err';
  }
  els.btnAsrTest.disabled = false;
}

// ============================================================
// ENGINE MODEL DEFAULTS
// ============================================================
const ENGINE_DEFAULTS = {
  faster_whisper: { model: 'large-v3-turbo', lang: 'zh', remote: false },
  moonshine: { model: 'small_streaming', lang: 'en', remote: false },
  funasr: { model: 'paraformer-zh', lang: 'zh', remote: false },
  volcengine: { model: 'bigmodel', lang: 'zh-CN', remote: true },
  tencent: { model: '16k_zh_en_2.0', lang: 'zh-CN', remote: true },
  alicloud: { model: 'domain-meeting-domain', lang: 'zh-CN', remote: true },
};
els.engineSelect.addEventListener('change', () => {
  const d = ENGINE_DEFAULTS[els.engineSelect.value] || ENGINE_DEFAULTS.faster_whisper;
  els.modelInput.value = ASR_MODELS[els.engineSelect.value] || d.model;
  els.langInput.value = d.lang;
  els.btnAsrSettings.style.display = d.remote ? '' : 'none';
  if (d.remote) {
    setDeviceStatus(els.engineStatus, 'warn', '云端引擎：请先配置 API Key');
  } else {
    setDeviceStatus(els.engineStatus, 'ok', '引擎已切换，模型加载将在首次录制时进行');
  }
});

// ============================================================
// EVENT BINDINGS
// ============================================================
els.btnRecord.addEventListener('click', () => (S.isRunning ? stopRecording() : startRecording()));
els.btnExport.addEventListener('click', exportTranscript);
els.btnClear.addEventListener('click', clearTranscript);
els.btnRefresh.addEventListener('click', () => { if (!S.isRunning) loadDevices(); });
els.btnSummarize.addEventListener('click', generateSummary);
$('guideToggle').addEventListener('click', () => $('loopGuide').classList.toggle('visible'));

els.btnAsrSettings.addEventListener('click', openAsrSettings);
els.btnAsrTest.addEventListener('click', testAsrConnection);
els.btnAsrSave.addEventListener('click', saveAsrSettings);
els.asrModalClose.addEventListener('click', closeAsrSettings);
els.asrModal.addEventListener('click', (e) => { if (e.target === els.asrModal) closeAsrSettings(); });

let panelOpen = true;
els.panelToggleBtn.addEventListener('click', () => {
  panelOpen = !panelOpen;
  els.aiPanel.classList.toggle('collapsed', !panelOpen);
  els.panelToggleBtn.textContent = panelOpen ? '◀' : '▶';
  setTimeout(setupCanvas, 300);
});

document.addEventListener('keydown', (e) => {
  const isMac = navigator.platform.toUpperCase().includes('MAC');
  const cmd = isMac ? e.metaKey : e.ctrlKey;
  if (cmd && e.key === 'r') { e.preventDefault(); S.isRunning ? stopRecording() : startRecording(); }
  if (cmd && e.key === 's') { e.preventDefault(); if (S.isRunning) stopRecording(); }
  if (cmd && e.key === 'e') { e.preventDefault(); exportTranscript(); }
  if (e.key === 'Escape' && S.isRunning) stopRecording();
});

// ============================================================
// TAURI EVENTS
// ============================================================
async function bindEvents() {
  await listen('transcript', (event) => {
    const msg = event.payload;
    if (msg && msg.text) addTranscriptEntry(msg);
  });
  await listen('volume', (event) => {
    const v = event.payload && event.payload.volume;
    if (typeof v === 'number') {
      drawWaveform(v * 3);
      const speaking = v > 0.015;
      els.vadLabel.className = 'vad-label' + (speaking ? ' speaking' : '');
      els.vadText.textContent = speaking ? '正在说话' : '等待输入';
    }
  });
  await listen('log', (event) => {
    const txt = String(event.payload);
    addLog(txt, txt.startsWith('[ERROR]') ? 'err' : txt.startsWith('[WARN]') ? 'warn' : '');
  });
  await listen('status', (event) => {
    if (event.payload === 'stopped') {
      _stopUI();
      addLog('转写进程已停止', 'info');
    } else if (event.payload === 'running') {
      // 已由 _startUI 处理
    }
  });
}

// ============================================================
// BOOT
// ============================================================
window.addEventListener('load', async () => {
  setupCanvas();
  try {
    const v = await invoke('get_version');
    els.versionTag.textContent = 'v' + v;
  } catch (e) { /* ignore */ }
  await bindEvents();
  await loadAsrProviders();
  await loadAsrSettings();
  await loadDevices();
});
window.addEventListener('resize', setupCanvas);
