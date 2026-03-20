/**
 * main.js — Moonshine 会议助手 Electron 主进程
 * 职责：窗口管理 / IPC 处理器 / Python 子进程生命周期
 */

const { app, BrowserWindow, ipcMain, dialog } = require('electron');
const path = require('path');
const { spawn } = require('child_process');
const log = require('electron-log');

log.transports.file.level = 'info';
log.transports.file.resolvePathFn = () => path.join(app.getPath('userData'), 'logs', 'main.log'));
log.info('=== Moonshine 会议助手启动 ===');

let mainWindow = null;
let pythonProcess = null;

// ========================
// 窗口
// ========================
function createWindow() {
  mainWindow = new BrowserWindow({
    width: 960,
    height: 680,
    minWidth: 760,
    minHeight: 520,
    title: 'Moonshine 会议助手',
    backgroundColor: '#0d1117',
    webPreferences: {
      preload: path.join(__dirname, 'preload.js'),
      contextIsolation: true,
      nodeIntegration: false,
      sandbox: false,
    },
    show: false,
  });

  mainWindow.loadFile(path.join(__dirname, 'renderer', 'index.html'));

  mainWindow.once('ready-to-show', () => {
    mainWindow.show();
    log.info('主窗口已显示');
  });

  mainWindow.on('closed', () => {
    mainWindow = null;
    stopTranscription();
  });
}

// ========================
// Python 子进程
// ========================
function startTranscription(config) {
  if (pythonProcess) {
    log.warn('已有 Python 进程，先停止');
    stopTranscription();
  }

  const script = path.join(__dirname, 'python', 'transcribe.py');
  const python = process.platform === 'win32' ? 'python' : 'python3';

  const args = [
    script,
    '--mic-device',     String(config.micDeviceIndex),
    '--loopback-device', String(config.loopbackDeviceIndex),
    '--sample-rate',    '16000',
    '--vad',           config.vadEnabled ? '1' : '0',
  ];

  // 双路关闭时加 flag
  if (!config.dualEnabled) {
    args.push('--no-dual');
  }

  log.info('启动 Python:', python, args.join(' '));

  pythonProcess = spawn(python, args, {
    stdio: ['pipe', 'pipe', 'pipe'],
    env: { ...process.env, PYTHONIOENCODING: 'utf-8' },
  });

  pythonProcess.stdout.setEncoding('utf-8');
  pythonProcess.stderr.setEncoding('utf-8');

  // JSON 行 → transcript 事件
  // 非 JSON 行 → log 事件
  pythonProcess.stdout.on('data', (raw) => {
    const lines = String(raw).split('\n').filter(l => l.trim());
    for (const line of lines) {
      try {
        const msg = JSON.parse(line);
        if (msg.log) {
          // { log: "..." } 是日志消息
          if (mainWindow && !mainWindow.isDestroyed()) {
            mainWindow.webContents.send('log', msg.log);
          }
        } else {
          // 转写结果
          if (mainWindow && !mainWindow.isDestroyed()) {
            mainWindow.webContents.send('transcript', msg);
          }
        }
      } catch {
        // 原始非 JSON 行直接打印
        if (mainWindow && !mainWindow.isDestroyed()) {
          mainWindow.webContents.send('log', line);
        }
      }
    }
  });

  pythonProcess.stderr.on('data', (raw) => {
    const txt = String(raw).trim();
    if (txt) {
      log.error('[Python stderr]', txt);
      if (mainWindow && !mainWindow.isDestroyed()) {
        mainWindow.webContents.send('log', '[ERROR] ' + txt);
      }
    }
  });

  pythonProcess.on('error', (err) => {
    log.error('Python 进程启动失败:', err);
    dialog.showErrorBox(
      '启动失败',
      '无法启动 Python 转写服务。\n\n请确认已安装依赖：\npip install moonshine sounddevice numpy'
    );
    if (mainWindow && !mainWindow.isDestroyed()) {
      mainWindow.webContents.send('status', 'stopped');
    }
  });

  pythonProcess.on('exit', (code) => {
    log.info('Python 进程退出, code:', code);
    pythonProcess = null;
    if (mainWindow && !mainWindow.isDestroyed()) {
      mainWindow.webContents.send('status', 'stopped');
    }
  });
}

function stopTranscription() {
  if (pythonProcess) {
    log.info('停止 Python 进程');
    pythonProcess.kill('SIGTERM');
    pythonProcess = null;
  }
}

// ========================
// IPC
// ========================

// 获取音频设备列表（带超时保护）
ipcMain.handle('get-audio-devices', async () => {
  const script = path.join(__dirname, 'python', 'list_devices.py');
  const python = process.platform === 'win32' ? 'python' : 'python3';

  return new Promise((resolve) => {
    // 10 秒超时
    const timer = setTimeout(() => {
      try { proc.kill(); } catch {}
      resolve({ error: '设备查询超时，请检查 Python 和 sounddevice 安装状态' });
    }, 10000);

    const proc = spawn(python, [script], { stdio: 'pipe' });
    let out = '';
    proc.stdout.setEncoding('utf-8');
    proc.stdout.on('data', d => { out += d; });
    proc.on('close', (code) => {
      clearTimeout(timer);
      try { resolve(JSON.parse(out)); }
      catch { resolve({ error: '设备数据解析失败: ' + (out || '').slice(0, 100) }); }
    });
    proc.on('error', (e) => {
      clearTimeout(timer);
      resolve({ error: 'Python 不可用: ' + e.message });
    });
  });
});

// 启动转写
ipcMain.handle('start-transcription', async (event, config) => {
  startTranscription(config);
  return { ok: true };
});

// 停止转写
ipcMain.handle('stop-transcription', async () => {
  stopTranscription();
  return { ok: true };
});

// 保存字幕
ipcMain.handle('save-subtitle', async (event, { filename, content }) => {
  const { filePath } = await dialog.showSaveDialog(mainWindow, {
    title: '保存会议记录',
    defaultPath: filename,
    filters: [
      { name: 'Markdown', extensions: ['md'] },
      { name: '纯文本',    extensions: ['txt'] },
      { name: 'SRT字幕',  extensions: ['srt'] },
    ],
  });
  if (filePath) {
    require('fs').writeFileSync(filePath, content, 'utf-8');
    return { ok: true, path: filePath };
  }
  return { ok: false };
});

// 版本号
ipcMain.handle('get-version', () => app.getVersion());

// ========================
// 生命周期
// ========================
app.whenReady().then(() => {
  log.info('App ready');
  createWindow();
  app.on('activate', () => {
    if (BrowserWindow.getAllWindows().length === 0) createWindow();
  });
});

app.on('window-all-closed', () => {
  stopTranscription();
  if (process.platform !== 'darwin') app.quit();
});

app.on('before-quit', () => { stopTranscription(); });
