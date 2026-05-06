use std::borrow::Cow;

use rubato::Resampler;

use super::TARGET_SAMPLE_RATE;

const MAX_RESAMPLE_CHUNK_FRAMES: usize = 4096;

// ---------- 采样格式转换 ----------

pub(super) fn f32_to_i16(s: f32) -> i16 {
    let c = s.clamp(-1.0, 1.0);
    if c < 0.0 {
        (c * 32768.0) as i16
    } else {
        (c * 32767.0) as i16
    }
}

pub(super) fn u16_to_i16(s: u16) -> i16 {
    (s as i32 - 32768) as i16
}

// ---------- 重采样（rubato 快速插值） ----------

pub(super) struct ChunkedResampler {
    input_rate: u32,
    resampler: Option<rubato::FastFixedIn<f32>>,
    pending: Vec<f32>,
    chunk_size: usize,
}

impl ChunkedResampler {
    pub(super) fn new(input_rate: u32) -> Result<Self, String> {
        if input_rate == 0 {
            return Err("输入采样率为 0，无法重采样".to_string());
        }

        let chunk_size = ((input_rate as usize) / 100).clamp(1, MAX_RESAMPLE_CHUNK_FRAMES);
        let resampler = if input_rate == TARGET_SAMPLE_RATE {
            None
        } else {
            let ratio = TARGET_SAMPLE_RATE as f64 / input_rate as f64;
            Some(
                rubato::FastFixedIn::<f32>::new(
                    ratio,
                    1.1,
                    rubato::PolynomialDegree::Cubic,
                    chunk_size,
                    1,
                )
                .map_err(|e| format!("rubato 初始化失败: {}", e))?,
            )
        };

        Ok(Self {
            input_rate,
            resampler,
            pending: Vec::with_capacity(chunk_size),
            chunk_size,
        })
    }

    pub(super) fn process_chunk(
        &mut self,
        input: &[i16],
        output: &mut Vec<i16>,
    ) -> Result<(), String> {
        if input.is_empty() {
            return Ok(());
        }
        if self.input_rate == TARGET_SAMPLE_RATE {
            output.extend_from_slice(input);
            return Ok(());
        }

        let mut offset = 0;
        while offset < input.len() {
            if self.pending.len() >= self.chunk_size {
                self.process_ready_chunks(output)?;
            }

            let remaining_capacity = self.chunk_size.saturating_sub(self.pending.len()).max(1);
            let take = remaining_capacity.min(input.len() - offset);
            self.pending.extend(
                input[offset..offset + take]
                    .iter()
                    .map(|&sample| sample as f32 / 32768.0),
            );
            offset += take;
            self.process_ready_chunks(output)?;
        }
        Ok(())
    }

    pub(super) fn finish(&mut self, output: &mut Vec<i16>) -> Result<(), String> {
        if self.input_rate == TARGET_SAMPLE_RATE || self.pending.is_empty() {
            return Ok(());
        }

        let resampler = self
            .resampler
            .as_mut()
            .ok_or_else(|| "rubato 状态缺失".to_string())?;
        let chunk = std::mem::take(&mut self.pending);
        let out = resampler
            .process_partial(Some(&[&chunk]), None)
            .map_err(|e| format!("rubato 重采样失败: {}", e))?;
        output.extend(out[0].iter().map(|&sample| f32_to_i16(sample)));
        Ok(())
    }

    fn process_ready_chunks(&mut self, output: &mut Vec<i16>) -> Result<(), String> {
        let resampler = self
            .resampler
            .as_mut()
            .ok_or_else(|| "rubato 状态缺失".to_string())?;

        while self.pending.len() >= self.chunk_size {
            let chunk: Vec<f32> = self.pending.drain(..self.chunk_size).collect();
            let out = resampler
                .process(&[&chunk], None)
                .map_err(|e| format!("rubato 重采样失败: {}", e))?;
            output.extend(out[0].iter().map(|&sample| f32_to_i16(sample)));
        }
        Ok(())
    }
}

pub(super) struct ResamplerState {
    input_rate: u32,
    inner: ChunkedResampler,
}

