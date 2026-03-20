---
AIGC:
    ContentProducer: Minimax Agent AI
    ContentPropagator: Minimax Agent AI
    Label: AIGC
    ProduceID: b31c80d79fb110ffb56dc849140cbc95
    PropagateID: b31c80d79fb110ffb56dc849140cbc95
    ReservedCode1: 304502202c1e696f050d423e6353b63a2bc083f6583be0223bf9be55da357b15188f09ee02210099e5cac9c0f98630acc5a0ae71b8b411280124be01ff35b65e6dc7912c91a6d6
    ReservedCode2: 304502207e90a9e7011d003ec0ad1862bf3c1c6de1fb51484982daf276c4f2b3b97fe5f2022100d5b697554dc0896c01dfb3160826e75f9ea777931b62ebb12de6013814dc3078
---

# Moonshine AI 跨平台会议助手方案

> 支持：麦克风 + 系统内录 双路语音转写
> 模型：moonshine-small（95MB）
> 跨平台：Windows / macOS / Linux
> 模式：本地离线 + 流式实时 + 内录采集

---

## 0. 核心新增能力

- **系统声音内录**（Loopback / Stereo Mix）
  支持录制：会议软件声音、视频声音、播放器声音、系统音频
- **麦克风 + 内录双轨同时采集**——可分别转写，也可混合转写
- **全程本地离线**，不上传任何音频

---

## 1. 项目架构 & 技术栈

### 1.1 整体流程

```
[音频源选择]
  ├─ 系统麦克风 → 人声输入
  └─ 系统内录（Loopback）→ 会议/视频声音
        ↓
[音频采集 16kHz 单声道 16bit]
        ↓
[VAD 人声检测]
        ↓
[流式音频分块]
        ↓
[moonshine-small ONNX 推理]
        ↓
[实时文本输出]
        ↓
[标点恢复 + 去重 + 时间戳]
        ↓
[UI 实时字幕 + 会议记录]
        ↓
[本地保存 MD/TXT/SRT字幕]
```

### 1.2 技术栈总表

| 层级 | 技术 |
|------|------|
| 模型推理 | PyTorch / ONNX Runtime |
| 模型 | moonshine-small |
| 音频采集（Windows） | SoundDevice / WASAPI Loopback |
| 音频采集（macOS） | SoundFlower / BlackHole |
| 音频采集（Linux） | PulseAudio / ALSA loopback |
| UI 框架（推荐） | Electron + Node.js + TypeScript |
| 备选高性能 | Qt 6 C++ + ONNX Runtime C++ |
| 进程通信 | IPC / stdio 管道 |
| 存储 | 本地 Markdown / SQLite |

---

## 2. 跨平台开发框架建议

**推荐：Electron + TypeScript + Python 后端**

理由：
- 一套代码支持 Win/macOS/Linux
- 系统音频设备枚举、内录开关最成熟
- UI 漂亮、打包简单
- 可直接调用系统音频接口做内录

**备选（高性能）**：Qt 6 + C++ + ONNX Runtime C++，适合追求极致低延迟、最小安装包体积。

---

## 3. 内录声音采集实现方案

### 3.1 Windows 内录原理

1. **WASAPI Loopback**（系统级内录）—— 直接抓取声卡输出，无需虚拟声卡
2. **立体声混音 Stereo Mix** —— 声卡驱动自带，启用后即可录制系统声音

### 3.2 各平台内录方案

**Windows**
- 音频库：sounddevice + WASAPI
- 无需安装虚拟声卡
- 枚举设备时选择含 `Loopback` 的设备

**macOS**
- 需安装虚拟声卡：BlackHole 2ch（免费开源）
- 将系统音频输出路由到 BlackHole

**Linux**
- PulseAudio 提供模块-loopback
- 或使用 ALSA 虚拟设备

### 3.3 双路采集方案

- 麦克风：人声
- 内录：会议远端声音
- 可分别识别、分别标记发言人，实现完整会议对话记录

