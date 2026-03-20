/**
 * preload.js — 安全桥接主进程与渲染进程
 */
const { contextBridge, ipcRenderer } = require('electron');

contextBridge.exposeInMainWorld('electronAPI', {
  // 音频设备
  getAudioDevices: () => ipcRenderer.invoke('get-audio-devices'),

  // 转写控制
  startTranscription: (config) => ipcRenderer.invoke('start-transcription', config),
  stopTranscription: () => ipcRenderer.invoke('stop-transcription'),

  // 文件保存
  saveSubtitle: (data) => ipcRenderer.invoke('save-subtitle', data),

  // 版本
  getVersion: () => ipcRenderer.invoke('get-version'),

  // 事件监听
  onTranscript: (cb) => ipcRenderer.on('transcript', (e, d) => cb(d)),
  onLog: (cb) => ipcRenderer.on('log', (e, d) => cb(d)),
  onStatus: (cb) => ipcRenderer.on('status', (e, d) => cb(d)),

  // 移除监听
  removeAllListeners: (ch) => ipcRenderer.removeAllListeners(ch),
});
