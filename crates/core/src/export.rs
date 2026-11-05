//! 会议记录导出：Markdown / SRT / TXT。

use crate::session::Session;

/// 导出格式。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportFormat {
    Markdown,
    Srt,
    Txt,
}

impl ExportFormat {
    pub fn extension(&self) -> &'static str {
        match self {
            ExportFormat::Markdown => "md",
            ExportFormat::Srt => "srt",
            ExportFormat::Txt => "txt",
        }
    }
}

/// 将会话导出为指定格式的字符串。
pub fn export(session: &Session, format: ExportFormat) -> String {
    match format {
        ExportFormat::Markdown => export_markdown(session),
        ExportFormat::Srt => export_srt(session),
        ExportFormat::Txt => export_txt(session),
    }
}

fn export_markdown(session: &Session) -> String {
    let stats = session.stats();
    let mut md = String::new();
    md.push_str("# Moonshine 会议记录\n\n");
    md.push_str(&format!(
        "> {} · {} 条 · {} 字\n\n---\n\n",
        now_string(),
        stats.segments,
        stats.total_chars
    ));
    for seg in &session.segments {
        let speaker = seg.speaker.as_deref().unwrap_or("未标记");
        md.push_str(&format!(
            "**[{}]** `[{}]`\n\n{}\n\n---\n\n",
            speaker,
            fmt_clock(seg.start),
            seg.text.trim()
        ));
    }
    md.push_str("\n*由 Moonshine 会议助手 本地转写生成*\n");
    md
}

fn export_srt(session: &Session) -> String {
    let mut srt = String::new();
    for (i, seg) in session.segments.iter().enumerate() {
        let end = if seg.end > seg.start {
            seg.end
        } else {
            seg.start + 3.0
        };
        srt.push_str(&format!(
            "{}\n{} --> {}\n{}\n\n",
            i + 1,
            fmt_srt_time(seg.start),
            fmt_srt_time(end),
            seg.text.trim()
        ));
    }
    srt
}

fn export_txt(session: &Session) -> String {
    session
        .segments
        .iter()
        .map(|seg| {
            let speaker = seg.speaker.as_deref().unwrap_or("未标记");
            format!("[{}] [{}] {}", fmt_clock(seg.start), speaker, seg.text.trim())
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn now_string() -> String {
    // 使用系统本地时间作为导出标题，避免引入 chrono。
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let (y, mo, d) = civil_from_days((secs / 86_400) as i64);
    let rem = secs % 86_400;
    let (h, mi, s) = (rem / 3600, (rem % 3600) / 60, rem % 60);
    format!("{y:04}-{mo:02}-{d:02} {h:02}:{mi:02}:{s:02}")
}

/// 简化版公历换算（自 1970-01-01 起的天数 → 年月日）。
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let mut y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    if m <= 2 {
        y += 1;
    }
    (y, m, d)
}

/// 秒 → `HH:MM:SS`。
fn fmt_clock(secs: f32) -> String {
    let s = secs.max(0.0) as u32;
    format!("{:02}:{:02}:{:02}", s / 3600, (s % 3600) / 60, s % 60)
}

/// 秒 → SRT 时间戳 `HH:MM:SS,mmm`。
fn fmt_srt_time(secs: f32) -> String {
    let s = secs.max(0.0);
    let ms = ((s.fract()) * 1000.0).round() as u32;
    let total = s as u32;
    format!(
        "{:02}:{:02}:{:02},{:03}",
        total / 3600,
        (total % 3600) / 60,
        total % 60,
        ms
    )
}
