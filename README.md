# 🌙 Moonshine — 本地 AI 会议助手

<div align="center">

[![MIT License](https://img.shields.io/badge/License-MIT-blue.svg)](https://github.com/xiaopengs/moonshine-meeting-assistant/blob/main/LICENSE)
[![Platform](https://img.shields.io/badge/Platform-Windows%20%7C%20macOS%20%7C%20Linux-green.svg)](https://github.com/xiaopengs/moonshine-meeting-assistant)
[![Model](https://img.shields.io/badge/Model-moonshine--small%20123M-orange.svg)](https://github.com/moonshine-ai/moonshine)
[![Python](https://img.shields.io/badge/Python-3.9%2B-yellow.svg)](https://www.python.org/)
[![Electron](https://img.shields.io/badge/Electron-28-blue.svg)](https://electronjs.org/)

**开源免费 · 完全本地 · 隐私零风险**

*[Live Demo](https://nrzriq5a49cp.space.minimaxi.com)* · *[下载Releases](https://github.com/xiaopengs/moonshine-meeting-assistant/releases)*

</div>

---

## ✨ 痛点

| 你是否遇到过 | 现状 | 根因 |
|---|---|---|
| 会议内容涉及敏感信息，不敢用云端转写 | 商业机密裸奔到服务器 | 数据利益驱动 |
| 内录会议软件声音还要装虚拟声卡 | 配置半小时，会议五分钟 | 工具割裂 |
| 导出格式不兼容，复制到笔记工具重排版 | 转写 10 分钟，整理半小时 | 没有一件事被真正解决 |
| 每次转写都要等云端排队 | 延迟 2-5 秒，体验割裂 | 网络依赖 |

**Moonshine 的前提假设：** 会议记录的本质是「让没参会的人获得同等上下文」——不是为了 AI，是为了可查询、可追溯、可复用。

---

## 🎯 核心能力

### 🎙️ 双路实时采集
同时捕获**麦克风**（你的声音）和**系统内录**（对方/会议软件声音），自动区分来源，输出结构化对话记录。

> Windows 原生支持 WASAPI Loopback，macOS 支持 BlackHole，无需安装虚拟声卡。

### ⚡ 端到端 < 300ms
160ms 音频 chunk + ~80ms moonshine 推理，VAD 静音跳过。实时字幕几乎无感知延迟，会议节奏不被打破。

### 🔒 完全本地运行
所有数据留在本地磁盘，不经过任何服务器。没有订阅费，没有数据留厂商风险，离线环境也能用。

### 📁 多格式一键导出
- **Markdown** — 会议纪要，直接导入 Notion / 飞书 / Obsidian
- **SRT 字幕** — 压制字幕视频，兼容所有主流播放器
- **纯文本** — 快速复制到任何地方

---

## 🚀 30 秒快速上手

```bash
# 1. 克隆
git clone https://github.com/xiaopengs/moonshine-meeting-assistant.git
cd moonshine-meeting-assistant

# 2. 安装（自动检测 Python 环境）
npm install && pip install -r requirements.txt

# 3. 启动（首次自动下载模型 ~95MB）
npm start
```

> macOS / Linux 用户运行 `python3` 替代 `pip`。

<details>
<summary><strong>Windows 系统内录设置（一次性）</strong></summary>

1. 右键点击任务栏 🔊 音量图标 → 「打开声音设置」
2. 拉到底 → 「声音控制面板」
3. 切换到「录制」选项卡
4. 右键空白处 → ☑️「显示禁用的设备」
5. 找到「**立体声混音**」→ 右键 → 启用 → 确定
6. 回到 App 点击「↻ 刷新设备」

</details>

<details>
<summary><strong>macOS 内录设置</strong></summary>

1. 安装 BlackHole：`brew install blackhole-2ch`
2. 打开「音频 MIDI 设置」→ 创建「聚集设备」，勾选「内置麦克风 + BlackHole」
3. 将系统音频输出设置为 BlackHole
4. App 内「系统内录」选择 BlackHole 设备

</details>

---

## 🧠 技术架构

```
[麦克风 / WASAPI Loopback]
           ↓
   sounddevice 16kHz 采集
           ↓
      VAD 静音检测
           ↓
  moonshine-small streaming
       ~80ms 推理
           ↓
    JSON stdout → IPC
           ↓
   Electron 实时渲染字幕
           ↓
      MD / SRT 导出
```

**技术栈：**
- 推理引擎：[moonshine-ai/moonshine](https://github.com/moonshine-ai/moonshine)（Apache 2.0，123M params，WER 7.84%）
- 音频采集：`sounddevice`（跨平台 WASAPI / CoreAudio / ALSA）
- 桌面框架：`Electron 28`（无框架 UI，轻量可控）
- IPC 协议：`stdout JSON`（零依赖稳定通道）
- 打包构建：`electron-builder`（Windows NSIS + macOS DMG + Linux AppImage）

---

## 📊 与竞品对比

| | 🌙 Moonshine | 讯飞听见 | Otter.ai | Whisper Desktop |
|---|---|---|---|---|
| 运行方式 | **完全本地** | 云端 | 云端 | 本地 |
| 实时字幕 | **●** | ● | ● | ○ 批量 |
| 系统内录 | **●** | △ 部分 | ○ | ○ |
| 端到端延迟 | **<300ms** | 1-3s | 2-5s | N/A |
| 使用成本 | **免费** | 付费 | 付费 | 免费 |
| 隐私风险 | **零风险** | 数据留厂商 | 数据留厂商 | 零风险 |
| 跨平台 | **Win/Mac/Linux** | Win/Mac | 全平台 | 需配置 |
| 开箱即用 | **●** | ● | ● | ○ 命令行 |
| 双路采集 | **●** | ○ | ○ | ○ |
| 开源 | **MIT** | 闭源 | 闭源 | 开源 |

---

## 📐 技术规格

| 参数 | 值 |
|---|---|
| 模型 | moonshine-small streaming |
| 参数量 | 123M |
| WER（英文 LibriSpeech） | 7.84% |
| 模型大小 | 95 MB |
| 采样率 | 16,000 Hz |
| Chunk 大小 | 160ms（2,560 samples）|
| moonshine 推理延迟 | ~80ms（Linux 实测）|
| 端到端延迟 | **< 300ms** |
| 音频接口 | WASAPI（Win）/ CoreAudio（Mac）/ ALSA（Linux）|
| Python 版本 | ≥ 3.9 |
| Node.js 版本 | ≥ 18.x |

---

## 🗂️ 项目结构

```
moonshine-meeting-assistant/
├── main.js                  # Electron 主进程
│                            #   · 窗口管理
│                            #   · Python 子进程生命周期
│                            #   · IPC 通道
│                            #   · 文件导出对话框
├── preload.js               # contextBridge 安全桥接
├── renderer/
│   └── index.html           # 完整 UI（v2.0 商业级界面）
│                            #   · 三栏布局 + 启动引导
│                            #   · Canvas VAD 波形可视化
│                            #   · AI 摘要面板（预留接口）
│                            #   · 键盘快捷键系统
├── python/
│   ├── transcribe.py        # 转写核心
│   │                            #   · sounddevice 双路采集
│   │                            #   · VAD 能量阈值检测
│   │                            #   · moonshine 流式推理
│   │                            #   · JSON stdout 回报协议
│   └── list_devices.py      # 音频设备枚举
├── assets/                  # 图标资源
│   ├── icon.png             # 256px 主图标
│   └── icon.ico             # Windows 多尺寸图标
├── doc/
│   ├── PRD.md               # 产品需求文档
│   ├── PROGRESS.md          # 里程碑进度
│   ├── UI-SPEC-v2.md        # v2.0 UI 设计规格书
│   └── Moonshine_AI_跨平台会议助手方案.md
├── package.json
├── requirements.txt         # Python 依赖
└── README.md
```

---

## 🌐 部署静态介绍页

静态介绍页已部署至 GitHub Pages：

> **https://xiaopengs.github.io/moonshine-meeting-assistant/**

（对应 `docs/` 目录，GitHub Pages 自动从 `main` 分支 `docs/` 文件夹提供服务）

如需自定义域名：在仓库 Settings → Pages → Source 选择 `main` 分支 `docs/` 文件夹，保存即可。

---

## 🤝 贡献

Issues 和 PR 欢迎！如果你有功能想法或发现 Bug，请提交 [Issue](https://github.com/xiaopengs/moonshine-meeting-assistant/issues)。

**开发模式：**
```bash
npm run dev          # 带日志输出运行
pip install -e .    # 以开发模式安装 moonshine（修改源码即时生效）
```

---

## 📄 License

MIT — 可免费商用，无需署名（但非常感谢一个 ⭐）

---

*Moonshine：让每一次会议都被认真记录。*
