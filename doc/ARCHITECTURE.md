# Moonshine 会议助手 — 架构设计文档（重构版）

> 版本：v2.0.0
> 更新：2026-10-26
> 状态：**深度重构进行中**
> 关联调研：`doc/竞品调研-20261026.md`

---

## 一、重构动机

原 v1 架构为 **Electron 28 + Python 子进程（moonshine-voice + sounddevice）**，存在以下结构性缺陷：

| # | 问题 | 影响 | 重构对策 |
|---|---|---|---|
| 1 | `main.js` 第 12 行存在语法错误（多余的右括号） | 应用无法启动 | 整体重写为 Rust，消除该类错误 |
| 2 | 单文件堆叠（UI 1461 行、主进程 200+ 行） | 可维护性差、难以测试 | 模块化 workspace + 独立核心库 |
| 3 | 推理引擎单一（moonshine 以英文见长） | 中文会议 WER 偏高 | 多引擎可插拔，默认接入 faster-whisper |
| 4 | 说话人分离/AI 摘要仍是 Mock/规划态 | 核心价值未落地 | 双路物理隔离 + 可配置摘要端点 |
| 5 | 无 LICENSE 文件（README 标 MIT 但仓库 license=null） | 开源合规风险 | 补全 MIT LICENSE |
| 6 | Electron 包体/内存开销大 | 启动慢、占用高 | 迁移 Tauri 2 + Rust |

**重构目标**：在保留「本地隐私 + 双路采集 + 实时字幕」产品定位的前提下，将工程底座替换为 **Tauri 2 + Rust + 多引擎可插拔转写管线**，对齐竞品 Meetily（Tauri + Rust）与 MacParakeet（公开基准、中文优先）的工程实践。

---

## 二、总体架构

```
┌─────────────────────────────────────────────────────────────┐
│                         前端 UI（ui/）                        │
│   三栏布局 · VAD 波形 · 设备选择 · 字幕流 · 摘要 · 导出        │
│        (HTML + Vanilla JS + Tauri `invoke`/`listen`)          │
└───────────────────────────┬─────────────────────────────────┘
                            │ Tauri IPC（command / event）
┌───────────────────────────▼─────────────────────────────────┐
│                     Tauri 2 应用壳（src-tauri/）               │
│   main.rs / lib.rs — 命令注册、事件派发、状态管理、子进程生命周期 │
└───────────────────────────┬─────────────────────────────────┘
                            │ 调用
┌───────────────────────────▼─────────────────────────────────┐
│               moonshine-core 核心库（crates/core/）            │
│                                                              │
│  ┌──────────┐  ┌──────────┐  ┌──────────┐  ┌─────────────┐  │
│  │ audio     │  │ engine    │  │ diarize   │  │ summary      │  │
│  │ 双路采集   │→│ 引擎抽象   │→│ 说话人归属 │→│ 会议纪要生成  │  │
│  │ (cpal)    │  │ (trait)   │  │ 双路/可选  │  │ (OpenAI 兼容)│  │
│  └──────────┘  └──────────┘  └──────────┘  └─────────────┘  │
│         ↓            ↓              ↓                          │
│  ┌──────────┐  ┌──────────┐  ┌─────────────┐                  │
│  │ vad       │  │ export    │  │ session      │               │
│  │ 能量/静音  │  │ MD/SRT/TXT│  │ 会话状态聚合  │               │
│  └──────────┘  └──────────┘  └─────────────┘                  │
└───────────────────────────┬─────────────────────────────────┘
                            │ 子进程 / JSON 协议
┌───────────────────────────▼─────────────────────────────────┐
│           Python 引擎服务（python/engine_service.py）          │
│  faster-whisper (CTranslate2) · moonshine-voice · FunASR       │
└─────────────────────────────────────────────────────────────┘
```

---

## 三、技术选型

| 层 | 技术 | 选型理由 |
|---|---|---|
| 桌面框架 | **Tauri 2** | 包体 ~10MB（vs Electron ~100MB）、内存更低、Rust 原生安全；对齐 Meetily |
| 后端语言 | **Rust** | 零成本抽象、内存安全、跨平台（WASAPI/CoreAudio/ALSA 统一封装） |
| 音频采集 | **cpal** | Rust 跨平台音频库，原生支持 Windows WASAPI Loopback、macOS CoreAudio、Linux ALSA |
| VAD | 能量检测（默认）+ silero-vad（可选） | 能量检测零依赖低延迟；预留神经网络 VAD 接口 |
| 转写引擎 | **多引擎可插拔**（见下） | 中文会议默认 faster-whisper，兼顾精度与生态 |
| 说话人分离 | 双路物理隔离（默认）+ pyannote（可选） | 双路物理隔离零模型成本、最干净；pyannote 用于单路多人 |
| 摘要 | OpenAI 兼容端点（本地 Ollama / 可选云端） | 隐私优先默认本地，接口统一可切换 |
| 导出 | MD / SRT / TXT | 沿用 v1 产品定义，结构化输出 |
| 前端 | HTML + Vanilla JS | 无框架依赖，复用 v2.0 UI 设计系统 |

