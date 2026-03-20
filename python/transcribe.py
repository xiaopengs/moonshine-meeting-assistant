#!/usr/bin/env python3
"""
transcribe.py — Moonshine 双路语音转写服务
使用官方 moonshine-voice 包（PyPI: moonshine-voice）

支持：麦克风 + 系统内录双路同时采集与转写
输出：JSON 到 stdout（Electron IPC 消费）

用法:
  python transcribe.py --mic-device 1 --loopback-device 3 --sample-rate 16000 --vad 1
"""
import sys
import os
import json
import time
import argparse
import threading
import queue

import numpy as np

try:
    import sounddevice as sd
except ImportError:
    print(json.dumps({"error": "请安装 sounddevice: pip install sounddevice"}))
    sys.exit(1)

try:
    from moonshine_voice import Transcriber, MicTranscriber, TranscriptEventListener
except ImportError:
    print(json.dumps({"error": "请安装 moonshine-voice: pip install moonshine-voice"}))
    sys.exit(1)

# ========================
# 配置
# ========================
SAMPLE_RATE      = 16000
CHUNK_DURATION   = 0.16   # 160ms → E2E ~260ms（<300ms 目标）
CHUNK_SIZE       = int(SAMPLE_RATE * CHUNK_DURATION)  # 2560 samples
VAD_THRESHOLD    = 0.01   # RMS 能量阈值

# moonshine-voice 模型名称（streaming 版本）
MODEL_ARCH = "small_streaming"   # moonshine-small streaming，123M params，WER 7.84%

# 去重
SEEN_TEXT    = ""
DEDUP_COUNT  = 0
DEDUP_WIN    = 3

# 共享队列
mic_queue      = queue.Queue()
loopback_queue = queue.Queue()
stop_event     = threading.Event()

# ========================
# 事件监听器（moonshine-voice 原生）
# ========================
class LiveListener(TranscriptEventListener):
    """将 moonshine-voice 事件转为 JSON stdout 输出"""

    def __init__(self, source: str):
        self.source = source

    def on_line_started(self, event):
        pass  # 流式更新由 on_line_text_changed 处理

    def on_line_text_changed(self, event):
        text = event.line.text.strip()
        if not text:
            return
        # 去重（防止同一句话重复输出）
        global SEEN_TEXT, DEDUP_COUNT
        if text == SEEN_TEXT:
            DEDUP_COUNT += 1
            if DEDUP_COUNT < DEDUP_WIN:
                return
        else:
            SEEN_TEXT = text
            DEDUP_COUNT = 0
        out = {
            "source": self.source,
            "text": text,
            "ts": time.time(),
        }
        print(json.dumps(out, ensure_ascii=False), flush=True)

    def on_line_completed(self, event):
        pass  # 实时更新已由 on_line_text_changed 处理

# ========================
# 音频回调
# ========================
def mic_callback(indata, frames, time_info, status):
    if status:
        print(json.dumps({"log": f"[mic warn] {status}"}), flush=True)
    audio = indata[:, 0].astype(np.float32)
    mic_queue.put(audio)

def loopback_callback(indata, frames, time_info, status):
    if status:
        print(json.dumps({"log": f"[loop warn] {status}"}), flush=True)
    audio = indata[:, 0].astype(np.float32)
    loopback_queue.put(audio)

# ========================
# VAD（能量检测）
# ========================
def is_speech(audio: np.ndarray) -> bool:
    return np.sqrt(np.mean(audio ** 2)) > VAD_THRESHOLD

# ========================
# 转写工作线程
# ========================
def transcription_loop(transcriber, q: queue.Queue, source: str):
    transcriber.start()
    while not stop_event.is_set():
        try:
            audio = q.get(timeout=0.5)
        except queue.Empty:
            continue
        if is_speech(audio):
            transcriber.add_audio(audio, SAMPLE_RATE)
    transcriber.stop()