impl ResamplerState {
    pub(super) fn new(input_rate: u32) -> Result<Self, String> {
        Ok(Self {
            input_rate,
            inner: ChunkedResampler::new(input_rate)?,
        })
    }

    pub(super) fn push_i16<'a>(&mut self, input: &'a [i16]) -> Result<Cow<'a, [i16]>, String> {
        if input.is_empty() || self.input_rate == TARGET_SAMPLE_RATE {
            return Ok(Cow::Borrowed(input));
        }

        let mut output = Vec::with_capacity(
            ((input.len() as f64 * TARGET_SAMPLE_RATE as f64 / self.input_rate as f64).ceil()
                as usize)
                + 8,
        );
        // rubato's exact streaming contract is fixed-input chunks. For interim ASR we flush
        // each delta with process_partial so latency stays low; final ASR uses bounded chunks.
        self.inner.process_chunk(input, &mut output)?;
        self.inner.finish(&mut output)?;
        Ok(Cow::Owned(output))
    }
}

#[allow(dead_code)]
pub(super) fn resample_to_16k(input: &[i16], input_rate: u32) -> Result<Cow<'_, [i16]>, String> {
    if input.is_empty() || input_rate == TARGET_SAMPLE_RATE {
        return Ok(Cow::Borrowed(input));
    }

    let mut resampler = ChunkedResampler::new(input_rate)?;
    let mut output = Vec::with_capacity(
        ((input.len() as f64 * TARGET_SAMPLE_RATE as f64 / input_rate as f64).ceil() as usize) + 8,
    );
    resampler.process_chunk(input, &mut output)?;
    resampler.finish(&mut output)?;
    Ok(Cow::Owned(output))
}

#[cfg(test)]
mod tests {
    use super::{resample_to_16k, ResamplerState};
    use std::borrow::Cow;

    #[test]
    fn invalid_sample_rate_is_not_reported_as_successful_16k_audio() {
        let input = [1_i16, -1, 2, -2];
        let output = resample_to_16k(&input, 0);

        assert!(output.is_err());
    }

    #[test]
    fn stateful_resampler_rejects_zero_input_rate() {
        let output = ResamplerState::new(0);

        assert!(output.is_err());
    }

    #[test]
    fn stateful_resampler_accepts_multiple_chunks_for_non_16k_input() {
        let mut state = ResamplerState::new(48_000).expect("48k input should be supported");
        let chunk_a = [0_i16; 480];
        let chunk_b = [100_i16; 480];

        let out_a = state
            .push_i16(&chunk_a)
            .expect("first chunk should resample");
        let out_b = state
            .push_i16(&chunk_b)
            .expect("second chunk should resample");

        let total = out_a.len() + out_b.len();
        assert!(
            (300..=340).contains(&total),
            "two 10ms 48k chunks should produce about 320 16k samples, got {total}"
        );
    }

    #[test]
    fn chunked_resampler_keeps_only_one_input_chunk_pending() {
        let mut resampler =
            super::ChunkedResampler::new(48_000).expect("48k input should be supported");
        let mut output = Vec::new();
        let input = vec![0_i16; 48_000 * 2];

        resampler
            .process_chunk(&input, &mut output)
            .expect("large input should process in bounded chunks");

        assert!(
            resampler.pending.len() < resampler.chunk_size,
            "resampler pending buffer should remain below one chunk after processing"
        );
        assert!(!output.is_empty());
    }

    #[test]
    fn stateful_resampler_passes_through_16k_without_owned_resample_buffers() {
        let mut state = ResamplerState::new(16_000).expect("16k input should be supported");
        let chunk_a = [1_i16, 2, 3];
        let chunk_b = [4_i16, 5];

        let out_a = state
            .push_i16(&chunk_a)
            .expect("first chunk should pass through");
        let out_b = state
            .push_i16(&chunk_b)
            .expect("second chunk should pass through");

        assert!(matches!(out_a, Cow::Borrowed(_)));
        assert!(matches!(out_b, Cow::Borrowed(_)));
        assert_eq!(out_a.as_ref(), &chunk_a);
        assert_eq!(out_b.as_ref(), &chunk_b);
    }
}
