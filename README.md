# 🌙 Moonshine — 本地 AI 会议助手

<div align="center">

[![MIT License](https://img.shields.io/badge/License-MIT-blue.svg)](./LICENSE)
[![Platform](https://img.shields.io/badge/Platform-Windows%20%7C%20macOS%20%7C%20Linux-green.svg)](#)
[![Tauri](https://img.shields.io/badge/Tauri-2-blue.svg)](https://tauri.app)
[![Rust](https://img.shields.io/badge/Rust-1.7x-orange.svg)](https://www.rust-lang.org)
[![Python](https://img.shields.io/badge/Python-3.9%2B-yellow.svg)](https://www.python.org)

**开源免费 · 本地优先 · 云端 BYOK 可插拔 · 隐私零风险**

</div>

---

## ✨ 这是什么

一个**本地优先的跨平台会议转写助手**：同时采集**麦克风**（你的声音）和**系统内录**（对方/会议软件声音），实时转写为带说话人归属的结构化会议记录，支持一键导出 Markdown / SRT / 纯文本，并可调用本地大模型生成会议摘要。

> 会议记录的本质是「让没参会的人获得同等上下文」——不是为了 AI，是为了**可查询、可追溯、可复用**。

---

## 🎯 核心能力

| 能力 | 说明 |
|---|---|
| 🎙️ 双路采集 | 麦克风 + 系统内录并行，物理隔离区分「本端 / 远端」说话人 |
| 🧠 多引擎可插拔 | faster-whisper（中文优先，默认）/ moonshine（英文流式）/ FunASR·Paraformer（中文流式） |
| ☁️ 云端 ASR（BYOK） | 火山引擎 / 腾讯云 / 阿里云，仅需填入 API Key，无需本地模型与 Python |
| ⚡ 低延迟 | 160ms 音频块 + 本地推理，实时字幕无感知延迟 |
| 🔒 隐私可控 | 本地引擎数据零上传；云端引擎仅在显式选择时上传音频 |
| ✨ AI 摘要 | 对接 OpenAI 兼容端点（本地 Ollama / LM Studio），生成结构化纪要 |
| 📁 一键导出 | Markdown / SRT 字幕 / 纯文本，含时间戳与说话人标记 |

---

## 🧱 技术架构

```
[麦克风 / 系统内录]
        ↓
   cpal 16kHz 采集（Rust）
        ↓
   VAD 静音检测 → 多引擎抽象 TranscriptionEngine
        ↓
   ┌─ 本地：faster-whisper / moonshine / funasr（Python 子进程）
   └─ 云端：火山引擎 / 腾讯云 / 阿里云（Rust WebSocket 流式，HMAC 签名）
        ↓
   Segment { source, speaker, text, start, end }
        ↓
   说话人归属 → Session 聚合 → Tauri 事件 → 前端实时字幕
        ↓
   MD / SRT / TXT 导出 · OpenAI 兼容摘要
```

- **桌面框架**：Tauri 2 + Rust（包体 ~10MB，内存占用低）
- **音频采集**：`cpal`（WASAPI / CoreAudio / ALSA）
- **转写引擎**：多引擎可插拔，默认 `faster-whisper`（中文 CER 业界领先）
- **云端 ASR**：原生 `tokio-tungstenite` WebSocket 客户端，火山（HMAC-SHA256）/ 腾讯（HMAC-SHA1）/ 阿里（POP 签名 + CreateToken），零 Python 依赖
- **核心库**：`moonshine-core`（UI 无关，可独立编译测试）

> 详细设计见 [`doc/ARCHITECTURE.md`](doc/ARCHITECTURE.md)，选型依据见 [`doc/竞品调研-20261026.md`](doc/竞品调研-20261026.md)。

---

## 🚀 快速上手

### 环境要求

- **Rust** ≥ 1.70（[rustup](https://rustup.rs)）
- **Node.js** ≥ 18（Tauri CLI 依赖）
- **Python** ≥ 3.9 + 至少一个转写引擎依赖
- **系统库**（Linux 需 `libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev libasound2-dev`）

### 安装

```bash
# 1. 克隆
git clone https://github.com/xiaopengs/moonshine-meeting-assistant.git
cd moonshine-meeting-assistant

# 2. 安装 Tauri CLI
cargo install tauri-cli --locked

# 3. 安装 Python 引擎依赖（默认 faster-whisper）
pip install -r python/requirements.txt
```

### 开发运行

```bash
cargo tauri dev
```

### 打包发布

```bash
cargo tauri build          # 按当前平台产出安装包（NSIS / DMG / AppImage·deb）
```

---

## 🧠 多引擎说明

### 本地引擎（隐私优先）

| 引擎 | 定位 | 中文 | 安装 |
|---|---|---|---|
| **faster-whisper** | 默认，通用首选 | **强**（large-v3 turbo） | `pip install faster-whisper` |
| **moonshine** | 英文流式，低延迟 | 弱 | `pip install moonshine-voice` |
| **FunASR / Paraformer** | 中文流式专用 | **强** | `pip install funasr`（需 torch） |

### 云端 ASR（BYOK，无需本地模型）

| 服务商 | 产品 | 鉴权方式 | 所需 Key |
|---|---|---|---|
| **火山引擎** | 豆包流式识别 2.0 | HMAC-SHA256 签名 | AccessKey ID + Secret Key + App ID |
| **腾讯云** | 实时语音识别（大模型 2.0） | HMAC-SHA1 签名 | AppID + SecretId + SecretKey |
| **阿里云** | 智能语音交互（NLS） | POP 签名 + CreateToken | AccessKey ID + AccessKey Secret + AppKey |

在 App 内「转写引擎」面板切换到云引擎，点击「云 ASR 凭证设置」填入 Key 即可使用，支持「测试连接」验证凭证；凭证仅保存在本机，不上传。

引擎在 App 内「转写引擎」面板切换；本地引擎首次使用会自动下载对应模型。

---

## 📊 竞品对比

| | 🌙 Moonshine | Meetily | VoiceStudio | Minutes | MacParakeet |
|---|---|---|---|---|---|
| 运行方式 | **完全本地** | 本地 | 本地 | 本地 | 本地 |
| 桌面框架 | **Tauri 2** | Tauri | Electron | 原生 | SwiftUI |
| 多引擎 | **●** | ● Whisper/Parakeet | ● | △ | ● |
| 系统内录 | **●** | ● | ○ | ● | ● |
| 说话人归属 | **双路物理隔离** | PRO 规划 | 取决于引擎 | 本地处理 | 依赖引擎 |
| AI 摘要 | **● 本地 LLM** | ● | 非核心 | 可选 | 可选 |
| 中文优先 | **● faster-whisper** | Whisper 路线 | 取决于模型 | 取决于模型 | WhisperKit |

> 完整调研（含 Ghost Pepper / Vexa / Jigasi / WhisperX）：[`doc/竞品调研-20261026.md`](doc/竞品调研-20261026.md)。

---

## 🗂️ 项目结构

```
moonshine-meeting-assistant/
├── Cargo.toml                  # Cargo workspace
├── crates/
│   └── core/                   # moonshine-core 核心库（UI 无关）
│       └── src/
│           ├── audio/          # cpal 双路采集
│           ├── engine/         # 引擎抽象 + 本地引擎 + remote/（火山/腾讯/阿里云 ASR）
│           ├── vad.rs          # 能量 VAD
│           ├── diarization.rs  # 说话人归属
│           ├── summary.rs      # OpenAI 兼容摘要
│           ├── export.rs       # MD/SRT/TXT
│           └── session.rs      # 会话聚合
├── src-tauri/                  # Tauri 2 应用壳
│   ├── src/{main.rs,lib.rs}    # 命令 + 事件 + 转录运行时
│   ├── tauri.conf.json
│   └── capabilities/
├── ui/                         # 前端（v2.0 设计系统，无框架）
│   ├── index.html
│   ├── styles.css
│   └── main.js
├── python/
│   ├── engine_service.py       # 多引擎 JSON 协议服务
│   └── requirements.txt
├── doc/
│   ├── ARCHITECTURE.md         # 架构设计（重构版）
│   ├── 竞品调研-20261026.md    # 竞品调研
│   ├── PRD.md / PROGRESS.md / UI-SPEC-v2.md
├── assets/                     # 图标资源
├── LICENSE                     # MIT
└── README.md
```

---

## 🤝 贡献

Issues 和 PR 欢迎！功能想法或 Bug 请提交 [Issue](https://github.com/xiaopengs/moonshine-meeting-assistant/issues)。

---

## 📄 License

[MIT](./LICENSE) — 可免费商用。

> 注意：转写引擎（faster-whisper / moonshine / FunASR）及其模型权重遵循各自许可证，商用前请逐项审计。

---

*Moonshine：让每一次会议都被认真记录。*
