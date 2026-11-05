//! moonshine 引擎后端（英文流式，低延迟）。

use super::{
    EngineConfig, EngineError, PythonEngineClient, Segment, Source, TranscriptionEngine,
};

/// 基于 moonshine-voice 的流式转写引擎。
///
/// 英文 LibriSpeech WER 7.84%，~80ms 推理延迟，适合英文会议的低延迟场景。
pub struct MoonshineEngine {
    client: PythonEngineClient,
    source: Source,
}

impl MoonshineEngine {
    pub fn new(config: &EngineConfig, source: Source) -> Result<Self, EngineError> {
        let mut client = PythonEngineClient::spawn(config)?;
        client.load()?;
        Ok(Self { client, source })
    }
}

impl TranscriptionEngine for MoonshineEngine {
    fn id(&self) -> &'static str {
        "moonshine"
    }

    fn feed(&mut self, samples: &[f32], sample_rate: u32) -> Result<Vec<Segment>, EngineError> {
        self.client.feed(samples, sample_rate)?;
        Ok(self.client.drain(self.source))
    }

    fn flush(&mut self) -> Result<Vec<Segment>, EngineError> {
        self.client.flush()?;
        Ok(self.client.drain(self.source))
    }
}
