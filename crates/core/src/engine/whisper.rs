//! faster-whisper 引擎后端（默认，中文优先）。

use super::{
    EngineConfig, EngineError, PythonEngineClient, Segment, Source, TranscriptionEngine,
};

/// 基于 faster-whisper（CTranslate2）的转写引擎。
///
/// 中文 WER/CER 业界领先，CPU 上 RTF 优秀；作为默认引擎。
pub struct FasterWhisperEngine {
    client: PythonEngineClient,
    source: Source,
}

impl FasterWhisperEngine {
    pub fn new(config: &EngineConfig, source: Source) -> Result<Self, EngineError> {
        let mut client = PythonEngineClient::spawn(config)?;
        client.load()?;
        Ok(Self { client, source })
    }
}

impl TranscriptionEngine for FasterWhisperEngine {
    fn id(&self) -> &'static str {
        "faster_whisper"
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
