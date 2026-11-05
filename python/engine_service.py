#!/usr/bin/env python3
"""
engine_service.py — Moonshine 多引擎转写服务（stdin/stdout JSON 协议）

由 Rust 侧（moonshine-core）通过子进程调用，支持三种后端：
  - faster_whisper : faster-whisper（CTranslate2），中文优先，默认引擎
  - moonshine      : moonshine-voice，英文流式
  - funasr         : FunASR / Paraformer，中文流式专用

协议（stdin 一行一个 JSON 命令，stdout 一行一个 JSON 事件）：

  命令:
    {"cmd":"load"}                                        # 加载模型
    {"cmd":"feed","samples":"<base64 f32 little-endian>","rate":16000}
    {"cmd":"flush"}                                       # 结束一段话，输出最终文本
    {"cmd":"stop"}                                        # 退出

  事件:
    {"type":"log","msg":"..."}
    {"type":"error","msg":"..."}
    {"type":"segment","text":"...","start":0.0,"end":1.2,"final":true}
"""
import argparse
import base64
import json
import sys

import numpy as np

SAMPLE_RATE = 16000
# 累积多少秒音频后自动转写一次（准实时粒度，中文需足够上下文）
FLUSH_SECONDS = 1.0


def emit(obj):
    print(json.dumps(obj, ensure_ascii=False), flush=True)


def log(msg):
    emit({"type": "log", "msg": str(msg)})


def error(msg):
    emit({"type": "error", "msg": str(msg)})


def resample(samples: np.ndarray, from_rate: int, to_rate: int) -> np.ndarray:
    """线性重采样到目标采样率。"""
    if from_rate == to_rate or len(samples) == 0:
        return samples
    ratio = to_rate / from_rate
    out_len = int(round(len(samples) * ratio))
    idx = np.arange(out_len) / ratio
    i0 = np.floor(idx).astype(int)
    i1 = np.minimum(i0 + 1, len(samples) - 1)
    frac = (idx - i0).astype(np.float32)
    return (samples[i0] * (1 - frac) + samples[i1] * frac).astype(np.float32)


class BaseEngine:
    name = "base"

    def __init__(self, model, language):
        self.model_name = model
        self.language = language
        self._model = None

    def load(self):
        raise NotImplementedError

    def transcribe(self, audio: np.ndarray):
        """返回 [(text, start_sec, end_sec), ...]"""
        raise NotImplementedError

    def flush(self):
        """返回 flush 时的剩余 [(text, start_sec, end_sec), ...]，默认空。"""
        return []


class FasterWhisperEngine(BaseEngine):
    name = "faster_whisper"

    def load(self):
        from faster_whisper import WhisperModel

        self._model = WhisperModel(
            self.model_name or "large-v3-turbo",
            device="cpu",
            compute_type="int8",
        )

    def transcribe(self, audio: np.ndarray):
        if audio.dtype != np.float32:
            audio = audio.astype(np.float32)
        segments, _ = self._model.transcribe(
            audio,
            language=self.language or None,
            vad_filter=True,
            beam_size=5,
            initial_prompt="以下是普通话的句子。",
        )
        return [(s.text.strip(), s.start, s.end) for s in segments if s.text.strip()]


class MoonshineEngine(BaseEngine):
    name = "moonshine"

    def __init__(self, model, language):
        super().__init__(model, language)
        self._listener = _MoonshineListener()

    def load(self):
        from moonshine_voice import Transcriber

        self._model = Transcriber(
            model_path=None,
            model_arch=self.model_name or "small_streaming",
            update_interval=0.3,
        )
        self._model.add_listener(self._listener)
        self._model.start()

    def transcribe(self, audio: np.ndarray):
        # moonshine 为流式设计，这里委托给 flush 增量；本方法作为兜底。
        self._listener.reset()
        self._model.add_audio(audio, SAMPLE_RATE)
        return self._listener.drain()

    def flush(self):
        self._listener.reset()
        try:
            self._model.stop()
        except Exception:
            pass
        return self._listener.drain()


