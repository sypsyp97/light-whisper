//! Owns one audio.cpp session. A session is used serially by one recognition worker.
mod abi;

use abi::{Api, BackendConfig, ModelConfig, Owned, R};
use std::ffi::{CStr, CString};
use std::path::Path;
use std::ptr::{null, null_mut};

const RATE: usize = 16_000;
const FINISH_SAMPLES: usize = 5120;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Family {
    Qwen,
    R2t2,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Backend {
    Cpu,
    Cuda,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Transcript {
    pub text: String,
    pub preview: String,
    pub language: Option<String>,
}

struct Stream {
    offset: i64,
    transcript: Transcript,
}

pub struct NativeAsr {
    // Rust drops fields in declaration order: children, parents, then the library.
    session: Owned,
    _model: Owned,
    _registry: Owned,
    api: Api,
    family: Family,
    chunk: usize,
    coalesced_finish: bool,
    batch_first: bool,
    stream: Option<Stream>,
}

fn string(s: &str) -> R<CString> {
    CString::new(s).map_err(|_| "text contains an embedded NUL".into())
}

impl NativeAsr {
    pub fn open(
        library: &Path,
        model: &Path,
        family: Family,
        backend: Backend,
        threads: i32,
    ) -> R<Self> {
        if threads < 1 {
            return Err("Native threads must be a positive integer".into());
        }
        let api = Api::load(library)?;
        let qwen = family == Family::Qwen;
        let cuda = backend == Backend::Cuda;
        let path = string(model.to_str().ok_or("model path is not Unicode")?)?;
        // SAFETY: API signatures follow audiocpp.h; RAII owners exist before any out-param
        // call and outlive their dependent handles. All text/config inputs live through the call.
        unsafe {
            let minor = ((api.abi_version)() >> 8) & 255;
            let coalesced_finish = !qwen && cuda && minor >= 5;
            let mut registry = Owned::empty(api.registry_free);
            api.check(
                "registry_create",
                (api.registry_create)(null(), &mut registry.0),
            )?;
            let family_name = if qwen {
                c"qwen3_asr"
            } else {
                c"confucius4_r2t2"
            };
            let cfg = ModelConfig {
                family: family_name.as_ptr(),
                config: null(),
                weight: null(),
                spec: null(),
            };
            let mut model = Owned::empty(api.model_free);
            api.check(
                "model_load",
                (api.model_load)(
                    registry.require("registry")?,
                    path.as_ptr(),
                    &cfg,
                    null_mut(),
                    &mut model.0,
                ),
            )?;
            let options = Owned((api.options_create)(), api.options_free);
            let opt = options.require("options")?;
            if !qwen {
                let set = |key: &CStr, value: &CStr| {
                    api.check(
                        "options_set",
                        (api.options_set)(opt, key.as_ptr(), value.as_ptr()),
                    )
                };
                set(
                    c"confucius4_r2t2.chunk_size_ms",
                    if cuda { c"160" } else { c"320" },
                )?;
                set(c"confucius4_r2t2.rolling_window", c"true")?;
                set(c"confucius4_r2t2.rollback_punctuation", c"true")?;
                if coalesced_finish {
                    set(c"confucius4_r2t2.coalesced_finish", c"true")?;
                }
                if cuda && minor >= 6 {
                    set(c"confucius4_r2t2.adaptive_cadence", c"true")?;
                }
            }
            let backend_cfg = BackendConfig {
                backend: if cuda { c"cuda" } else { c"cpu" }.as_ptr(),
                device: 0,
                threads,
            };
            let mut session = Owned::empty(api.session_free);
            api.check(
                "session_create",
                (api.session_create)(
                    model.require("model")?,
                    c"asr".as_ptr(),
                    if qwen { c"offline" } else { c"streaming" }.as_ptr(),
                    &backend_cfg,
                    opt,
                    &mut session.0,
                ),
            )?;
            session.require("session")?;
            drop(options);
            Ok(Self {
                session,
                _model: model,
                _registry: registry,
                api,
                family,
                chunk: if cuda { 2560 } else { 5120 },
                coalesced_finish,
                batch_first: cuda && minor >= 4,
                stream: None,
            })
        }
    }

    pub fn chunk_samples(&self) -> usize {
        self.chunk
    }

    fn request(&self, context: &str, language: Option<&str>) -> R<Owned> {
        let context = string(context)?;
        let language = language.filter(|s| !s.is_empty()).map(string).transpose()?;
        // SAFETY: C API copies both borrowed strings. The allocation is released even on error.
        unsafe {
            let request = Owned((self.api.request_create)(), self.api.request_free);
            let req = request.require("request")?;
            self.api.check(
                "request_set_text",
                (self.api.request_set_text)(
                    req,
                    context.as_ptr(),
                    language.as_ref().map_or(null(), |s| s.as_ptr()),
                ),
            )?;
            Ok(request)
        }
    }

    pub fn transcribe(
        &mut self,
        pcm: &[f32],
        context: &str,
        language: Option<&str>,
    ) -> R<Transcript> {
        if self.family != Family::Qwen {
            return Err("offline inference requires Qwen".into());
        }
        if pcm.is_empty() || !pcm.iter().all(|v| v.is_finite()) {
            return Err("expected nonempty finite PCM".into());
        }
        let req = self.request(context, language)?;
        // SAFETY: request and contiguous PCM live through synchronous session_run.
        unsafe {
            for (key, value) in [
                (c"max_tokens", c"512"),
                (c"return_timestamps", c"false"),
                (
                    c"audio_chunk_mode",
                    if pcm.len() <= 115 * RATE {
                        c"none"
                    } else {
                        c"auto"
                    },
                ),
            ] {
                self.api.check(
                    "request_set_option",
                    (self.api.request_set_option)(req.0, key.as_ptr(), value.as_ptr()),
                )?;
            }
            self.api.check(
                "request_set_audio",
                (self.api.request_set_audio)(req.0, pcm.as_ptr(), pcm.len(), RATE as i32, 1),
            )?;
            let mut result = Owned::empty(self.api.result_free);
            self.api.check(
                "session_run",
                (self.api.session_run)(self.session.0, req.0, &mut result.0),
            )?;
            let (text, language) = self.api.text(result.0)?;
            Ok(Transcript {
                text,
                language,
                ..Default::default()
            })
        }
    }

    pub fn start(&mut self, context: &str, language: Option<&str>) -> R<()> {
        if self.family != Family::R2t2 {
            return Err("native streaming requires R2T2".into());
        }
        if self.stream.is_some() {
            self.reset()?;
        }
        let req = self.request(context, language)?;
        // SAFETY: both handles belong to this library and are live throughout start.
        self.api.check("stream_start", unsafe {
            (self.api.stream_start)(self.session.0, req.0)
        })?;
        self.stream = Some(Stream {
            offset: 0,
            transcript: Transcript {
                language: language.filter(|s| !s.is_empty()).map(str::to_owned),
                ..Default::default()
            },
        });
        Ok(())
    }

    pub fn feed(&mut self, pcm: &[f32]) -> R<Transcript> {
        let state = self.stream.as_mut().ok_or("No active R2T2 stream")?;
        if !pcm.iter().all(|v| v.is_finite()) {
            return Err("expected finite PCM".into());
        }
        let batch = self.batch_first
            && state.offset == 0
            && pcm.len() > self.chunk
            && pcm.len() <= RATE
            && pcm.len().is_multiple_of(self.chunk);
        let size = if batch { pcm.len() } else { self.chunk };
        for chunk in pcm.chunks(size) {
            let next = state
                .offset
                .checked_add(chunk.len() as i64)
                .ok_or("stream offset overflow")?;
            // SAFETY: synchronous push borrows chunk. The returned event owns result
            // and preview strings; copy all strings before freeing the event on any path.
            unsafe {
                let mut event = Owned::empty(self.api.event_free);
                self.api.check(
                    "stream_push",
                    (self.api.stream_push)(
                        self.session.0,
                        chunk.as_ptr(),
                        chunk.len(),
                        RATE as i32,
                        1,
                        state.offset,
                        &mut event.0,
                    ),
                )?;
                state.offset = next;
                if event.0.is_null() {
                    continue;
                }
                let result = (self.api.event_as_result)(event.0);
                let (delta, language) = self.api.text(result)?;
                let mut preview = null();
                let code = (self.api.result_preview_text)(result, &mut preview);
                if code != 7 {
                    self.api.check("result_preview_text", code)?;
                    state.transcript.preview = abi::copied(preview)?;
                }
                state.transcript.text.push_str(&delta);
                if !state.transcript.preview.starts_with(&state.transcript.text) {
                    return Err("R2T2 preview conflicts with committed text".into());
                }
                if language.is_some() {
                    state.transcript.language = language;
                }
            }
        }
        Ok(state.transcript.clone())
    }

    pub fn finish(&mut self) -> R<Transcript> {
        let state = self.stream.as_ref().ok_or("No active R2T2 stream")?;
        if state.offset > 0 && !self.coalesced_finish {
            self.feed(&[0.0; FINISH_SAMPLES])?;
        }
        let mut result = Owned::empty(self.api.result_free);
        // SAFETY: the session is live and active; result is owned across text extraction.
        self.api.check("stream_finish", unsafe {
            (self.api.stream_finish)(self.session.0, &mut result.0)
        })?;
        let (text, language) = self.api.text(result.0)?;
        let previous = self
            .stream
            .take()
            .and_then(|state| state.transcript.language);
        Ok(Transcript {
            text,
            language: language.or(previous),
            ..Default::default()
        })
    }

    pub fn reset(&mut self) -> R<()> {
        if self.stream.take().is_some() {
            // SAFETY: this exclusive owner retains its live session during reset.
            self.api.check("stream_reset", unsafe {
                (self.api.stream_reset)(self.session.0)
            })?;
        }
        Ok(())
    }
}
