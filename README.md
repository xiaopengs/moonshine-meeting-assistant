---
AIGC:
    ContentProducer: Minimax Agent AI
    ContentPropagator: Minimax Agent AI
    Label: AIGC
    ProduceID: 53972b06532cacbbe5c8ae94d2dddd73
    PropagateID: 53972b06532cacbbe5c8ae94d2dddd73
    ReservedCode1: 304402206d8029fa25f8ee65c56b766d058720780b783be759a5f5b4cf221ddc439e30b9022005851f1b6b98068a017d7dc6eb12d61460fadc1e481ed1635480ec7d84bf533a
    ReservedCode2: 304502201d7dcc18d11274dd9027f3220c9848e37702f9cad8f338061e31c79a7d1d6bfa022100fb4355fa0068f541c9bc660d082205cc61ff6b6441d0d702fd3e6e2d25f20cc6
---

# 🌙 Moonshine 会议助手

> 支持麦克风 + 系统内录双路语音转写的跨平台 Electron 桌面应用
> 模型：[moonshine-ai/moonshine](https://github.com/moonshine-ai/moonshine)（small streaming，123M params，WER 7.84%）

---

## 环境要求

| 环境 | 要求 |
|------|------|
| Node.js | ≥ 18.x（建议 LTS） |
| Python | ≥ 3.9（建议 3.10–3.12） |
| 操作系统 | Windows 10/11（✅已测试）、macOS（✅代码就绪）、Linux（✅代码就绪） |
| 磁盘空间 | 预留 500MB（含模型缓存 ~95MB） |
| 网络 | 仅首次运行需要（下载模型） |

---

## 安装步骤

### 第一步：克隆项目

```bash
git clone https://github.com/xiaopengs/moonshine-meeting-assistant.git
cd moonshine-meeting-assistant
```

### 第二步：安装 Node.js 依赖

```bash
npm install
```

### 第三步：安装 Python 依赖

**方式 A：直接安装（推荐）**
```bash
pip install -r requirements.txt
```

**方式 B：虚拟环境（隔离项目依赖）**
```bash
python -m venv .venv

# Windows
.venv\Scripts\activate
# macOS / Linux
source .venv/bin/activate

pip install -r requirements.txt
```

> 💡 首次运行 `npm start` 时，moonshine 模型会自动从 HuggingFace 下载（~95MB）。
> 如需手动下载：
> ```bash
> python -m moonshine_voice.download --language en
> ```

### 第四步：验证 Python 环境

```bash
python python/list_devices.py
```

**期望输出**（节选）：
```json
[
  {"index": 0, "name": "麦克风 (Realtek High Definition)", "maxInputChannels": 1, "defaultSampleRate": 48000.0},
  {"index": 1, "name": "立体声混音 (Realtek High Definition Audio)", "maxInputChannels": 2, "defaultSampleRate": 48000.0}
]
```

如果看到 `sounddevice not found` 错误，重新执行第三步。

### 第五步：启动 Electron

```bash
npm start
```

> 首次启动约需 10–30 秒（模型加载），之后会缓存模型，后续启动 < 5 秒。

---

## 使用流程

### 基础使用（仅麦克风）

1. 启动 App 后，在左上角「🎤 麦克风」下拉框选择你的麦克风设备
2. 确认左侧「本次会议」统计面板显示设备就绪
3. 点击「▶ 开始转写」
4. 说话，右侧实时字幕区域会显示转写结果
5. 会议结束后点击「■ 停止」
6. 选择导出格式（MD / SRT / TXT），点击「💾 导出」保存

### 开启系统内录（Windows）

> 无需安装虚拟声卡，Windows 10/11 自带 WASAPI Loopback。

**首次启用（一次性设置）：**
1. 右键点击任务栏 🔊 音量图标 → 「打开声音设置」
2. 页面拉到底 → 「声音控制面板」
3. 切换到「录制」选项卡
4. 右键空白处 → ☑️「显示禁用的设备」
5. 找到「**立体声混音**」或「**WASAPI Loopback**」→ 右键 → 启用 → 确定

**App 内操作：**
1. 重启 App（或点击「↻ 刷新设备」）
2. 左上角「🔊 系统内录」下拉框会出现 ⭐ 标记的设备
3. 勾选「⚙️ 选项 → 双路同时采集」
4. 点击「▶ 开始转写」→ 麦克风和会议声音**同时**转写并区分显示

### macOS 内录

> 需要安装 [BlackHole](https://existential.audio/blackhole/)（免费开源）。

**设置步骤：**
1. 安装 BlackHole：`brew install blackhole-2ch`（需 Homebrew）
2. 打开「音频 MIDI 设置」→ 创建「聚集设备」，勾选「内置麦克风 + BlackHole」
3. 将系统音频输出设置为 BlackHole
4. App 内「🔊 系统内录」选择 BlackHole 设备

---

## 开发调试

### 开发模式（含日志输出）

```bash
npm run dev
```

### 查看 Electron 日志文件

日志路径（启动后 `Help → Toggle DevTools` 或查看文件）：
```
# Windows
%USERPROFILE%\AppData\Roaming\moonshine-meeting-assistant\logs\main.log

# macOS
~/Library/Logs/moonshine-meeting-assistant/main.log

# Linux
~/.config/moonshine-meeting-assistant/logs/main.log
```

### 直接运行 Python 后端（脱离 UI 测试）

```bash
# 方式 A：交互式测试
python python/list_devices.py

# 方式 B：单次转写测试（需麦克风）
python python/transcribe.py --mic-device 0 --vad 1
```

### 导出格式说明

| 格式 | 适用场景 |
|------|---------|
| **Markdown (.md)** | 会议纪要、Notion/飞书导入 |
| **SRT (.srt)** | 视频字幕、播放器加载 |
| **纯文本 (.txt)** | 快速复制、内容分析 |

---

## 构建发布包

### Windows（输出 .exe 安装包 + 便携版）

```bash
npm run build -- --win
```

输出位置：`dist/`
- `Moonshine会议助手-Setup-*.exe` — NSIS 安装包
- `Moonshine会议助手-*.exe` — 便携版（无需安装）

### macOS

```bash
npm run build -- --mac
```

> ⚠️ macOS 构建需在 macOS 机器上运行（Electron 签名要求）。

### Linux

```bash
npm run build -- --linux
```

---

## 故障排查

### 常见错误

**错误：`sounddevice not found`**
```
# 重新安装 sounddevice
pip install --upgrade sounddevice

# 确认 Python 版本兼容
python --version   # 需 ≥ 3.9
```

**错误：`请安装 Python 依赖`**
- 确认在项目根目录执行 `pip install -r requirements.txt`
- Windows 用户建议使用 [Python 官网安装器](https://www.python.org/downloads/)，勾选 **Add Python to PATH**

**内录设备（Loopback）未出现**
1. 确认已完成「Windows 内录启用」设置步骤（重启电脑后生效）
2. 点击 App 内「↻ 刷新设备」
3. 检查设备名称是否包含：Loopback / Stereo Mix / 立体声混音 / WASAPI

**Python 进程启动失败**
```bash
# 手动验证 Python 路径
python --version
python -c "import sounddevice; print(sounddevice.query_devices())"
```
若输出设备列表，说明 Python 环境正常，问题在 App 启动脚本。

**转写延迟过高（>500ms 感知延迟）**
- 确认使用的是 `small_streaming` 模型（不是 medium）
- 检查 CPU 占用：关闭其他大型程序
- VAD 已开启（默认），静音时段会自动跳过

**打包体积过大（>200MB）**
- 正常，包含 moonshine 模型（95MB）+ Electron 框架
- 后续版本计划：PyInstaller 分离打包减少体积

---

## 技术规格

| 项目 | 值 |
|------|---|
| 模型 | moonshine-small streaming |
| 参数量 | 123M |
| WER | 7.84%（英文） |
| 内录音质 | 16kHz 单声道 |
| chunk 大小 | 160ms（2560 samples）|
| 推理延迟 | ~80ms（Linux 实测）|
| 端到端延迟 | **< 300ms**（含 VAD + IPC）|
| Python 包 | `moonshine-voice` `sounddevice` `numpy` |

---

## 项目结构

```
moonshine-meeting-assistant/
├── main.js                  # Electron 主进程（窗口 / IPC / Python 进程管理）
├── preload.js               # contextBridge 安全桥接（renderer ↔ main）
├── renderer/
│   └── index.html           # 完整 UI（设备选择 / 实时字幕 / 统计 / 导出）
├── python/
│   ├── list_devices.py      # 音频设备枚举（sounddevice.query_devices）
│   └── transcribe.py        # 双路转写核心（VAD + moonshine Transcriber）
├── assets/                  # 应用图标（打包用）
│   ├── icon.png             # 256px 主图标
│   └── icon.ico             # Windows 多尺寸图标
├── doc/                     # 项目文档
│   ├── PRD.md               # 产品需求文档
│   ├── PROGRESS.md          # 里程碑进度
│   └── Moonshine_AI_跨平台会议助手方案.md
├── query/                   # 决策记录
│   ├── 决策记录-20260320.md
│   └── 选型三刀-目标边界定义.md
├── package.json             # Node.js 依赖 + 构建配置
├── requirements.txt         # Python 依赖
└── README.md                # 本文件
```

---

## 数据流

```
[物理麦克风 / 系统内录设备]
        ↓
[ sounddevice 采集 16kHz 单声道 160ms chunk ]
        ↓
[ VAD 能量阈值过滤（去除静音）]
        ↓
[ moonshine-small streaming 转写 ~80ms ]
        ↓
[ JSON stdout → Electron IPC → webContents.send ]
        ↓
[ renderer/index.html 实时渲染字幕 + 动画 ]
        ↓
[ 用户导出 → dialog.showSaveDialog → .md / .srt / .txt ]
```

---

## 截图预览（v2.0 UI）

```
┌─────────────────────────────────────────────────────────────────┐
│ 🌙 Moonshine                              ⌘R 开始 ⌘S 停止  [●] 就绪 │
├──────────────┬──────────────────────────────┬────────────────────┤
│ 🎤 输入设备   │  实时字幕              42条  │ ✨ 会议洞察         │
│ 麦克风    ▼  │ ┌────────────────────────┐   │ 📝 实时摘要        │
│ 麦克风名称   │ │ 🎤 麦克风  10:32:15    │   │ 正在生成摘要...    │
│  ✔ 就绪     │ │  "我们这个季度的主要   │   │ 🔑 关键词          │
│             │ │   目标是提升留存率..." │   │ [产品迭代][Q2目标] │
│ 🔊 系统内录  │ └────────────────────────┘   │ 🎙️ 比例            │
│  未检测到    │                              │ ████████░░ 麦克风  │
│  ⚠ 警告     │                              │ ⚡ 语速 186字/分   │
│ 〰️ 音频电平  │                              └────────────────────┘
│ [波形可视化] │                              [▶] 左侧面板可折叠
│ 📊 本次会议  │
│ 00:45  3    │                                                      │
│ ⚙️ 选项     │                                                      │
│ [VAD] [双路]│                                                      │
│ ⏺ 开始录制 │                                                      │
└──────────────┴──────────────────────────────────────────────────┘
```



MIT
