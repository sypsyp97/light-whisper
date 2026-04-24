use std::borrow::Cow;

use super::TARGET_SAMPLE_RATE;

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

// ---------- 重采样（rubato sinc 插值） ----------

pub(super) fn resample_to_16k(input: &[i16], input_rate: u32) -> Result<Cow<'_, [i16]>, String> {
    if input.is_empty() || input_rate == TARGET_SAMPLE_RATE {
        return Ok(Cow::Borrowed(input));
    }
    if input_rate == 0 {
        return Err("输入采样率为 0，无法重采样".to_string());
    }

    use rubato::{FastFixedIn, PolynomialDegree, Resampler};

    let ratio = TARGET_SAMPLE_RATE as f64 / input_rate as f64;
    let chunk_size = input.len();

    let mut resampler =
        match FastFixedIn::<f32>::new(ratio, 1.1, PolynomialDegree::Cubic, chunk_size, 1) {
            Ok(r) => r,
            Err(e) => {
                return Err(format!("rubato 初始化失败: {}", e));
            }
        };

    let input_f32: Vec<f32> = input.iter().map(|&s| s as f32 / 32768.0).collect();

    match resampler.process(&[&input_f32], None) {
        Ok(output) => {
            let resampled: Vec<i16> = output[0].iter().map(|&s| f32_to_i16(s)).collect();
            Ok(Cow::Owned(resampled))
        }
        Err(e) => Err(format!("rubato 重采样失败: {}", e)),
    }
}

#[cfg(test)]
mod tests {
    use super::resample_to_16k;

    #[test]
    fn invalid_sample_rate_is_not_reported_as_successful_16k_audio() {
        let input = [1_i16, -1, 2, -2];
        let output = resample_to_16k(&input, 0);

        assert!(output.is_err());
    }
}
