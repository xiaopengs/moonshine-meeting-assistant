//! FunASR / Paraformer 引擎后端（中文流式专用）。

use super::{
    EngineConfig, EngineError, PythonEngineClient, Segment, Source, TranscriptionEngine,
};

/// 基于 FunASR（Paraformer）的中文流式转写引擎。
///
/// 中文流式专用，可复用本地已下载的 Paraformer 模型；适合对中文识别有更高要求的场景。
pub struct FunasrEngine {
    client: PythonEngineClient,
    source: Source,
}

impl FunasrEngine {
    pub fn new(config: &EngineConfig, source: Source) -> Result<Self, EngineError> {
        let mut client = PythonEngineClient::spawn(config)?;
        client.load()?;
        Ok(Self { client, source })
    }
}

impl TranscriptionEngine for FunasrEngine {
    fn id(&self) -> &'static str {
        "funasr"
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