---

## 四、多引擎可插拔设计（核心）

统一 `TranscriptionEngine` trait，引擎在配置中声明，运行时按需实例化：

```rust
pub trait TranscriptionEngine: Send {
    fn id(&self) -> &'static str;
    fn ready(&self) -> bool;
    /// 流式送入音频 chunk，返回新增/修正的文本片段
    fn feed(&mut self, samples: &[f32], sample_rate: u32) -> Result<Vec<Segment>>;
    /// 结束一段话，返回最终文本
    fn flush(&mut self) -> Result<Vec<Segment>>;
}
```

| 引擎 | 后端方式 | 中文能力 | 依赖 | 默认 |
|---|---|---|---|---|
| **faster-whisper** | Python 子进程（CTranslate2） | **强**（large-v3 turbo 中文 CER 业界领先） | `faster-whisper` | ✅ |
| **moonshine** | Python 子进程（moonshine-voice） | 弱（英文见长，WER 7.84% 为 LibriSpeech） | `moonshine-voice` | — |
| **funasr** | Python 子进程（Paraformer） | **强**（中文流式专用） | `funasr` | — |
| **volcengine（火山）** | Rust WebSocket 流式（HMAC-SHA256） | **强**（豆包流式 2.0） | 仅 API Key（BYOK） | — |
| **tencent（腾讯云）** | Rust WebSocket 流式（HMAC-SHA1） | **强**（大模型 2.0，可选说话人分离） | 仅 API Key（BYOK） | — |
| **alicloud（阿里云）** | Rust WebSocket（POP 签名 + CreateToken） | **强**（会议/客服领域模型） | 仅 API Key（BYOK） | — |

> 引擎选择依据：MacParakeet 公开基准显示 Whisper large-v3 turbo 中文 CER 优于 Parakeet v3；faster-whisper 的 CTranslate2 在 CPU 上 RTF 优秀。故默认 faster-whisper，保留 moonshine 与 FunASR/Paraformer 作为可选后端。

### 4.1 云端 ASR（BYOK 模式，参考 coretab）

本地引擎需下载模型并依赖 Python，云端 ASR 只需用户在各云控制台开通语音识别并填入 API Key（**Bring Your Own Key**），零模型、零 Python 依赖。三家均为 **16kHz / 16bit / 单声道 PCM** 的 WebSocket 流式协议，由 Rust 原生 `tokio-tungstenite` 客户端驱动，签名在客户端完成（`crates/core/src/engine/remote/`）：

| 服务商 | 端点 | 鉴权 | 握手 |
|---|---|---|---|
| 火山引擎 | `wss://openspeech.bytedance.com/api/v3/sauc/bigmodel` | HMAC-SHA256（`ak\nts\nsessionId`） | 连接后发 init JSON |
| 腾讯云 | `wss://asr.cloud.tencent.com/asr/v2/{appid}` | HMAC-SHA1（字典序参数） | 首条 `code=0` |
| 阿里云 | `wss://nls-gateway-cn-shanghai.aliyuncs.com/ws/v1` | POP 签名换 Token（`CreateToken`） | `StartTranscription` → `TranscriptionStarted` |

实现要点：云引擎与本地引擎共用同一 `TranscriptionEngine` trait——后台线程 + tokio runtime 驱动 WebSocket，`feed()` 把 f32 样本转 PCM 16bit 发送并 `drain` 已到达结果，`flush()` 发送结束指令（腾讯 `{"type":"end"}` / 阿里 `StopTranscription`）并等待最终结果。凭证经 `save_asr_settings` 持久化到本机配置目录（`asr-settings.json`），`test_asr_connection` 做握手验证。

**子进程协议**（stdin/stdout JSON，一行一事件，沿用 v1 已验证思路，仅本地引擎使用）：

```
→ {"cmd":"load","engine":"faster_whisper","model":"large-v3-turbo","lang":"zh"}
← {"type":"log","msg":"model loaded"}
→ {"cmd":"feed","samples":"<base64 f32 le>","rate":16000}
← {"type":"segment","text":"...","start":0.0,"end":1.2,"partial":true}
← {"type":"segment","text":"...","final":true}
→ {"cmd":"flush"}
```

