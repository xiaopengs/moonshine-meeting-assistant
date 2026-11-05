//! Moonshine 会议助手 · 音频设备诊断（免安装单文件）。
//!
//! 用途：在真机上定位「打开内录失败」——列出所有输入设备、逐个真实尝试打开，
//! 打印命中的配置或原始系统错误，并可采集 1 秒音频检查内录是否有声音。
//!
//! ```
//! audio-probe                 # 设备清单 + 逐设备打开测试
//! audio-probe --level         # 额外采集 1 秒电平（播放音乐时用来确认内录真的有声音）
//! audio-probe --level 3       # 采集 3 秒
//! audio-probe --json          # 机器可读输出（便于复制给开发者）
//! ```

use std::env;
use std::io::{self, Write};
use std::time::{Duration, Instant};

use moonshine_core::audio::{self, AudioCapture};

#[cfg(windows)]
fn enable_utf8_console() {
    // Windows 控制台默认代码页可能是 GBK，强制 UTF-8 避免中文乱码。
    unsafe {
        windows_sys::Win32::System::Console::SetConsoleOutputCP(65001);
    }
}

#[cfg(not(windows))]
fn enable_utf8_console() {}

fn main() {
    enable_utf8_console();

    let args: Vec<String> = env::args().skip(1).collect();
    let json_mode = args.iter().any(|a| a == "--json");
    let level_mode = args.iter().any(|a| a == "--level");
    let secs = args
        .iter()
        .position(|a| a == "--level")
        .and_then(|i| args.get(i + 1))
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(1)
        .clamp(1, 30);

    let devices = match audio::list_input_devices() {
        Ok(d) => d,
        Err(e) => {
            eprintln!("枚举输入设备失败: {e}");
            std::process::exit(1);
        }
    };

    print!("正在逐设备尝试打开输入流（失败设备会尝试多种配置，可能稍慢）... ");
    let _ = io::stdout().flush();
    let probes = match audio::probe_devices() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("\n诊断失败: {e}");
            std::process::exit(1);
        }
    };
    println!("完成\n");

    if json_mode {
        let payload = serde_json::json!({
            "host": audio::host_name(),
            "targetSampleRate": moonshine_core::audio::TARGET_SAMPLE_RATE,
            "devices": devices,
            "probes": probes,
        });
        println!("{}", serde_json::to_string_pretty(&payload).unwrap());
        return;
    }

    println!("================================================================");
    println!("  Moonshine 会议助手 · 音频设备诊断");
    println!("================================================================");
    println!("音频宿主     : {}", audio::host_name());
    println!("目标输出     : 16 kHz / 单声道 / f32（内部自动混单与重采样）");
    println!("输入设备总数 : {}\n", devices.len());

    println!("【设备清单】");
    if devices.is_empty() {
        println!("  （空）未枚举到任何输入设备");
    }
    for d in &devices {
        let tag = if d.is_loopback {
            "[明确内录]"
        } else if d.loopback_hint {
            "[疑似内录]"
        } else {
            "[普通输入]"
        };
        println!(
            "  [{:>2}] {:<10} {:<44} {}ch / {}Hz",
            d.index, tag, trunc(&d.name, 44), d.channels, d.default_rate
        );
    }

    println!("\n【逐设备打开测试】");
    for p in &probes {
        if p.ok {
            println!("  [{:>2}] OK   {} ", p.index, p.name);
            println!("       命中: {}", p.config.as_deref().unwrap_or("-"));
        } else {
            println!("  [{:>2}] FAIL {} ", p.index, p.name);
            println!("       {}", p.error.as_deref().unwrap_or("-"));
            for a in p.attempts.iter().take(6) {
                println!("         · {a}");
            }
            if p.attempts.len() > 6 {
                println!("         · ……另有 {} 条（用 --json 查看全部）", p.attempts.len() - 6);
            }
        }
    }

    if level_mode {
        println!(
            "\n【电平采样】同时打开全部可打开设备并采集 {} 秒（双路并发；请同时播放音乐/视频）",
            secs
        );
        let targets: Vec<(usize, String)> = probes
            .iter()
            .filter(|p| p.ok)
            .map(|p| (p.index, p.name.clone()))
            .collect();
        for (idx, blocks, peak, avg) in measure_all(&targets, secs) {
            let name = targets
                .iter()
                .find(|(i, _)| *i == idx)
                .map(|(_, n)| n.as_str())
                .unwrap_or("?");
            let verdict = if peak > 0.001 {
                "检测到声音"
            } else {
                "静音（若正在播放声音仍为静音 → 该设备不是真正的系统内录源）"
            };
            println!(
                "  [{:>2}] {:<28} 块数={:>4} 峰值={:.4} 均值={:.4}  {}",
                idx,
                trunc(name, 28),
                blocks,
                peak,
                avg,
                verdict
            );
        }
    }

    // 结论与建议
    let openable: Vec<&audio::DeviceProbe> = probes.iter().filter(|p| p.ok).collect();
    let strong: Vec<_> = openable.iter().filter(|p| p.is_loopback).collect();
    let weak: Vec<_> = openable.iter().filter(|p| !p.is_loopback && p.loopback_hint).collect();

    println!("\n【结论】");
    if openable.is_empty() {
        println!("  ❌ 没有任何输入设备能成功打开。请查看上方原始错误：");
        println!("     · 权限被拒 → 检查系统「麦克风隐私设置」是否允许桌面应用");
        println!("     · 设备被独占 → 关闭正在占用音频的软件（会议/播放器/录音机）");
        println!("     · 采样格式/缓冲不支持 → 已自动尝试多格式多配置，仍失败说明驱动异常");
    } else if !strong.is_empty() {
        let names: Vec<_> = strong.iter().map(|p| format!("[{}] {}", p.index, p.name)).collect();
        println!("  ✅ 找到可打开的明确内录设备：{}", names.join("、"));
        println!("     在主界面「系统内录」下拉中选择它即可。");
    } else if !weak.is_empty() {
        let names: Vec<_> = weak.iter().map(|p| format!("[{}] {}", p.index, p.name)).collect();
        println!("  🔶 未识别到标准内录设备，但以下设备命名像「输出/回环」且可打开：");
        println!("     {}", names.join("、"));
        println!("     主界面内录下拉已列出全部输入设备，可手动选它并用 --level 验证是否有声音。");
    } else {
        println!("  ⚠️ 设备都能打开，但没有内录/回环设备。下面是启用系统内录的方法。");
    }

    println!("\n【Windows 启用系统内录】");
    println!("  1. 右下角音量图标 → 右键「声音」→「录制」选项卡");
    println!("  2. 空白处右键 → 勾选「显示禁用的设备」");
    println!("  3. 启用「立体声混音 / Stereo Mix」（若无此项，用 WASAPI 回环设备，");
    println!("     即名称类似「扬声器 (Realtek…)」的输入设备，主界面可手动选）");
    println!("  4. 回到应用点「↻ 刷新设备」，再跑本工具确认");
    println!("  提示：若中文乱码，请在 Windows Terminal / PowerShell 7 运行，或先执行 chcp 65001");
    println!("================================================================");
    pause_for_double_click(json_mode);
}