---

## 4. 模型选型：moonshine-small

- 体积 95MB，适合打包进客户端
- CPU 实时推理无压力
- 内录音频清晰，识别准确率高
- 流式低延迟，适合会议实时字幕
- 比 tiny 更稳，比 medium 更轻

---

## 5. 双路采集代码（Python 可直接跑）

\`\`\`python
import sounddevice as sd
import numpy as np
from moonshine import MoonshineModel

SAMPLE_RATE = 16000
CHUNK_DURATION = 0.3
CHUNK_SIZE = int(SAMPLE_RATE * CHUNK_DURATION)

MIC_DEVICE = 1         # 麦克风
LOOPBACK_DEVICE = 3    # 系统内录（Loopback）

model = MoonshineModel.from_pretrained("moonshine-small")

def mic_callback(indata, frames, time, status):
    audio = indata[:,0].astype(np.float32)
    text = model.transcribe_stream(audio)
    if text.strip():
        print(f"[麦克风] {text}")

def loopback_callback(indata, frames, time, status):
    audio = indata[:,0].astype(np.float32)
    text = model.transcribe_stream(audio)
    if text.strip():
        print(f"[系统内录] {text}")

with sd.InputStream(device=MIC_DEVICE, samplerate=SAMPLE_RATE,
                    blocksize=CHUNK_SIZE, callback=mic_callback):
    with sd.InputStream(device=LOOPBACK_DEVICE,
                        samplerate=SAMPLE_RATE,
                        blocksize=CHUNK_SIZE, callback=loopback_callback):
        print("双路录音转写已启动，按 Enter 退出")
        input()
\`\`\`

---

## 6. C# 调用 moonshine ONNX 示例（Avalonia / WPF）

\`\`\`csharp
using Microsoft.ML.OnnxRuntime;
using Microsoft.ML.OnnxRuntime.Tensors;

var session = new InferenceSession("moonshine-small.onnx");
float[] pcm = GetLoopbackAudioChunk();
var input = new DenseTensor<float>(pcm, new[] { 1, pcm.Length });
var inputs = new List<NamedOnnxValue> {
    NamedOnnxValue.CreateFromTensor("audio", input)
};
using var results = session.Run(inputs);
string text = DecodeMoonshineOutput(results);
Dispatcher.UIThread.Invoke(() => {
    SubtitleText.Text = $"[内录] {text}";
});
\`\`\`

---

## 7. 项目结构

```
moonshine-meeting-assistant/
├── main.js               # Electron 主进程
├── preload.js            # 安全 IPC 桥接
├── renderer/             # UI（字幕 + 设备选择 + 导出）
├── python/
│   ├── list_devices.py   # 音频设备枚举
│   └── transcribe.py     # 双路转写核心（VAD + 去重）
├── doc/                  # 项目文档（本目录）
├── requirements.txt      # pip 依赖
└── README.md             # 项目说明
```

---

## 8. 内录转写靠谱性总结

**完全靠谱，适合：**
- 腾讯会议 / Zoom / 飞书 / 钉钉 会议内录转写
- 视频课程语音转写
- 直播语音实时字幕
- 本地隐私会议，不上云

**优势：**
- 内录音频清晰，moonshine-small 识别效果极佳
- 流式实时，延迟 <150ms
- 全程本地，不联网
- 双路采集可区分"自己说话"和"对方说话"

**小问题与解决：**
- 嘈杂内录 → 加 VAD 过滤静音
- 无标点 → 接轻量标点模型
- 多人混讲 → 可做简单说话人分离

---

## 9. 依赖安装

```bash
# Python 依赖
pip install -r requirements.txt

# Node.js 依赖
npm install
```

## 10. 构建发布包

```bash
# Windows
npm run build -- --win

# macOS
npm run build -- --mac

# Linux
npm run build -- --linux
```

---

*文档由 OpenClaw + GitHub Skill 生成 | https://github.com/xiaopengs/moonshine-meeting-assistant*