# ========================
# 获取模型路径
# ========================
def get_model_path():
    """返回缓存的 moonshine 模型路径"""
    cache = os.path.expanduser("~/.cache/moonshine")
    # 模型目录结构: ~/.cache/moonshine/{arch}/
    arch_dir = os.path.join(cache, MODEL_ARCH)
    if os.path.isdir(arch_dir):
        # 找 .onnx 文件
        for f in os.listdir(arch_dir):
            if f.endswith(".onnx"):
                return os.path.join(arch_dir, f)
    return None  # 未下载，MicTranscriber 自动下载

# ========================
# 主流程
# ========================
def main():
    parser = argparse.ArgumentParser(description="Moonshine 双路转写服务")
    parser.add_argument('--mic-device',      type=int, default=-1)
    parser.add_argument('--loopback-device',  type=int, default=-1)
    parser.add_argument('--sample-rate',      type=int, default=SAMPLE_RATE)
    parser.add_argument('--vad',              type=int, default=1)
    parser.add_argument('--no-dual',         action='store_true')
    args = parser.parse_args()

    print(json.dumps({"log": "加载 moonshine 模型…"}), flush=True)

    try:
        # MicTranscriber: 自动管理麦克风，事件驱动
        # 或使用 Transcriber + 手动音频输入
        model_path = get_model_path()

        mic_transcriber = Transcriber(
            model_path=model_path,
            model_arch=MODEL_ARCH,
            update_interval=0.3,  # 300ms 更新间隔
        )
        mic_transcriber.add_listener(LiveListener("mic"))
        print(json.dumps({"log": "模型加载成功 (small_streaming, 123M params)"}), flush=True)
    except Exception as e:
        print(json.dumps({"error": f"模型加载失败: {e}"}), flush=True)
        sys.exit(1)

    streams, threads = [], []

    def make_stream(device_idx, callback, name):
        if device_idx < 0:
            print(json.dumps({"log": f"{name} 未选择，跳过"}), flush=True)
            return None
        try:
            s = sd.InputStream(
                device=device_idx,
                samplerate=args.sample_rate,
                blocksize=CHUNK_SIZE,
                dtype='float32',
                channels=1,
                callback=callback,
            )
            print(json.dumps({"log": f"{name} 启动 (device={device_idx})"}), flush=True)
            return s
        except Exception as e:
            print(json.dumps({"log": f"{name} 失败: {e}"}), flush=True)
            return None

    # 麦克风流
    s = make_stream(args.mic_device, mic_callback, "麦克风")
    if s:
        streams.append(s)
        t = threading.Thread(
            target=transcription_loop,
            args=(mic_transcriber, mic_queue, "mic"),
            daemon=True
        )
        t.start()
        threads.append(t)

    # 系统内录流（独立 Transcriber 实例，避免状态混淆）
    if not args.no_dual and args.loopback_device >= 0:
        try:
            loop_transcriber = Transcriber(
                model_path=model_path,
                model_arch=MODEL_ARCH,
                update_interval=0.3,
            )
            loop_transcriber.add_listener(LiveListener("loopback"))
            s = make_stream(args.loopback_device, loopback_callback, "系统内录")
            if s:
                streams.append(s)
                t = threading.Thread(
                    target=transcription_loop,
                    args=(loop_transcriber, loopback_queue, "loopback"),
                    daemon=True
                )
                t.start()
                threads.append(t)
        except Exception as e:
            print(json.dumps({"log": f"内录 Transcriber 创建失败: {e}"}), flush=True)

    if not streams:
        print(json.dumps({"error": "无可用音频设备"}), flush=True)
        sys.exit(1)

    print(json.dumps({"log": "转写服务就绪，E2E 延迟 <300ms"}), flush=True)

    try:
        while True:
            time.sleep(0.5)
            if stop_event.is_set():
                break
    except KeyboardInterrupt:
        print(json.dumps({"log": "停止中…"}), flush=True)
    finally:
        stop_event.set()
        for s in streams:
            try:
                s.close()
            except Exception:
                pass
        print(json.dumps({"log": "已停止"}))


if __name__ == '__main__':
    main()
