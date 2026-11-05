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
  devices: [],
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
  asrGuideLink: $('asrGuideLink'),
  btnHelp: $('btnHelp'),
  btnHelpSide: $('btnHelpSide'),
  helpModal: $('helpModal'),
  helpTabs: $('helpTabs'),
  helpBody: $('helpBody'),
  helpHint: $('helpHint'),
  btnHelpClose: $('btnHelpClose'),
  btnHelpDoc: $('btnHelpDoc'),
  helpModalClose: $('helpModalClose'),
  btnAudioDiag: $('btnAudioDiag'),
  diagModal: $('diagModal'),
  diagModalClose: $('diagModalClose'),
  diagBody: $('diagBody'),
  diagResult: $('diagResult'),
  btnDiagRun: $('btnDiagRun'),
  btnDiagClose: $('btnDiagClose'),
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
  // 后端 AudioDevice 现输出 camelCase；兼容旧字段，且不再误过滤掉设备
  const inputs = devs.filter((d) => (d.channels ?? d.maxInputChannels ?? 1) > 0);
  S.devices = inputs;

  if (inputs.length === 0) {
    showAlert('⚠️ 未检测到任何音频输入设备，请检查麦克风连接或系统权限');
    setStatusPill('error', '无设备');
    return;
  }

  const isStrong = (d) => d.isLoopback === true;
  const isWeak = (d) => !isStrong(d) && d.loopbackHint === true;

  // 内录下拉：列出【所有】输入设备。关键词识别可能漏判 WASAPI 回环 / PulseAudio
  // monitor 设备，允许手动选任意设备作为内录源，避免「未检测到 Loopback」直接不可用。
  const loopOrder = [...inputs].sort(
    (a, b) => (isStrong(a) ? 0 : isWeak(a) ? 1 : 2) - (isStrong(b) ? 0 : isWeak(b) ? 1 : 2)
      || a.index - b.index
  );
  els.loopSelect.innerHTML = '';
  loopOrder.forEach((d) => {
    const tag = isStrong(d) ? ' ⭐内录' : isWeak(d) ? ' 🔶疑似内录' : '';
    els.loopSelect.add(new Option(`${d.index}: ${d.name}${tag}`, d.index));
  });

  // 麦克风下拉：非内录设备优先
  const micOrder = [...inputs].sort(
    (a, b) => (isStrong(a) ? 1 : 0) - (isStrong(b) ? 1 : 0) || a.index - b.index
  );
  els.micSelect.innerHTML = '';
  micOrder.forEach((d) => els.micSelect.add(new Option(`${d.index}: ${d.name}`, d.index)));

  const strong = inputs.filter(isStrong);
  const weak = inputs.filter(isWeak);
  S.loopFound = strong.length > 0 || weak.length > 0;

  if (strong.length) {
    els.loopSelect.value = String(strong[0].index);
    setDeviceStatus(els.loopStatus, 'ok', `检测到内录设备 ${strong[0].name}`);
  } else if (weak.length) {
    els.loopSelect.value = String(weak[0].index);
    setDeviceStatus(els.loopStatus, 'warn', `未识别到标准内录，已预选「${weak[0].name}」，建议先跑音频诊断`);
  } else {
    els.loopSelect.value = String(loopOrder[0].index);
    setDeviceStatus(els.loopStatus, 'warn', '未发现内录设备，可手动选任意输入设备并用音频诊断确认');
  }

  const micDevice = inputs.find((d) => !isStrong(d)) || inputs[0];
  els.micSelect.value = String(micDevice.index);
  els.micSelect.disabled = false;
  els.loopSelect.disabled = false;
  S.micReady = true;
  setDeviceStatus(els.micStatus, 'ok', `就绪: ${micDevice.name}`);

  els.btnRecord.disabled = false;
  setStatusPill('idle', '就绪');
  if (!strong.length) {
    $('loopGuide').classList.add('visible');
    $('guideToggle').classList.add('visible');
  }
}

function deviceName(index) {
  if (!Number.isFinite(index) || index < 0) return null;
  const d = S.devices.find((x) => x.index === index);
  return d ? d.name : null;
}

// ============================================================
// AUDIO DIAGNOSTIC
// ============================================================
function openAudioDiag() {
  els.diagModal.style.display = 'flex';
  if (!els.diagBody.dataset.loaded) runAudioDiag();
}
function closeAudioDiag() { els.diagModal.style.display = 'none'; }