class _MoonshineListener:
    """收集 moonshine-voice 流式事件。"""

    def __init__(self):
        self._lines = []

    def reset(self):
        self._lines = []

    def drain(self):
        out = [(t, 0.0, 0.0) for t in self._lines]
        self._lines = []
        return out

    def on_line_text_changed(self, event):
        text = (event.line.text or "").strip()
        if text and (not self._lines or self._lines[-1] != text):
            self._lines.append(text)

    def on_line_started(self, event):
        pass

    def on_line_completed(self, event):
        pass


class FunasrEngine(BaseEngine):
    name = "funasr"

    def load(self):
        from funasr import AutoModel

        self._model = AutoModel(
            model=self.model_name or "paraformer-zh",
            model_revision="v2.0.4",
            vad_model="fsmn-vad",
            punc_model="ct-punc",
            disable_update=True,
        )

    def transcribe(self, audio: np.ndarray):
        if audio.dtype != np.float32:
            audio = audio.astype(np.float32)
        res = self._model.generate(input=audio, fs=SAMPLE_RATE, batch_size_s=300)
        out = []
        if isinstance(res, list):
            for item in res:
                text = (item.get("text") or "").strip() if isinstance(item, dict) else ""
                if text:
                    out.append((text, 0.0, 0.0))
        return out


def make_engine(name, model, language):
    if name == "faster_whisper":
        return FasterWhisperEngine(model, language)
    if name == "moonshine":
        return MoonshineEngine(model, language)
    if name == "funasr":
        return FunasrEngine(model, language)
    raise ValueError(f"未知引擎: {name}")


def main():
    parser = argparse.ArgumentParser(description="Moonshine 多引擎转写服务")
    parser.add_argument("--engine", default="faster_whisper")
    parser.add_argument("--model", default=None)
    parser.add_argument("--language", default=None)
    args = parser.parse_args()

    try:
        engine = make_engine(args.engine, args.model, args.language)
    except Exception as e:
        error(f"创建引擎失败: {e}")
        return

    try:
        engine.load()
        log(f"模型加载完成 · {args.engine}")
    except Exception as e:
        error(f"模型加载失败（请确认已安装对应依赖与模型）: {e}")
        return

    buffer = []
    buffered_secs = 0.0
    offset = 0.0

    for line in sys.stdin:
        line = line.strip()
        if not line:
            continue
        try:
            cmd = json.loads(line)
        except json.JSONDecodeError:
            continue

        op = cmd.get("cmd")
        if op == "feed":
            try:
                raw = base64.b64decode(cmd.get("samples", ""))
                samples = np.frombuffer(raw, dtype="<f4").copy()
                rate = int(cmd.get("rate", SAMPLE_RATE))
                if rate != SAMPLE_RATE:
                    samples = resample(samples, rate, SAMPLE_RATE)
                buffer.append(samples)
                buffered_secs += len(samples) / SAMPLE_RATE
            except Exception as e:
                error(f"feed 处理失败: {e}")
                continue

            if buffered_secs >= FLUSH_SECONDS:
                offset += _flush_buffer(engine, buffer, offset)
                buffer, buffered_secs = [], 0.0

        elif op == "flush":
            offset += _flush_buffer(engine, buffer, offset)
            buffer, buffered_secs = [], 0.0
            try:
                for text, s, e in engine.flush():
                    emit({"type": "segment", "text": text, "start": offset + s, "end": offset + e, "final": True})
            except Exception:
                pass

        elif op == "stop":
            break


def _flush_buffer(engine, buffer, offset):
    if not buffer:
        return 0.0
    audio = np.concatenate(buffer) if len(buffer) > 1 else buffer[0]
    dur = len(audio) / SAMPLE_RATE
    try:
        for text, s, e in engine.transcribe(audio):
            if text:
                emit({"type": "segment", "text": text, "start": offset + s, "end": offset + e, "final": True})
    except Exception as e:
        error(f"转写失败: {e}")
    return dur


if __name__ == "__main__":
    main()
