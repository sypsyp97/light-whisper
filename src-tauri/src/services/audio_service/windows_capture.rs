use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

use wasapi::{
    AudioCaptureClient, AudioClient, AudioClientProperties, Device, DeviceEnumerator, Direction,
    Handle, SampleType, StreamCategory, StreamMode, WasapiError, WaveFormat, GUID,
};

use super::capture::{mix_to_mono_capped_i16, warn_if_record_cap_reached, MAX_RECORD_SAMPLES};
use super::TARGET_SAMPLE_RATE;

const EVENT_WAIT_MS: u32 = 200;
const PRIME_ATTEMPTS: usize = 5;

const EFFECT_AEC: GUID = GUID::from_u128(0x6f64adbe_8211_11e2_8c70_2c27d7f001fa);
const EFFECT_NOISE_SUPPRESSION: GUID = GUID::from_u128(0x6f64adbf_8211_11e2_8c70_2c27d7f001fa);
const EFFECT_AUTOMATIC_GAIN_CONTROL: GUID = GUID::from_u128(0x6f64adc0_8211_11e2_8c70_2c27d7f001fa);
const EFFECT_BEAMFORMING: GUID = GUID::from_u128(0x6f64adc1_8211_11e2_8c70_2c27d7f001fa);
const EFFECT_DEEP_NOISE_SUPPRESSION: GUID = GUID::from_u128(0x6f64add0_8211_11e2_8c70_2c27d7f001fa);

struct ComGuard;

impl ComGuard {
    fn initialize() -> Result<Self, String> {
        wasapi::initialize_mta()
            .ok()
            .map_err(|err| format!("初始化 COM MTA 失败: {err}"))?;
        Ok(Self)
    }
}

impl Drop for ComGuard {
    fn drop(&mut self) {
        wasapi::deinitialize();
    }
}

pub(super) struct WindowsSpeechCapture {
    client: AudioClient,
    capture_client: AudioCaptureClient,
    event: Handle,
    raw_buffer: Vec<u8>,
    // 必须最后析构，确保所有 COM 音频对象先释放。
    _com: ComGuard,
}

impl WindowsSpeechCapture {
    pub(super) fn start(preferred_name: Option<&str>) -> Result<Self, String> {
        let com = ComGuard::initialize()?;
        let enumerator =
            DeviceEnumerator::new().map_err(|err| format!("创建设备枚举器失败: {err}"))?;
        let (device, device_name) = select_capture_device(&enumerator, preferred_name)?;

        let categories = [
            (StreamCategory::VoiceTyping, "VoiceTyping"),
            (StreamCategory::UniformSpeech, "UniformSpeech"),
            (StreamCategory::Speech, "Speech"),
            (StreamCategory::Communications, "Communications"),
        ];
        let mut failures = Vec::with_capacity(categories.len());

        for (category, category_name) in categories {
            match Self::start_with_category(&enumerator, &device, category, category_name) {
                Ok((client, capture_client, event, raw_buffer)) => {
                    #[cfg(test)]
                    eprintln!("native_category={category_name}");
                    log::info!(
                        "使用 Windows 原生语音处理录音: 设备='{}', 类别={}, {}Hz, 1ch, i16",
                        device_name,
                        category_name,
                        TARGET_SAMPLE_RATE
                    );
                    return Ok(Self {
                        _com: com,
                        client,
                        capture_client,
                        event,
                        raw_buffer,
                    });
                }
                Err(err) => failures.push(format!("{category_name}: {err}")),
            }
        }

        Err(format!(
            "设备 '{}' 不支持听写语音流（{}）",
            device_name,
            failures.join("; ")
        ))
    }