async function runAudioDiag() {
  if (S.isRunning) {
    els.diagResult.textContent = '请先停止录制再诊断';
    els.diagResult.className = 'asr-test-result err';
    return;
  }
  els.btnDiagRun.disabled = true;
  els.diagBody.innerHTML = '<span class="placeholder">正在逐个打开输入流（失败设备会尝试多种配置），请稍候…</span>';
  els.diagResult.textContent = '诊断中…';
  els.diagResult.className = 'asr-test-result';
  try {
    const r = await invoke('probe_audio_devices');
    els.diagBody.dataset.loaded = '1';
    renderDiag(r);
  } catch (e) {
    els.diagBody.innerHTML = '<div class="diag-err">诊断失败: ' + escHtml(String(e)) + '</div>';
    els.diagResult.textContent = '✗ 失败';
    els.diagResult.className = 'asr-test-result err';
  } finally {
    els.btnDiagRun.disabled = false;
  }
}

function renderDiag(r) {
  const probes = r.probes || [];
  const ok = probes.filter((p) => p.ok).length;
  els.diagResult.textContent = `宿主 ${r.host} · ${ok}/${probes.length} 个设备可打开`;
  els.diagResult.className = 'asr-test-result ' + (ok > 0 ? 'ok' : 'err');

  if (!probes.length) {
    els.diagBody.innerHTML = '<div class="diag-err">未枚举到任何输入设备。请检查系统麦克风权限与音频驱动。</div>';
    return;
  }

  let html = `<div class="diag-summary">目标输出 <b>${r.targetSampleRate || 16000} Hz / 单声道 / f32</b>；「命中」即该设备实际可用的采集配置。</div>`;
  for (const p of probes) {
    const tag = p.isLoopback
      ? '<span class="diag-tag strong">内录</span>'
      : (p.loopbackHint ? '<span class="diag-tag weak">疑似内录</span>' : '');
    const state = p.ok
      ? '<span class="diag-state ok">✅ 可打开</span>'
      : '<span class="diag-state fail">❌ 打不开</span>';
    const detail = escHtml(p.ok ? (p.config || '') : (p.error || ''));
    const attempts = (!p.ok && p.attempts && p.attempts.length)
      ? `<details class="diag-attempts"><summary>查看 ${p.attempts.length} 条尝试明细</summary><pre>${escHtml(p.attempts.join('\n'))}</pre></details>`
      : '';
    html += `<div class="diag-item ${p.ok ? 'ok' : 'fail'}">
      <div class="diag-item-head"><span class="diag-idx">[${p.index}]</span> ${escHtml(p.name)} ${tag} ${state}</div>
      <div class="diag-detail">${detail}</div>
      ${attempts}
    </div>`;
  }
  html += '<div class="diag-tip">💡 若内录取不到声音，可在 Windows 命令行运行 <code>audio-probe.exe --level</code> 并播放音乐，查看该设备电平是否大于 0。</div>';
  els.diagBody.innerHTML = html;
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

  const micIdx = parseInt(els.micSelect.value);
  const loopIdx = els.dualToggle.checked ? parseInt(els.loopSelect.value) : -1;
  const config = {
    micDeviceIndex: Number.isFinite(micIdx) ? micIdx : -1,
    loopbackDeviceIndex: Number.isFinite(loopIdx) ? loopIdx : -1,
    micDeviceName: deviceName(micIdx),
    loopbackDeviceName: els.dualToggle.checked ? deviceName(loopIdx) : null,
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
// HELP / GUIDES
// ============================================================
const REPO = 'https://github.com/xiaopengs/moonshine-meeting-assistant';
const GUIDE_URLS = {
  access: `${REPO}/blob/main/doc/接入指导.md`,
  build: `${REPO}/blob/main/doc/本地编译指导.md`,
  deploy: `${REPO}/blob/main/doc/部署指导.md`,
};
let HELP_TAB = 'access';

async function openExternal(url) {
  try {
    await invoke('open_external_url', { url });
  } catch (e) {
    addLog('[WARN] 打开链接失败: ' + e, 'warn');
  }
}

const HELP_CONTENT = {
  access: {
    title: '☁️ 云 ASR 接入指导',
    lead: '云端 ASR 采用 BYOK 模式：在云控制台开通服务并填入 Key 即可用，<b>无需 Python 与本地模型</b>。选择云引擎后，16 kHz PCM 音频会实时上传至所选服务商。',
    steps: [
      ['开通服务并创建应用', '火山：语音技术控制台创建应用并勾选「流式语音识别大模型」；腾讯：开通语音识别并在「API 密钥管理」新建密钥；阿里：开通 NLS 并创建项目。'],
      ['获取凭证', '火山 <code>AK / SK + APP ID</code>；腾讯 <code>AppID / SecretId / SecretKey</code>；阿里 <code>AccessKey ID / Secret + 项目 Appkey</code>。'],
      ['在 App 中填写', '侧栏选择云引擎 →「🔑 云 ASR 凭证设置」→ 选服务商 → 填 Key → 保存。凭证仅存本机 <code>asr-settings.json</code>。'],
      ['验证', '点「测试连接」：火山 / 腾讯做 WebSocket 握手，阿里验证 CreateToken；通过后再录 30 秒真实音频做端到端确认。'],
      ['排错', '鉴权失败先核对控制台版本差异（火山新旧版）、Key 是否含空格、系统时间是否准确；模型名可在主界面手动填写。'],
    ],
    code: {
      label: '凭证文件位置（app_config_dir）',
      lines: 'Windows  %APPDATA%\\com.moonshine.meeting-assistant\\\nmacOS    ~/Library/Application Support/com.moonshine.meeting-assistant/\nLinux    ~/.config/com.moonshine.meeting-assistant/',
    },
    tips: [
      '三家按识别时长 / 并发计费，双路采集占用 2 路并发',
      '火山新版控制台仅提供 App Key 时需按完整文档适配鉴权方式',
      '涉密会议请改用本地引擎（音频零上传）',
    ],
    doc: '接入指导.md',
  },
  build: {
    title: '🛠️ 本地编译指导',
    lead: '前端是<b>零构建静态文件</b>，不需要 Node / npm；云 ASR 引擎也不需要 Python。只编译核心库时甚至不需要 WebKitGTK。',
    steps: [
      ['安装工具链', 'Rust stable ≥ 1.75；Linux 另需 WebKitGTK / GTK / ALSA 等系统库（见下方命令）；macOS 装 Xcode CLT；Windows 装 MSVC 生成工具 + WebView2。'],
      ['只编译核心库（最快）', '<code>cargo check -p moonshine-core</code>，含三家云 ASR 签名与 WebSocket 代码，不依赖 Tauri 系统库。'],
      ['整仓检查', '<code>cargo check --workspace</code> 与 <code>cargo clippy --workspace</code>。'],
      ['安装 CLI 并运行', '<code>cargo install tauri-cli --version "^2" --locked</code> → <code>cargo tauri dev</code>。'],
      ['打包', '<code>cargo tauri build</code>，产物位于 <code>target/release/bundle/</code>（NSIS / DMG / AppImage·deb）。'],
    ],
    code: {
      label: 'Linux（Ubuntu / Debian）系统依赖',
      lines: 'sudo apt-get install -y build-essential pkg-config \\\n  libwebkit2gtk-4.1-dev libgtk-3-dev \\\n  libayatana-appindicator3-dev librsvg2-dev \\\n  libasound2-dev libssl-dev patchelf',
    },
    tips: [
      '本地引擎需 pip install -r python/requirements.txt',
      '模型下载慢可设 HF_ENDPOINT=https://hf-mirror.com',
      '打包后脚本路径可用 MOONSHINE_ENGINE_SCRIPT 覆盖',
    ],
    doc: '本地编译指导.md',
  },
  deploy: {
    title: '📦 部署指导',
    lead: '三种部署形态按隐私与运维成本选择，<b>切换引擎无需重装</b>。',
    steps: [
      ['A 纯本地（默认）', '装 Python + 引擎依赖 + 预置模型，音频零上传，适合涉密 / 内网环境。'],
      ['B 云 ASR（BYOK）', '免 Python 与模型，只需填 Key；需实时出网，音频上传至所选厂商。'],
      ['C 混合', '日常用云引擎，敏感会议切换本地引擎。'],
      ['随包资源', '<code>bundle.resources</code> 会把 <code>python/engine_service.py</code> 与 requirements.txt 打进安装包；运行时按「环境变量 → 开发目录 → 资源目录」顺序查找脚本。'],
      ['系统内录', 'Windows 启用「立体声混音」；macOS 需虚拟声卡（BlackHole 等）；Linux 用 PulseAudio monitor 设备。'],
      ['验收', '设备枚举、30 秒录音、导出 MD/SRT/TXT、长会稳定性逐项确认。'],
    ],
    code: {
      label: '云 ASR 出网域名白名单（WebSocket / HTTPS 443）',
      lines: 'openspeech.bytedance.com\nasr.cloud.tencent.com\nnls-meta.cn-shanghai.aliyuncs.com\nnls-gateway-cn-shanghai.aliyuncs.com',
    },
    tips: [
      'macOS 分发需 Developer ID 签名 + 公证，Windows 建议代码签名',
      '凭证位于用户配置目录，覆盖安装不会被覆盖',
      '当前版本尚未在真机打包 / 真实云 Key 联调，请按验收清单确认',
    ],
    doc: '部署指导.md',
  },
};

function renderHelp() {
  const c = HELP_CONTENT[HELP_TAB];
  const steps = c.steps.map((s, i) => `
    <div class="help-step">
      <div class="help-step-num">${i + 1}</div>
      <div>
        <div class="help-step-title">${escHtml(s[0])}</div>
        <div class="help-step-desc">${s[1]}</div>
      </div>
    </div>`).join('');
  const tips = c.tips.map((t) => `<li>${escHtml(t)}</li>`).join('');
  els.helpBody.innerHTML = `
    <div class="help-lead">${c.lead}</div>
    <div class="help-steps">${steps}</div>
    <div class="help-code-label">${escHtml(c.code.label)}</div>
    <pre class="help-code">${escHtml(c.code.lines)}</pre>
    <div class="help-section-title">注意事项</div>
    <ul class="help-tips">${tips}</ul>
  `;
  els.helpHint.textContent = '完整文档：doc/' + c.doc;
  els.helpTabs.querySelectorAll('.help-tab').forEach((b) => {
    b.classList.toggle('active', b.dataset.tab === HELP_TAB);
  });
}

function openHelp(tab) {
  if (tab) HELP_TAB = tab;
  renderHelp();
  els.helpModal.style.display = 'flex';
}

function closeHelp() {
  els.helpModal.style.display = 'none';
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
    <div class="asr-guide-box">
      <div class="asr-guide-title">🔑 ${escHtml(p.name)} 取 Key 指引</div>
      <p>${escHtml(p.keyGuide || '在服务商控制台开通语音识别并创建凭证')}</p>
      <div class="asr-guide-links">
        <button type="button" class="link-btn" data-open-url="${escHtml(p.console)}">${escHtml(p.consoleLabel || '打开控制台')} ↗</button>
        ${p.docUrl ? `<button type="button" class="link-btn" data-open-url="${escHtml(p.docUrl)}">官方文档 ↗</button>` : ''}
        <button type="button" class="link-btn" data-open-url="${escHtml(GUIDE_URLS.access)}">接入指导 ↗</button>
      </div>
    </div>
    ${fields}
    <label class="asr-field">
      <span class="asr-field-label">识别模型</span>
      <select data-asr-model>${models}</select>
    </label>
  `;
  els.asrFields.querySelectorAll('[data-open-url]').forEach((btn) => {
    btn.addEventListener('click', () => {
      const url = btn.dataset.openUrl;
      if (url) openExternal(url);
    });
  });
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
  tencent: { model: '16k_zh_en', lang: 'zh-CN', remote: true },
  alicloud: { model: 'domain-meeting-domain', lang: 'zh-CN', remote: true },
};
els.engineSelect.addEventListener('change', () => {
  const d = ENGINE_DEFAULTS[els.engineSelect.value] || ENGINE_DEFAULTS.faster_whisper;
  els.modelInput.value = ASR_MODELS[els.engineSelect.value] || d.model;
  els.langInput.value = d.lang;
  els.btnAsrSettings.style.display = d.remote ? '' : 'none';
  if (d.remote) {
    const saved = (ASR_SETTINGS && ASR_SETTINGS[els.engineSelect.value]) || {};
    const hasCreds = Object.values(saved).some((v) => typeof v === 'string' && v.trim().length > 0);
    setDeviceStatus(
      els.engineStatus,
      hasCreds ? 'ok' : 'warn',
      hasCreds ? '已保存凭证，建议先「测试连接」' : '请点击「🔑 云 ASR 凭证设置」填入 Key'
    );
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
els.asrGuideLink.addEventListener('click', (e) => { e.preventDefault(); openExternal(GUIDE_URLS.access); });

els.btnHelp.addEventListener('click', () => openHelp());
els.btnHelpSide.addEventListener('click', () => openHelp());
els.helpModalClose.addEventListener('click', closeHelp);
els.btnHelpClose.addEventListener('click', closeHelp);
els.btnHelpDoc.addEventListener('click', () => openExternal(GUIDE_URLS[HELP_TAB]));
els.helpModal.addEventListener('click', (e) => { if (e.target === els.helpModal) closeHelp(); });
els.helpTabs.addEventListener('click', (e) => {
  const btn = e.target.closest('.help-tab');
  if (!btn) return;
  HELP_TAB = btn.dataset.tab;
  renderHelp();
});

els.btnAudioDiag.addEventListener('click', openAudioDiag);
els.btnDiagRun.addEventListener('click', runAudioDiag);
els.btnDiagClose.addEventListener('click', closeAudioDiag);
els.diagModalClose.addEventListener('click', closeAudioDiag);
els.diagModal.addEventListener('click', (e) => { if (e.target === els.diagModal) closeAudioDiag(); });

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
  if (e.key === 'Escape') {
    if (els.helpModal.style.display !== 'none') { closeHelp(); return; }
    if (els.diagModal.style.display !== 'none') { closeAudioDiag(); return; }
    if (els.asrModal.style.display !== 'none') { closeAsrSettings(); return; }
    if (S.isRunning) stopRecording();
  }
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