/// 双击运行时保留控制台窗口，避免诊断结果一闪而过（--json / 管道场景不阻塞）。
#[cfg(windows)]
fn pause_for_double_click(json_mode: bool) {
    if json_mode {
        return;
    }
    println!("\n（诊断完成，按 Enter 键退出）");
    let _ = io::stdout().flush();
    let mut buf = String::new();
    let _ = io::stdin().read_line(&mut buf);
}

#[cfg(not(windows))]
fn pause_for_double_click(_json_mode: bool) {}

/// **同时**打开所有目标设备并采集若干秒，模拟真实「麦克风 + 系统内录」双路并发。
/// 返回每个设备的 (index, 音频块数, 峰值, 平均绝对值)。
fn measure_all(targets: &[(usize, String)], secs: u64) -> Vec<(usize, usize, f32, f32)> {
    let mut caps = Vec::new();
    for (idx, name) in targets {
        match AudioCapture::open(*idx, 2560) {
            Ok(c) => caps.push((*idx, name.clone(), c)),
            Err(e) => eprintln!("  [{idx}] 电平采样打开失败: {e}"),
        }
    }

    let mut blocks = vec![0usize; caps.len()];
    let mut peaks = vec![0f32; caps.len()];
    let mut sums = vec![0f64; caps.len()];
    let mut counts = vec![0u64; caps.len()];

    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(secs) {
        let mut got = false;
        for (k, (_, _, cap)) in caps.iter().enumerate() {
            // 一次循环内尽量排空缓冲，避免高频流积压
            while let Some(s) = cap.try_recv() {
                got = true;
                blocks[k] += 1;
                for &v in &s {
                    let a = v.abs();
                    if a > peaks[k] {
                        peaks[k] = a;
                    }
                    sums[k] += a as f64;
                    counts[k] += 1;
                }
            }
        }
        if !got {
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    caps.iter()
        .enumerate()
        .map(|(k, (idx, _, _))| {
            let avg = if counts[k] > 0 {
                (sums[k] / counts[k] as f64) as f32
            } else {
                0.0
            };
            (*idx, blocks[k], peaks[k], avg)
        })
        .collect()
}

fn trunc(s: &str, max: usize) -> String {
    let chars: Vec<char> = s.chars().collect();
    if chars.len() <= max {
        return s.to_string();
    }
    let mut out: String = chars[..max.saturating_sub(1)].iter().collect();
    out.push('…');
    out
}
