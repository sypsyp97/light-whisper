//! Native streaming IPC client for the resident R2T2 server.

use super::{
    profile_hot_words, send_command_to_server, ServerCommand, ServerResponse, TranscriptionResult,
};
use crate::state::AppState;
use crate::utils::AppError;
use tauri::AppHandle;

#[derive(Debug)]
pub(crate) struct NativeStreamUpdate {
    pub(crate) tentative_text: String,
    pub(crate) text: String,
    pub(crate) language: Option<String>,
    pub(crate) sample_count: usize,
    pub(crate) is_final: bool,
}

pub(super) fn validate_stream_response(
    response: ServerResponse,
    session_id: u64,
    expected_samples: usize,
    expected_final: bool,
    previous_text: &str,
) -> Result<NativeStreamUpdate, AppError> {
    if response.success != Some(true) {
        return Err(stream_error("stream response reported failure"));
    }

    let actual_session_id = response
        .session_id
        .ok_or_else(|| stream_error("stream response missing session identity"))?;
    if actual_session_id != session_id {
        return Err(stream_error("stream response session identity mismatch"));
    }

    let sample_count = response
        .sample_count
        .ok_or_else(|| stream_error("stream response missing sample count"))?;
    if sample_count != expected_samples {
        return Err(stream_error("stream response sample count mismatch"));
    }

    let is_final = response
        .is_final
        .ok_or_else(|| stream_error("stream response missing final state"))?;
    if is_final != expected_final {
        return Err(stream_error("stream response final state mismatch"));
    }

    let text = response
        .text
        .ok_or_else(|| stream_error("stream response missing text"))?;
    if !text.starts_with(previous_text) {
        return Err(stream_error("stream response text regressed"));
    }

    let tentative_text = response.tentative_text.unwrap_or_default();
    if is_final && !tentative_text.is_empty() {
        return Err(stream_error("final response contained tentative text"));
    }
    Ok(NativeStreamUpdate {
        tentative_text,
        text,
        language: response.language.filter(|language| !language.is_empty()),
        sample_count,
        is_final,
    })
}

fn stream_error(message: &str) -> AppError {
    AppError::Asr(format!("R2T2 stream error: {message}"))
}

pub(crate) struct NativeStreamClient {
    pub(crate) session_id: u64,
    pub(crate) sample_count: usize,
    pub(crate) committed_text: String,
    tentative_text: String,
    pub(crate) language: Option<String>,
    closed: bool,
}

impl NativeStreamClient {
    pub(crate) async fn start(
        state: &AppState,
        app: Option<&AppHandle>,
        session_id: u64,
        context: String,
        language: Option<String>,
    ) -> Result<Self, AppError> {
        if session_id == 0 {
            return Err(stream_error("session id must be positive"));
        }

        let response = send_command_to_server(
            state,
            &ServerCommand::StreamStart {
                session_id,
                context,
                hot_words: profile_hot_words(state),
                language: language.clone(),
            },
            app,
        )
        .await?;
        let update = validate_stream_response(response, session_id, 0, false, "")?;

        Ok(Self {
            session_id,
            sample_count: update.sample_count,
            committed_text: update.text,
            tentative_text: update.tentative_text,
            language: update.language.or(language),
            closed: false,
        })
    }

    pub(crate) async fn feed(
        &mut self,
        state: &AppState,
        app: Option<&AppHandle>,
        samples: &[i16],
    ) -> Result<NativeStreamUpdate, AppError> {
        if self.closed {
            return Err(stream_error("stream client is closed"));
        }
        if samples.is_empty() {
            return Ok(self.current_update());
        }

        for chunk in samples.chunks(5120) {
            let expected_samples = self.sample_count + chunk.len();
            let response = send_command_to_server(
                state,
                &ServerCommand::StreamFeed {
                    session_id: self.session_id,
                    offset: self.sample_count,
                    audio_base64: super::encode_pcm16_base64(chunk),
                    audio_format: "pcm_s16le".to_string(),
                    sample_rate: 16_000,
                },
                app,
            )
            .await;

            let response = match response {
                Ok(response) => response,
                Err(error) => {
                    self.closed = true;
                    return Err(error);
                }
            };
            let update = match validate_stream_response(
                response,
                self.session_id,
                expected_samples,
                false,
                &self.committed_text,
            ) {
                Ok(update) => update,
                Err(error) => {
                    self.closed = true;
                    return Err(error);
                }
            };
            self.apply_update(&update);
        }

        Ok(self.current_update())
    }

    pub(crate) async fn finish(
        &mut self,
        state: &AppState,
        app: Option<&AppHandle>,
    ) -> Result<TranscriptionResult, AppError> {
        if self.closed {
            return Err(stream_error("stream client is closed"));
        }

        let response = send_command_to_server(
            state,
            &ServerCommand::StreamFinish {
                session_id: self.session_id,
            },
            app,
        )
        .await;
        let response = match response {
            Ok(response) => response,
            Err(error) => {
                self.closed = true;
                return Err(error);
            }
        };
        let update = match validate_stream_response(
            response,
            self.session_id,
            self.sample_count,
            true,
            &self.committed_text,
        ) {
            Ok(update) => update,
            Err(error) => {
                self.closed = true;
                return Err(error);
            }
        };

        self.apply_update(&update);
        self.closed = true;
        Ok(TranscriptionResult {
            text: self.committed_text.clone(),
            duration: Some(self.sample_count as f64 / 16_000.0),
            success: true,
            error: None,
            language: self.language.clone(),
        })
    }

    pub(crate) async fn cancel(
        &mut self,
        state: &AppState,
        app: Option<&AppHandle>,
    ) -> Result<(), AppError> {
        if self.closed {
            return Err(stream_error("stream client is closed"));
        }

        let response = send_command_to_server(
            state,
            &ServerCommand::StreamCancel {
                session_id: self.session_id,
            },
            app,
        )
        .await;
        let response = match response {
            Ok(response) => response,
            Err(error) => {
                self.closed = true;
                return Err(error);
            }
        };
        let update = match validate_stream_response(
            response,
            self.session_id,
            self.sample_count,
            true,
            "",
        ) {
            Ok(update) => update,
            Err(error) => {
                self.closed = true;
                return Err(error);
            }
        };
        if !update.text.is_empty() {
            self.closed = true;
            return Err(stream_error("cancel response contained text"));
        }

        self.closed = true;
        Ok(())
    }

    fn apply_update(&mut self, update: &NativeStreamUpdate) {
        self.committed_text = update.text.clone();
        self.tentative_text = update.tentative_text.clone();
        self.sample_count = update.sample_count;
        if update.language.is_some() {
            self.language = update.language.clone();
        }
    }

    fn current_update(&self) -> NativeStreamUpdate {
        NativeStreamUpdate {
            tentative_text: self.tentative_text.clone(),
            text: self.committed_text.clone(),
            language: self.language.clone(),
            sample_count: self.sample_count,
            is_final: false,
        }
    }
}