---

## 五、数据流

```
[麦克风 cpal 流] ──┐
                   ├─→ VAD 过滤 ──→ TranscriptionEngine.feed()
[内录 cpal 流] ───┘                      │
                                         ↓
                              Segment { source, text, start, end }
                                         │
                       ┌─────────────────┼─────────────────┐
                       ↓                 ↓                 ↓
                 Session 聚合      说话人归属(双路)      实时字幕事件
                       │                 │                 │
                       ↓                 ↓                 ↓
                 MD/SRT/TXT 导出   说话人标签         Tauri event → UI
```

端到端延迟目标沿用 v1 决策：**< 300ms**（160ms chunk + ~80ms 推理 + 余量）。

---

## 六、项目结构

```
moonshine-meeting-assistant/
├── Cargo.toml                  # Cargo workspace
├── crates/
│   └── core/                   # moonshine-core 核心库（UI 无关，可独立测试）
│       └── src/
│           ├── lib.rs
│           ├── audio/          # cpal 双路采集
│           │   ├── mod.rs
│           │   └── capture.rs
│           ├── vad.rs          # 能量 VAD + 可选 silero
│           ├── engine/         # 引擎抽象 + 本地后端 + 云端 ASR
│           │   ├── mod.rs
│           │   ├── whisper.rs
│           │   ├── moonshine.rs
│           │   ├── funasr.rs
│           │   └── remote/     # 火山引擎 / 腾讯云 / 阿里云（WebSocket + 签名）
│           │       ├── mod.rs  #   通用流式会话框架
│           │       ├── sign.rs #   HMAC-SHA256/SHA1 + 阿里 POP 签名
│           │       ├── volcengine.rs
│           │       ├── tencent.rs
│           │       └── alicloud.rs
│           ├── diarization.rs  # 双路归属 + 可选 pyannote
│           ├── summary.rs      # OpenAI 兼容摘要
│           ├── export.rs       # MD/SRT/TXT
│           └── session.rs      # 会话状态聚合
├── src-tauri/                  # Tauri 2 应用壳
│   ├── Cargo.toml
│   ├── tauri.conf.json
│   ├── build.rs
│   ├── capabilities/default.json
│   ├── icons/
│   └── src/
│       ├── main.rs
│       └── lib.rs              # command + event + 状态
├── ui/                         # 前端（v2.0 设计延续）
│   ├── index.html
│   ├── main.js
│   └── styles.css
├── python/                     # Python 引擎服务
│   ├── engine_service.py
│   └── requirements.txt
├── assets/                     # 图标资源（沿用）
├── doc/                        # 文档（含旧 PRD/PROGRESS/UI-SPEC）
│   ├── ARCHITECTURE.md         # 本文档
│   └── 竞品调研-20261026.md
├── query/                      # 历史决策记录（沿用）
├── LICENSE                     # MIT（新增）
└── README.md
```

---

## 七、关键决策记录

| 决策项 | 结论 | 依据 |
|---|---|---|
| 桌面框架 | Tauri 2 + Rust（弃 Electron） | Meetily 路线；包体/内存/安全 |
| 推理引擎 | 多引擎可插拔，默认 faster-whisper | 中文 CER 领先（MacParakeet 基准） |
| 引擎通信 | Python 子进程 + JSON stdout | 复用成熟 Python ASR 生态，崩溃隔离 |
| 说话人分离 | 双路物理隔离（默认） | 零模型成本、最干净；v1 已确认 |
| 摘要 | OpenAI 兼容端点，默认本地 Ollama | 隐私优先，接口统一可切换 |
| 前端 | Vanilla JS，无框架 | 复用 v2.0 UI 设计系统，零构建 |

---

## 八、迁移路径与验证计划

1. ✅ 拆除 Electron（main.js / preload.js / renderer / 旧 python）
2. ✅ 搭建 workspace + moonshine-core 核心库
3. ✅ 实现 Tauri 2 应用壳 + 前端
4. ⏳ 编译验证（`cargo check` / `cargo build`）
5. ⏳ 真机验证（需 Windows/macOS 真实音频设备）：
   - 中文会议 WER/CER 复测（faster-whisper vs moonshine vs FunASR）
   - 系统内录 + 麦克风双路采集的设备兼容性、回声、采样率
   - 长会稳定性（2 小时、断电恢复、内存占用）
6. ⏳ 打包发布（Tauri bundle：Windows NSIS / macOS DMG / Linux AppImage）

> 沙箱为 Linux 无音频设备，步骤 4 可在本环境完成，步骤 5/6 需在真机执行。
