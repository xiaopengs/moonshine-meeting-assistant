# 🌙 Moonshine 会议助手

> 支持麦克风 + 系统内录双路语音转写的跨平台 Electron 桌面应用
> 模型：[moonshine-ai/moonshine](https://github.com/moonshine-ai/moonshine)（small streaming，123M params，WER 7.84%）

---

## 快速开始

### 1. 安装 Python 依赖

```bash
pip install moonshine-voice sounddevice numpy
```

### 2. 下载模型（自动）

首次运行时会自动下载，亦可手动：

```bash
python -m moonshine_voice.download --language en
```

### 3. 查询音频设备

```bash
python python/list_devices.py
```

### 4. 启动 Electron

```bash
npm install
npm start
```

---

## 技术规格

| 项目 | 值 |
|------|---|
| 模型 | moonshine-small streaming |
| 参数量 | 123M |
| WER | 7.84%（英文） |
| Linux 推理延迟 | 165ms |
| 端到端延迟 | ~260ms（160ms chunk） |
| Python 包 | `moonshine-voice` |

---

## 项目结构

```
moonshine-meeting-assistant/
├── main.js               # Electron 主进程
├── preload.js           # IPC 安全桥接
├── renderer/
│   └── index.html       # 完整 UI（设备选择 + 实时字幕 + 导出）
├── python/
│   ├── list_devices.py  # 音频设备枚举
│   └── transcribe.py    # 双路转写核心（moonshine-voice 事件驱动）
├── doc/                 # 需求文档 / 进度
├── query/               # 决策记录
└── requirements.txt     # Python 依赖
```

---

## 架构

```
[麦克风 / Loopback] → sounddevice 采集（160ms chunk）
        ↓
[能量 VAD 过滤]
        ↓
[moonshine-voice Transcriber]
  · on_line_text_changed → 实时输出
  · on_line_completed  → 段结束
        ↓
[stdout JSON] → Electron IPC → UI 实时字幕
```

---

## Windows 内录说明

WASAPI Loopback 内录（无需虚拟声卡）：

1. 打开系统声音设置 → 声音控制面板 → 录制选项卡
2. 确认「立体声混音」或「WASAPI Loopback」已启用
3. App 中选择对应设备（自动标记 ⭐）

---

## 构建发布包

```bash
npm run build -- --win     # Windows
npm run build -- --mac     # macOS
npm run build -- --linux    # Linux
```

---

## License

MIT
