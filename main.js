/**
 * Moonshine 会议助手 - Electron 主进程
 * 功能：音频设备枚举、双路采集控制、Python 子进程管理、实时字幕渲染
 */

const { app, BrowserWindow, ipcMain, dialog } = require('electron');
const path = require('path');
const { spawn } = require('child_process');
const log = require('electron-log');

log.transports.file.level = 'info';
log.transports.file.resolvePathFn = () => path.join(app.getPath('userData'), 'logs', 'main.log');
log.info('=== Moonshine 会议助手启动 ===');

// 全局窗口引用
let mainWindow = null;
// Python 转写子进程
let pythonProcess = null;

// ========================
// 窗口创建
// ========================
function createWindow() {
  mainWindow = new BrowserWindow({
    width: 900,
    height: 700,
    minWidth: 700,
    minHeight: 500,
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
    mainWindow.setTitle('Moonshine 会议助手');
    log.info('主窗口已显示');
  });

  mainWindow.on('closed', () => {
    mainWindow = null;
    stopTranscription();
  });
}

// ========================
// Python 子进程管理
// ========================
function startTranscription(config) {
  if (pythonProcess) {
    log.warn('Python 进程已存在，先停止');
    stopTranscription();
  }

  const pythonScript = path.join(__dirname, 'python', 'transcribe.py');
  const pythonExec = process.platform === 'win32' ? 'python' : 'python3';

  const args = [
    pythonScript,
    '--mic-device', String(config.micDeviceIndex),
    '--loopback-device', String(config.loopbackDeviceIndex),
    '--sample-rate', '16000',
    '--vad', String(config.vadEnabled ? 1 : 0),
  ];

  log.info('启动 Python 转写服务:', args.join(' '));

  pythonProcess = spawn(pythonExec, args, {
    stdio: ['pipe', 'pipe', 'pipe'],
    env: {
      ...process.env,
      PYTHONIOENCODING: 'utf-8',
      PATH: process.env.PATH,
    },
  });

  pythonProcess.stdout.setEncoding('utf-8');
  pythonProcess.stderr.setEncoding('utf-8');

  pythonProcess.stdout.on('data', (data) => {
    const lines = data.trim().split('\n').filter(Boolean);
    for (const line of lines) {
      try {
        const msg = JSON.parse(line);
        if (mainWindow && !mainWindow.isDestroyed()) {
          mainWindow.webContents.send('transcript', msg);
        }
      } catch {
        // 非 JSON 行直接输出
        if (mainWindow && !mainWindow.isDestroyed()) {
          mainWindow.webContents.send('log', line);
        }
      }
    }
  });

  pythonProcess.stderr.on('data', (data) => {
    log.error('Python stderr:', data);
    if (mainWindow && !mainWindow.isDestroyed()) {
      mainWindow.webContents.send('log', '[ERROR] ' + data);
    }
  });

  pythonProcess.on('error', (err) => {
    log.error('Python 进程启动失败:', err);
    dialog.showErrorBox('启动失败', '无法启动 Python 转写服务，请确认已安装 Python 依赖：\npip install moonshine sounddevice numpy');
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
    log.info('停止 Python 转写服务');
    pythonProcess.kill('SIGTERM');
    pythonProcess = null;
  }
}

// ========================
// IPC 处理器
// ========================

// 获取系统音频设备列表
ipcMain.handle('get-audio-devices', async () => {
  log.info('查询音频设备');
  const pythonScript = path.join(__dirname, 'python', 'list_devices.py');
  const pythonExec = process.platform === 'win32' ? 'python' : 'python3';

  return new Promise((resolve) => {
    const proc = spawn(pythonExec, [pythonScript], { stdio: 'pipe' });
    let output = '';
    proc.stdout.setEncoding('utf-8');
    proc.stdout.on('data', (d) => { output += d; });
    proc.on('close', () => {
      try {
        resolve(JSON.parse(output));
      } catch {
        resolve({ error: '无法获取设备列表，请安装 sounddevice: pip install sounddevice' });
      }
    });
    proc.on('error', () => resolve({ error: 'Python 不可用' }));
  });
});

// 启动转写
ipcMain.handle('start-transcription', async (event, config) => {
  log.info('启动转写:', config);
  startTranscription(config);
  return { ok: true };
});

// 停止转写
ipcMain.handle('stop-transcription', async () => {
  stopTranscription();
  return { ok: true };
});

// 保存字幕文件
ipcMain.handle('save-subtitle', async (event, { filename, content }) => {
  const { filePath } = await dialog.showSaveDialog(mainWindow, {
    title: '保存字幕/笔记',
    defaultPath: filename,
    filters: [
      { name: 'Markdown', extensions: ['md'] },
      { name: '纯文本', extensions: ['txt'] },
      { name: 'SRT字幕', extensions: ['srt'] },
    ],
  });
  if (filePath) {
    require('fs').writeFileSync(filePath, content, 'utf-8');
    log.info('文件已保存:', filePath);
    return { ok: true, path: filePath };
  }
  return { ok: false };
});

// 获取版本
ipcMain.handle('get-version', () => {
  return app.getVersion();
});

// ========================
// 应用生命周期
// ========================
app.whenReady().then(() => {
  log.info('App ready');
  createWindow();

  app.on('activate', () => {
    if (BrowserWindow.getAllWindows().length === 0) {
      createWindow();
    }
  });
});

app.on('window-all-closed', () => {
  stopTranscription();
  if (process.platform !== 'darwin') {
    app.quit();
  }
});

app.on('before-quit', () => {
  stopTranscription();
});