    fn start_with_category(
        enumerator: &DeviceEnumerator,
        device: &Device,
        category: StreamCategory,
        category_name: &str,
    ) -> Result<(AudioClient, AudioCaptureClient, Handle, Vec<u8>), String> {
        let mut client = device
            .get_iaudioclient()
            .map_err(|err| format!("创建 AudioClient 失败: {err}"))?;
        client
            .set_properties(AudioClientProperties::new().set_category(category))
            .map_err(|err| format!("设置 {category_name} 类别失败: {err}"))?;

        let desired_format = WaveFormat::new(
            16,
            16,
            &SampleType::Int,
            TARGET_SAMPLE_RATE as usize,
            1,
            None,
        );
        let (_, min_period) = client
            .get_device_period()
            .map_err(|err| format!("读取设备周期失败: {err}"))?;
        let mode = StreamMode::EventsShared {
            autoconvert: true,
            buffer_duration_hns: min_period,
        };
        client
            .initialize_client(&desired_format, &Direction::Capture, &mode)
            .map_err(|err| format!("初始化共享语音流失败: {err}"))?;

        log_audio_effects(&client);
        configure_aec(&client, enumerator);

        let event = client
            .set_get_eventhandle()
            .map_err(|err| format!("创建音频事件失败: {err}"))?;
        let buffer_frames = client
            .get_buffer_size()
            .map_err(|err| format!("读取音频缓冲大小失败: {err}"))?;
        let capture_client = client
            .get_audiocaptureclient()
            .map_err(|err| format!("创建捕获客户端失败: {err}"))?;
        client
            .start_stream()
            .map_err(|err| format!("启动语音流失败: {err}"))?;

        Ok((
            client,
            capture_client,
            event,
            vec![0; buffer_frames as usize * desired_format.get_blockalign() as usize],
        ))
    }

    pub(super) fn prime(
        &mut self,
        samples: &Arc<parking_lot::Mutex<Vec<i16>>>,
    ) -> Result<(), String> {
        for _ in 0..PRIME_ATTEMPTS {
            match self.read_event(samples) {
                Ok(frames) if frames > 0 => return Ok(()),
                Ok(_) | Err(WasapiError::EventTimeout) => continue,
                Err(err) => return Err(err.to_string()),
            }
        }
        Err(format!(
            "{} ms 内没有收到音频数据",
            EVENT_WAIT_MS as usize * PRIME_ATTEMPTS
        ))
    }

    pub(super) fn run(
        &mut self,
        stop: &Arc<AtomicBool>,
        samples: &Arc<parking_lot::Mutex<Vec<i16>>>,
    ) {
        while !stop.load(Ordering::Relaxed) {
            match self.read_event(samples) {
                Ok(_) | Err(WasapiError::EventTimeout) => {}
                Err(err) => {
                    log::error!("Windows 原生音频流错误: {}", err);
                    break;
                }
            }
        }
    }

    fn read_event(
        &mut self,
        samples: &Arc<parking_lot::Mutex<Vec<i16>>>,
    ) -> Result<usize, WasapiError> {
        self.event.wait_for_event(EVENT_WAIT_MS)?;
        let mut total_frames = 0;

        loop {
            let frames = self.capture_client.get_next_packet_size()?.unwrap_or(0) as usize;
            if frames == 0 {
                break;
            }
            let required_bytes = frames * std::mem::size_of::<i16>();
            if self.raw_buffer.len() < required_bytes {
                self.raw_buffer.resize(required_bytes, 0);
            }
            let (read_frames, info) = self
                .capture_client
                .read_from_device(&mut self.raw_buffer[..required_bytes])?;
            let read_frames = read_frames as usize;
            if read_frames == 0 {
                continue;
            }

            let mut chunk = Vec::with_capacity(read_frames);
            if info.flags.silent {
                chunk.resize(read_frames, 0);
            } else {
                chunk.extend(
                    self.raw_buffer[..read_frames * std::mem::size_of::<i16>()]
                        .chunks_exact(2)
                        .map(|bytes| i16::from_le_bytes([bytes[0], bytes[1]])),
                );
            }

            let mut locked = samples.lock();
            warn_if_record_cap_reached(locked.len());
            mix_to_mono_capped_i16(&chunk, 1, &mut locked, MAX_RECORD_SAMPLES);
            total_frames += read_frames;

            if info.flags.data_discontinuity {
                log::warn!("Windows 原生音频流检测到数据不连续");
            }
        }

        Ok(total_frames)
    }
}

impl Drop for WindowsSpeechCapture {
    fn drop(&mut self) {
        if let Err(err) = self.client.stop_stream() {
            log::debug!("停止 Windows 原生音频流时返回错误: {}", err);
        }
    }
}

