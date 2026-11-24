use super::{R, Spec, language};
use light_whisper_native_asr::{NativeAsr, Transcript};
use light_whisper_vad::FireRedVad;

// Production R2T2 has no forced maximum. After 8 s, a 300 ms pause
// within the rolling one-second VAD window can terminate a segment.
const WINDOW: usize = 16_000;
const MIN_SEGMENT: usize = 128_000;
const PAUSE: usize = 4_800;

#[derive(Default)]
struct Stream {
    context: String,
    forced_language: Option<String>,
    language: Option<String>,
    pending: Vec<f32>,
    tail: Vec<f32>,
    samples: usize,
    closed_text: String,
    current: Transcript,
}

fn join(previous: &str, next: &str) -> String {
    let mut result = previous.to_owned();
    if previous
        .as_bytes()
        .last()
        .is_some_and(|b| b.is_ascii_alphanumeric() || b".!?:;".contains(b))
        && next
            .as_bytes()
            .first()
            .is_some_and(u8::is_ascii_alphanumeric)
    {
        result.push(' ');
    }
    result.push_str(next);
    result
}

impl Stream {
    fn window(&mut self, block: &[f32]) {
        let keep = WINDOW.saturating_sub(block.len()).min(self.tail.len());
        self.tail.drain(..self.tail.len() - keep);
        self.tail
            .extend_from_slice(&block[block.len().saturating_sub(WINDOW)..]);
    }

    fn block(&mut self, native: &mut NativeAsr, vad: &mut FireRedVad, block: &[f32]) -> R<()> {
        let end = self.samples + block.len();
        let mut trailing = 0;
        if self.samples == 0 || end >= MIN_SEGMENT {
            self.window(block);
            let regions = vad.speech_timestamps(&self.tail)?;
            trailing = self.tail.len() - regions.last().map_or(0, |r| r.1.min(self.tail.len()));
            if self.samples == 0 && regions.is_empty() {
                return Ok(());
            }
        } else if end + WINDOW > MIN_SEGMENT {
            self.window(block);
        }
        let result = if self.samples == 0 {
            native.start(&self.context, self.forced_language.as_deref())?;
            self.samples = self.tail.len();
            native.feed(&self.tail)?
        } else {
            self.samples = end;
            native.feed(block)?
        };
        if !result.text.starts_with(&self.current.text) || !result.preview.starts_with(&result.text)
        {
            return Err("native R2T2 text or preview regressed".into());
        }
        if result.language.is_some() {
            self.language.clone_from(&result.language);
        }
        self.current = result;
        if self.samples >= MIN_SEGMENT && trailing >= PAUSE {
            self.close_segment(native)?;
        }
        Ok(())
    }

    fn close_segment(&mut self, native: &mut NativeAsr) -> R<()> {
        let result = native.finish()?;
        if !result.text.starts_with(&self.current.text) {
            return Err("native R2T2 text regressed".into());
        }
        if result.language.is_some() {
            self.language = result.language;
        }
        self.closed_text = join(&self.closed_text, &result.text);
        self.current = Transcript::default();
        self.samples = 0;
        self.tail.clear();
        Ok(())
    }

    fn result(&self) -> Transcript {
        Transcript {
            text: join(&self.closed_text, &self.current.text),
            preview: join(&self.closed_text, &self.current.preview),
            language: self.language.clone(),
        }
    }
}

pub(super) struct Segmented {
    native: Option<NativeAsr>,
    vad: FireRedVad,
    stream: Option<Stream>,
}
impl Segmented {
    pub(super) fn open(spec: &Spec) -> R<Self> {
        Ok(Self {
            native: Some(spec.open()?),
            vad: FireRedVad::open(&spec.resources)?,
            stream: None,
        })
    }

    pub(super) fn start(&mut self, context: &str, lang: &str) -> R<()> {
        self.cancel()?;
        self.native.as_ref().ok_or("R2T2 is suspended")?;
        self.stream = Some(Stream {
            context: context.into(),
            forced_language: language(lang),
            language: language(lang),
            ..Default::default()
        });
        Ok(())
    }

    pub(super) fn feed(&mut self, pcm: &[f32]) -> R<Transcript> {
        let native = self.native.as_mut().ok_or("R2T2 is suspended")?;
        let stream = self.stream.as_mut().ok_or("No active R2T2 stream")?;
        let chunk = native.chunk_samples();
        let mut input = pcm;
        if !stream.pending.is_empty() {
            let take = (chunk - stream.pending.len()).min(input.len());
            stream.pending.extend_from_slice(&input[..take]);
            input = &input[take..];
            if stream.pending.len() == chunk {
                let block = std::mem::take(&mut stream.pending);
                stream.block(native, &mut self.vad, &block)?;
            }
        }
        let mut blocks = input.chunks_exact(chunk);
        for block in blocks.by_ref() {
            stream.block(native, &mut self.vad, block)?;
        }
        stream.pending.extend_from_slice(blocks.remainder());
        Ok(stream.result())
    }

    pub(super) fn finish(&mut self) -> R<Transcript> {
        let native = self.native.as_mut().ok_or("R2T2 is suspended")?;
        let mut stream = self.stream.take().ok_or("No active R2T2 stream")?;
        let tail = std::mem::take(&mut stream.pending);
        if !tail.is_empty() {
            stream.block(native, &mut self.vad, &tail)?;
        }
        if stream.samples > 0 {
            stream.close_segment(native)?;
        }
        Ok(Transcript {
            text: stream.closed_text,
            language: stream.language,
            ..Default::default()
        })
    }

    pub(super) fn cancel(&mut self) -> R<()> {
        self.stream = None;
        if let Some(native) = self.native.as_mut() {
            native.reset()?;
        }
        Ok(())
    }
    pub(super) fn suspend(&mut self) -> R<()> {
        self.cancel()?;
        self.native = None;
        Ok(())
    }
    pub(super) fn resume(&mut self, spec: &Spec) -> R<()> {
        if self.native.is_none() {
            self.native = Some(spec.open()?);
        }
        Ok(())
    }
}
