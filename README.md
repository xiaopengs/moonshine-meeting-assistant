# 🌙 Moonshine 会议助手

> 支持**麦克风 + 系统内录**双路语音转写的跨平台 Electron 桌面应用

## 功能特性

- 🎤 **麦克风采集** — 实时采集人声并转写
- 🔊 **系统内录** — 无需虚拟声卡，WASAPI Loopback 直接录制系统音频
- ⚡ **双路同时** — 麦克风 + 内录同时开启，分别标记发言人
- 🤫 **全程离线** — 所有音频数据不联网，纯本地处理
- 📝 **实时字幕** — 字幕实时显示，支持 Markdown 导出
- 🌐 **跨平台** — Windows / macOS / Linux

## 系统要求

- **Windows 10+**（内置 WASAPI Loopback，无需虚拟声卡）
- **macOS 10.15+**（需安装 BlackHole 2ch 虚拟声牙）
- **Linux**（需安装 PulseAudio 或 ALSA loopback 模块）

## 安装依赖

### Python 依赖

```bash
pip install moonshine sounddevice numpy
```

或一键安装：

```bash
pip install -r requirements.txt
```

### Node.js 依赖

```bash
npm install
```

## 运行

```bash
# 查询可用音频设备
python python/list_devices.py

# 启动 Electron 应用
npm start
```

## 设备说明

### Windows（推荐）

系统内录使用 **WASAPI Loopback**，声卡自带功能，无需安装虚拟声卡。
在应用中选择名称含 `Loopback` 或 `WASAPI` 的设备即可。

### macOS

需安装 **BlackHole 2ch**（免费开源）：
```bash
brew install blackhole-2ch
```
然后将系统音频输出路由到 BlackHole，再在应用中选择 BlackHole 作为内录设备。

### Linux

```bash
# PulseAudio 内录模块
pactl load-module module-loopback
```

## 技术栈

| 层级 | 技术 |
|------|------|
| 框架 | Electron 28 + TypeScript |
| UI | HTML/CSS/JS（原生，无框架依赖） |
| 后端转写 | Python 3.10+ / moonshine-small (ONNX) |
| 音频采集 | sounddevice + WASAPI Loopback |
| 打包 | electron-builder |

## 项目结构

```
moonshine-meeting-assistant/
├── main.js               # Electron 主进程
├── preload.js             # 安全桥接（IPC）
├── package.json           # 构建配置
├── renderer/
│   └── index.html         # 完整 UI（字幕 + 设备选择）
├── python/
│   ├── list_devices.py    # 音频设备枚举
│   └── transcribe.py      # 双路转写核心
└── assets/                # 图标等资源
```

## 构建发布包

```bash
# Windows
npm run build -- --win

# macOS
npm run build -- --mac

# Linux
npm run build -- --linux
```

## License

MIT