fn select_capture_device(
    enumerator: &DeviceEnumerator,
    preferred_name: Option<&str>,
) -> Result<(Device, String), String> {
    if let Some(name) = preferred_name.filter(|name| !name.trim().is_empty()) {
        let collection = enumerator
            .get_device_collection(&Direction::Capture)
            .map_err(|err| format!("枚举输入设备失败: {err}"))?;
        match collection.get_device_with_name(name) {
            Ok(device) => return Ok((device, name.to_owned())),
            Err(err) => log::warn!(
                "Windows 原生捕获找不到指定麦克风 '{}'，回退默认设备: {}",
                name,
                err
            ),
        }
    }

    let device = enumerator
        .get_default_device(&Direction::Capture)
        .map_err(|err| format!("获取默认输入设备失败: {err}"))?;
    let name = device
        .get_friendlyname()
        .unwrap_or_else(|_| "未知设备".into());
    Ok((device, name))
}

fn configure_aec(client: &AudioClient, enumerator: &DeviceEnumerator) {
    let control = match client.get_aec_control() {
        Ok(control) => control,
        Err(_) => {
            log::info!("Windows 当前设备/语音模式未提供可控 AEC");
            #[cfg(test)]
            eprintln!("native_aec=false");
            return;
        }
    };

    let render_endpoint_id = enumerator
        .get_default_device(&Direction::Render)
        .ok()
        .and_then(|device| device.get_id().ok());
    let endpoint_mode = if render_endpoint_id.is_some() {
        "默认播放设备"
    } else {
        "Windows 自动选择的播放设备"
    };
    match control.set_echo_cancellation_render_endpoint(render_endpoint_id) {
        Ok(()) => {
            log::info!("Windows AEC 已启用并绑定{}", endpoint_mode);
            #[cfg(test)]
            eprintln!("native_aec=true");
        }
        Err(err) => log::warn!("Windows AEC 可用但启用失败: {}", err),
    }
}

fn log_audio_effects(client: &AudioClient) {
    let effects = match client
        .get_audio_effects_manager()
        .and_then(|manager| manager.get_audio_effects())
    {
        Ok(Some(effects)) => effects,
        Ok(None) => {
            log::info!("Windows 原生语音流未报告活动音频效果");
            #[cfg(test)]
            eprintln!("native_effects=[] native_effect_count=0");
            return;
        }
        Err(err) => {
            log::info!("当前 Windows/驱动不支持枚举活动音频效果: {}", err);
            #[cfg(test)]
            eprintln!("native_effects=unavailable");
            return;
        }
    };

    let known: Vec<_> = effects
        .iter()
        .filter_map(|effect| known_effect_name(&effect.id))
        .collect();
    log::info!(
        "Windows 原生语音流活动音频效果: 已识别=[{}], 总数={}",
        known.join(", "),
        effects.len()
    );
    #[cfg(test)]
    eprintln!(
        "native_effects=[{}] native_effect_count={}",
        known.join(","),
        effects.len()
    );
}

fn known_effect_name(id: &GUID) -> Option<&'static str> {
    match *id {
        EFFECT_AEC => Some("AEC"),
        EFFECT_NOISE_SUPPRESSION => Some("NS"),
        EFFECT_DEEP_NOISE_SUPPRESSION => Some("Deep NS"),
        EFFECT_AUTOMATIC_GAIN_CONTROL => Some("AGC"),
        EFFECT_BEAMFORMING => Some("Beamforming"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_windows_speech_effect_guids() {
        assert_eq!(known_effect_name(&EFFECT_AEC), Some("AEC"));
        assert_eq!(
            known_effect_name(&EFFECT_DEEP_NOISE_SUPPRESSION),
            Some("Deep NS")
        );
        assert_eq!(known_effect_name(&GUID::zeroed()), None);
    }

    #[test]
    #[ignore = "requires an available Windows microphone"]
    fn native_speech_capture_receives_audio_frames() {
        let started = std::time::Instant::now();
        let samples = Arc::new(parking_lot::Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let mut capture = WindowsSpeechCapture::start(None).expect("start native capture");
        capture.prime(&samples).expect("receive first audio packet");

        let stop_later = stop.clone();
        let stopper = std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(500));
            stop_later.store(true, Ordering::Relaxed);
        });
        capture.run(&stop, &samples);
        stopper.join().unwrap();

        let frame_count = samples.lock().len();
        eprintln!(
            "native_frames={} elapsed_ms={:.1}",
            frame_count,
            started.elapsed().as_secs_f64() * 1000.0
        );
        assert!(frame_count >= TARGET_SAMPLE_RATE as usize / 4);
    }
}
