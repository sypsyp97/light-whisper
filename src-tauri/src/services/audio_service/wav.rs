use crate::utils::AppError;

// ---------- WAV 编码 ----------

pub fn encode_wav(samples: &[i16], sample_rate: u32) -> Result<Vec<u8>, AppError> {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut cursor = std::io::Cursor::new(Vec::with_capacity(44 + samples.len() * 2));
    {
        let mut writer = hound::WavWriter::new(&mut cursor, spec)
            .map_err(|e| AppError::Audio(format!("WAV writer creation failed: {}", e)))?;
        for &s in samples {
            writer
                .write_sample(s)
                .map_err(|e| AppError::Audio(format!("WAV sample write failed: {}", e)))?;
        }
        writer
            .finalize()
            .map_err(|e| AppError::Audio(format!("WAV finalize failed: {}", e)))?;
    }
    Ok(cursor.into_inner())
}

// ---------- 单元测试 ----------
//
// TDD red-state tests for `encode_wav`.
//
// Current signature is `fn encode_wav(samples: &[i16], sample_rate: u32) -> Vec<u8>`
// with three internal `.expect(...)` calls that panic on error. The contract we
// want to lock in is a fallible signature — `Result<Vec<u8>, AppError>` — so that
// hound write failures bubble up instead of panicking.
//
// These tests use `?` against the return value, which forces the function to
// return a `Result<_, AppError>`. That means **they are expected to fail to
// compile** until `encode_wav` is migrated to the fallible signature — exactly
// the red state we want.
#[cfg(test)]
mod tests {
    use super::*;
    use crate::utils::error::AppError;

    #[test]
    fn test_encode_wav_returns_ok_for_normal_samples() -> Result<(), AppError> {
        let bytes = encode_wav(&[0i16, 1, -1, 32767, -32768], 16000)?;
        assert!(!bytes.is_empty(), "encoded WAV bytes must be non-empty");
        assert!(
            bytes.len() >= 4,
            "encoded WAV must include at least the RIFF header"
        );
        assert_eq!(&bytes[..4], b"RIFF", "WAV file must start with 'RIFF'");
        Ok(())
    }

    #[test]
    fn test_encode_wav_handles_empty_samples() -> Result<(), AppError> {
        // Empty sample slice must not panic — it should still produce a well-formed
        // (header-only) WAV buffer.
        let _bytes = encode_wav(&[], 16000)?;
        Ok(())
    }
}
