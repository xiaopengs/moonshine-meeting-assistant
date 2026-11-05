//! 说话人分离/归属。

use crate::engine::{Segment, Source};

/// 说话人分离策略 trait，便于后续接入 pyannote 等单路多人分离方案。
pub trait Diarizer: Send {
    fn annotate(&self, segment: &mut Segment);
}

/// 双路物理隔离（默认）：麦克风 = 本端，内录 = 远端。
///
/// 零模型成本、归属最干净，是 v1 已确认的说话人分离方案。
#[derive(Debug, Clone, Copy, Default)]
pub struct DualChannelDiarizer;

impl Diarizer for DualChannelDiarizer {
    fn annotate(&self, segment: &mut Segment) {
        if segment.speaker.is_none() {
            segment.speaker = Some(match segment.source {
                Source::Mic => "本端".to_string(),
                Source::Loopback => "远端".to_string(),
            });
        }
    }
}
