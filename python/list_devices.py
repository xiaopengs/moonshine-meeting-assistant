#!/usr/bin/env python3
"""
list_devices.py — 枚举系统音频输入设备
输出 JSON 数组，包含每个设备的 index、name、maxInputChannels、hostapi
"""
import sys
import json

try:
    import sounddevice as sd
    devices = []
    for i, d in enumerate(sd.query_devices()):
        devs = sd.query_devices(i)
        devices.append({
            "index": i,
            "name": devs["name"],
            "maxInputChannels": devs["max_input_channels"],
            "maxOutputChannels": devs["max_output_channels"],
            "hostapi": devs["hostapi"],
            "sampleRate": devs.get("default_samplerate", 0),
        })
    print(json.dumps(devices, ensure_ascii=False))
except Exception as e:
    print(json.dumps({"error": str(e)}), file=sys.stderr)
    sys.exit(1)
