#!/usr/bin/env python3
"""把 ui/ 三件套内联成单文件界面预览（注入 mock Tauri API）。

用法：
    python3 scripts/build-ui-preview.py            # 输出 doc/界面预览.html
    python3 scripts/build-ui-preview.py out.html   # 自定义输出路径

用途：在不启动 Tauri 的前提下，用浏览器直接查看完整三栏交互形态。
数据为模拟值（含「系统声音」采集源、云 ASR 服务商、音频诊断结果），不会真实录音。
"""

import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
UI = ROOT / "ui"

MOCK = r"""<script>
/* 界面预览用 Tauri API mock：真实桌面应用由 src-tauri 提供同名命令 */
(() => {
  const listeners = {};
  let timer = null, volTimer = null;
  const emit = (n, p) => (listeners[n] || []).forEach((cb) => cb({ payload: p }));

  const DEVICES = [
    { index: 0, name: '麦克风 (Realtek(R) Audio)', channels: 2, defaultRate: 44100, isLoopback: false, loopbackHint: false, kind: 'input' },
    { index: 1, name: '麦克风阵列 (英特尔\u00ae 智音技术)', channels: 2, defaultRate: 48000, isLoopback: false, loopbackHint: false, kind: 'input' },
    { index: 2, name: '扬声器 (Realtek(R) Audio)（系统声音）', channels: 2, defaultRate: 48000, isLoopback: true, loopbackHint: true, kind: 'loopback' },
    { index: 3, name: '耳机 (WH-1000XM5)（系统声音）', channels: 2, defaultRate: 48000, isLoopback: true, loopbackHint: true, kind: 'loopback' }
  ];

  const PROBES = {
    host: 'Wasapi', targetSampleRate: 16000,
    probes: [
      { index: 0, name: '麦克风 (Realtek(R) Audio)', isLoopback: false, loopbackHint: false, kind: 'input', ok: true,
        config: '2ch/44100Hz/Default/F32 \u2192 内部混单/重采样为 16kHz 单声道 f32', error: null, attempts: [] },
      { index: 1, name: '麦克风阵列 (英特尔\u00ae 智音技术)', isLoopback: false, loopbackHint: false, kind: 'input', ok: true,
        config: '2ch/48000Hz/Default/F32 \u2192 内部混单/重采样为 16kHz 单声道 f32', error: null, attempts: [] },
      { index: 2, name: '扬声器 (Realtek(R) Audio)（系统声音）', isLoopback: true, loopbackHint: true, kind: 'loopback', ok: true,
        config: 'WASAPI 回环 OK：2ch/48000Hz/32-bit float \u2192 内部混单/重采样为 16kHz 单声道 f32', error: null, attempts: [] },
      { index: 3, name: '耳机 (WH-1000XM5)（系统声音）', isLoopback: true, loopbackHint: true, kind: 'loopback', ok: false, config: null,
        error: '回环初始化失败（设备可能被独占模式占用，或驱动不支持 loopback）: 0x8889000A 设备正在使用中', attempts: [] }
    ]
  };

  const PROVIDERS = [
    { id: 'volcengine', name: '火山引擎', needsKey: true,
      keyFields: [
        { key: 'accessKeyId', label: 'AccessKey ID', placeholder: 'AKLT...', type: 'text' },
        { key: 'secretKey', label: 'Secret Key', placeholder: '火山引擎 SecretKey', type: 'password' },
        { key: 'appId', label: 'App ID', placeholder: '数字应用 ID', type: 'text' }],
      models: [{ id: 'bigmodel', name: '豆包流式语音识别 2.0（中文）' }],
      console: 'https://console.volcengine.com/speech/app', consoleLabel: '火山引擎 · 语音技术控制台',
      docUrl: 'https://www.volcengine.com/docs/6561/1354869',
      keyGuide: '创建应用并勾选「流式语音识别大模型」，取得 APP ID 与 AK/SK。' },
    { id: 'tencent', name: '腾讯云', needsKey: true,
      keyFields: [
        { key: 'appId', label: 'AppID', placeholder: '例如 1250000000', type: 'text' },
        { key: 'secretId', label: 'SecretId', placeholder: 'AKID...', type: 'text' },
        { key: 'secretKey', label: 'SecretKey', placeholder: '腾讯云 SecretKey', type: 'password' }],
      models: [{ id: '16k_zh_en', name: '中英大模型 2.0（会议推荐）' }],
      console: 'https://console.cloud.tencent.com/asr', consoleLabel: '腾讯云 · 语音识别控制台',
      docUrl: 'https://cloud.tencent.com/document/product/1093/131127',
      keyGuide: '开通语音识别后，在「API 密钥管理」新建密钥，取得 AppID、SecretId、SecretKey。' },
    { id: 'alicloud', name: '阿里云', needsKey: true,
      keyFields: [
        { key: 'accessKeyId', label: 'AccessKey ID', placeholder: 'LTAI...', type: 'text' },
        { key: 'accessKeySecret', label: 'AccessKey Secret', placeholder: '阿里云 AccessKeySecret', type: 'password' },
        { key: 'appKey', label: 'AppKey', placeholder: '项目 AppKey', type: 'text' }],
      models: [{ id: 'domain-meeting-domain', name: '会议领域（中文）' }],
      console: 'https://nls-portal.console.aliyun.com/', consoleLabel: '阿里云 · 智能语音交互控制台',
      docUrl: 'https://help.aliyun.com/knowledge_detail/72214.html',
      keyGuide: '开通 NLS → 创建 AccessKey → 创建项目取 Appkey。' }
  ];

  const LINES = [
    ['loopback', '我们先过一下这周的进度，重点看新架构落地到什么程度。'],
    ['mic', '核心库已经拆出来了，双路采集也跑通了，本地引擎和云引擎共用同一套抽象。'],
    ['loopback', '内录这块之前一直打不开，现在定位到原因了吗？'],
    ['mic', '确认了，是 WASAPI 回环的问题——系统声音不是输入设备，得拿输出设备加 loopback 标志开流。'],
    ['loopback', '那现在设备列表里能直接看到「系统声音」了吗？'],
    ['mic', '能，麦克风在上面，扬声器和耳机在下面单独标成系统声音，选一下就能录。'],
    ['loopback', '云 ASR 三家都接好了？'],
    ['mic', '火山、腾讯云、阿里云都接了，填 Key 就能用，本地不用装 Python 也不用下模型。']
  ];
  let cursor = 0;

  window.__TAURI__ = {
    core: {
      invoke: async (cmd) => {
        switch (cmd) {
          case 'get_version': return '2.0.0';
          case 'list_devices': return DEVICES;
          case 'probe_audio_devices': return PROBES;
          case 'get_asr_providers': return PROVIDERS;
          case 'load_asr_settings': return {};
          case 'save_asr_settings': return null;
          case 'test_asr_connection': return null;
          case 'open_external_url': return null;
          case 'start_transcription': {
            emit('status', 'running');
            emit('log', '[mock] 界面预览模式：以下字幕为模拟数据');
            cursor = 0;
            timer = setInterval(() => {
              const [source, text] = LINES[cursor % LINES.length];
              cursor += 1;
              emit('transcript', { source, text, isFinal: true, ts: Date.now() });
            }, 2600);
            volTimer = setInterval(() => emit('volume', { volume: 0.15 + Math.random() * 0.6 }), 120);
            return null;
          }
          case 'stop_transcription': {
            clearInterval(timer); clearInterval(volTimer);
            emit('volume', { volume: 0 }); emit('status', 'stopped');
            return null;
          }
          case 'export_session': return '# 会议记录\n\n（界面预览：导出内容为模拟数据）\n';
          case 'save_transcript': return 'C:/Users/you/Documents/meeting.md';
          case 'summarize': return '## 摘要\n\n- 新架构核心库已拆分完成，双路采集跑通\n- 内录问题定位为 WASAPI 回环\n- 三家云 ASR 均已接入\n';
          default: return null;
        }
      }
    },
    event: {
      listen: async (name, cb) => {
        (listeners[name] = listeners[name] || []).push(cb);
        return () => { listeners[name] = (listeners[name] || []).filter((f) => f !== cb); };
      }
    }
  };
})();
</script>
"""


def main() -> None:
    out_path = Path(sys.argv[1]) if len(sys.argv) > 1 else ROOT / "doc" / "界面预览.html"

    html = (UI / "index.html").read_text(encoding="utf-8")
    css = (UI / "styles.css").read_text(encoding="utf-8")
    js = (UI / "main.js").read_text(encoding="utf-8")

    for name, text in (("main.js", js), ("styles.css", css)):
        if "</script" in text.lower() or "</style" in text.lower():
            raise SystemExit(f"{name} 含闭合标签，无法安全内联")

    html = html.replace('<link rel="stylesheet" href="styles.css"/>', "<style>\n" + css + "\n</style>")
    html = html.replace(
        '<script type="module" src="main.js"></script>',
        MOCK + "<script type=\"module\">\n" + js + "\n</script>",
    )

    if "styles.css" in html or "main.js" in html:
        raise SystemExit("内联失败：仍存在外部引用")

    out_path.parent.mkdir(parents=True, exist_ok=True)
    out_path.write_text(html, encoding="utf-8")
    print(f"已生成 {out_path} ({out_path.stat().st_size / 1024:.1f} KB)")


if __name__ == "__main__":
    main()
