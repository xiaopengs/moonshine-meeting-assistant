#!/usr/bin/env python3
"""
transcribe.py - Moonshine 双路语音转写服务
支持麦克风 + 系统内录双路同时采集与转写
"""
import sys, os, json, time, argparse, threading, queue
import numpy as np
import sounddevice as sd

try:
    from moonshine import MoonshineModel
except ImportError:
    print(json.dumps({"error": "请先安装 moonshine: pip install moonshine"}))
    sys.exit(1)

SAMPLE_RATE   = 16000
CHUNK_DURATION = 0.3
CHUNK_SIZE    = int(SAMPLE_RATE * CHUNK_DURATION)
VAD_THRESHOLD = 0.01
SEEN_TEXT     = ""
dedup_counter = 0

mic_queue      = queue.Queue()
loopback_queue = queue.Queue()
stop_event     = threading.Event()

def mic_callback(indata, frames, time_info, status):
    if status: print(json.dumps({"log":f"[mic warn] {status}"}),flush=True)
    mic_queue.put(indata[:,0].astype(np.float32))

def loopback_callback(indata, frames, time_info, status):
    if status: print(json.dumps({"log":f"[loop warn] {status}"}),flush=True)
    loopback_queue.put(indata[:,0].astype(np.float32))

def is_speech(audio):
    return np.sqrt(np.mean(audio**2)) > VAD_THRESHOLD

def transcribe_stream(model, audio, source):
    global SEEN_TEXT, dedup_counter
    if not is_speech(audio): return
    try:
        text = model.transcribe_stream(audio)
    except Exception as e:
        print(json.dumps({"log":f"[{source} err] {e}"}),flush=True)
        return
    text = text.strip()
    if not text: return
    if text == SEEN_TEXT:
        dedup_counter += 1
        if dedup_counter < 3: return
    else:
        SEEN_TEXT = text; dedup_counter = 0
    print(json.dumps({"source":source,"text":text,"ts":time.time()},ensure_ascii=False),flush=True)

def transcription_loop(model, q, source):
    while not stop_event.is_set():
        try:
            audio = q.get(timeout=0.5)
        except queue.Empty: continue
        transcribe_stream(model, audio, source)

def main():
    p = argparse.ArgumentParser()
    p.add_argument('--mic-device',     type=int, default=-1)
    p.add_argument('--loopback-device', type=int, default=-1)
    p.add_argument('--sample-rate',     type=int, default=SAMPLE_RATE)
    p.add_argument('--vad',             type=int, default=1)
    p.add_argument('--no-dual',         action='store_true')
    args = p.parse_args()

    print(json.dumps({"log":"加载模型 moonshine-small…"}),flush=True)
    try:
        model = MoonshineModel.from_pretrained("moonshine-small")
        print(json.dumps({"log":"模型加载成功"}),flush=True)
    except Exception as e:
        print(json.dumps({"error":f"模型加载失败: {e}"}),flush=True)
        sys.exit(1)

    streams, threads = [], []

    def go(device, callback, name):
        if device < 0: print(json.dumps({"log":f"{name} 未选择"}),flush=True); return None
        try:
            s = sd.InputStream(device=device, samplerate=args.sample_rate,
                               blocksize=CHUNK_SIZE, dtype='float32', channels=1, callback=callback)
            print(json.dumps({"log":f"{name} 启动 (device={device})"}),flush=True)
            return s
        except Exception as e:
            print(json.dumps({"log":f"{name} 失败: {e}"}),flush=True); return None

    s = go(args.mic_device, mic_callback, "麦克风")
    if s:
        streams.append(s)
        t = threading.Thread(target=transcription_loop, args=(model, mic_queue, "mic"), daemon=True)
        threads.append(t); t.start()

    if not args.no_dual:
        s = go(args.loopback_device, loopback_callback, "系统内录")
        if s:
            streams.append(s)
            t = threading.Thread(target=transcription_loop, args=(model, loopback_queue, "loopback"), daemon=True)
            threads.append(t); t.start()

    if not streams:
        print(json.dumps({"error":"无可用音频设备"}),flush=True); sys.exit(1)

    print(json.dumps({"log":"转写服务就绪，按 Ctrl+C 停止"}),flush=True)
    try:
        while True:
            time.sleep(0.5)
            if stop_event.is_set(): break
    except KeyboardInterrupt:
        print(json.dumps({"log":"停止中…"}),flush=True)
    finally:
        stop_event.set()
        for s in streams:
            try: s.close()
            except: pass
        print(json.dumps({"log":"已停止"}))

if __name__ == '__main__':
    main()
