#[path = "device.rs"]
mod device;
use crate::{audio, models, recognition::Recognition};
use light_whisper_native_asr::{Backend, Family, Transcript};
use serde_json::{Value as V, json};
use std::{
    collections::HashSet,
    io::{self, BufRead, Write},
    path::PathBuf,
    sync::mpsc,
    time::{Duration, Instant},
};
type R<T> = Result<T, String>;
fn merge(mut a: V, b: &V) -> V {
    if let (Some(dst), Some(src)) = (a.as_object_mut(), b.as_object()) {
        dst.extend(src.clone())
    }
    a
}
fn rounded(x: f64, factor: f64) -> f64 {
    (x * factor).round_ties_even() / factor
}
fn options(o: &V, hot: Option<&V>) -> R<(String, String)> {
    let mut context = match o.get("context") {
        None => "",
        Some(v) => v.as_str().ok_or("context must be a string")?,
    }
    .trim()
    .to_owned();
    let lang = match &o["language"] {
        V::Null => "",
        v => v.as_str().ok_or("language must be a string or null")?,
    }
    .trim()
    .to_owned();
    if let Some(list) = hot.or_else(|| o.get("hot_words")) {
        let words = list.as_array().ok_or("hot_words must be a list")?;
        if words.iter().any(|v| !v.is_string()) {
            return Err("hot_words must contain strings".into());
        }
        let mut seen = HashSet::new();
        let names: Vec<_> = words
            .iter()
            .filter_map(V::as_str)
            .map(|s| s.split_whitespace().collect::<Vec<_>>().join(" "))
            .filter(|s| !s.is_empty() && s.chars().count() <= 80 && seen.insert(s.to_lowercase()))
            .take(100)
            .collect();
        if !names.is_empty() {
            if !context.is_empty() {
                context.push('\n')
            }
            context.push_str("Hotwords: ");
            context.push_str(&names.join(", "));
        }
    }
    Ok((context, lang))
}
fn idle_seconds(v: &V) -> u64 {
    let num = match v {
        V::Number(n) => n.as_f64(),
        V::String(s) => s.trim().parse::<i128>().ok().map(|n| n as f64),
        _ => None,
    };
    num.filter(|n| n.fract() == 0. && *n > 0.)
        .unwrap_or(0.)
        .min(604800.) as u64
}
struct Session {
    id: u64,
    samples: usize,
    lang: Option<String>,
    text: String,
    last_preview: String,
    ms: f64,
}
struct Server<'a> {
    name: &'a str,
    family: Family,
    assets: PathBuf,
    model: Option<PathBuf>,
    runtime: Option<Recognition>,
    suspended: bool,
    past_vad: (u64, u64, f64),
    enhance_enabled: bool,
    enhancer: Option<light_whisper_reuse::RealtimeEnhancer>,
    backend: Backend,
    candidates: Vec<Backend>,
    forced_gpu: bool,
    device: V,
    last_id: u64,
    active: Option<Session>,
    count: u64,
    total: f64,
    ms: f64,
    idle: u64,
    last: Instant,
}
impl Server<'_> {
    fn qwen(&self) -> bool {
        self.family == Family::Qwen
    }
    fn name_of(&self, b: Backend) -> &'static str {
        match b {
            Backend::Cuda => "cuda",
            Backend::Vulkan => "vulkan",
            Backend::Cpu => "cpu",
        }
    }
    fn backend(&self) -> &str {
        self.name_of(self.backend)
    }
    fn loaded(&self) -> bool {
        self.runtime.is_some() && !self.suspended
    }
    fn base(&self) -> V {
        json!({"engine":self.name,"backend":self.backend()})
    }
    fn err(&self, kind: &str, msg: &str) -> V {
        merge(
            json!({"success":false,"type":kind,"error":msg}),
            &self.base(),
        )
    }
    fn init(&mut self) -> Result<(), V> {
        if self.loaded() {
            return Ok(());
        }
        let path=match self.model.as_ref().filter(|p|p.is_file()){
            Some(p)=>p.clone(),None=>models::resolve(self.family).map_err(|e|json!({"success":false,"type":"models_not_downloaded","error":e,"engine":self.name}))?
        };
        self.model = Some(path.clone());
        let threads = std::env::var("LIGHT_WHISPER_NATIVE_THREADS")
            .unwrap_or_else(|_| "4".into())
            .parse::<i32>()
            .ok()
            .filter(|n| *n > 0)
            .ok_or_else(|| self.err("init_error", "invalid native thread count"))?;
        let mut reasons = Vec::new();
        for &b in &self.candidates {
            let name = self.name_of(b);
            if !self
                .assets
                .join("r2t2-native")
                .join(name)
                .join("audiocpp.dll")
                .is_file()
            {
                if self.forced_gpu {
                    eprintln!("{name}: native package is not installed");
                }
                continue;
            }
            let attempt = if self.suspended
                && b == self.backend
                && let Some(engine) = &mut self.runtime
            {
                engine.resume().map(|_| None)
            } else {
                Recognition::open(&self.assets, &path, self.family, b, threads).map(Some)
            };
            let replacement = match attempt {
                Ok(engine) => engine,
                Err(error) => {
                    eprintln!("{name}: {error}");
                    reasons.push(error);
                    continue;
                }
            };
            if self.enhance_enabled {
                match crate::enhancement::open(&self.assets) {
                    Ok(enhancer) => self.enhancer = Some(enhancer),
                    Err(e) => {
                        if replacement.is_none()
                            && let Some(engine) = &mut self.runtime
                        {
                            let _ = engine.suspend();
                        }
                        return Err(self.err("init_error", &e));
                    }
                }
            }
            if let Some(old) = replacement.and_then(|new| self.runtime.replace(new)) {
                let (calls, rejected, ms) = old.vad_stats();
                self.past_vad.0 += calls;
                self.past_vad.1 += rejected;
                self.past_vad.2 += ms;
            }
            self.backend = b;
            self.device = device::describe(b, self.qwen());
            self.suspended = false;
            self.last_id = 0;
            return Ok(());
        }
        Err(self.err(
            "init_error",
            &format!("语音引擎初始化失败 {}", reasons.join(", ")),
        ))
    }
    fn status(&self) -> V {
        let live = self.loaded();
        let mut v = merge(
            merge(
                json!({"success":true,"installed":live || self.qwen() && ["cuda","vulkan","cpu"].iter().any(|b|self.assets.join("r2t2-native").join(b).join("audiocpp.dll").is_file()),"initialized":live,"model_loaded":live,"native_streaming":live,"models":{"asr":live,"vad":self.runtime.is_some()}}),
                &self.base(),
            ),
            &self.device,
        );
        if self.qwen() {
            v["models"]["punc"] = json!(true)
        } else {
            v["inline_audio"] = json!(live)
        }
        if self.suspended {
            v["gpu_suspended"] = json!(true)
        }
        v
    }
    fn stats(&self) -> V {
        let mut v = merge(
            json!({"transcription_count":self.count,"total_audio_duration":rounded(self.total,100.),"average_inference_ms":rounded(self.ms/self.count.max(1) as f64,1000.),"initialized":self.loaded(),"native_streaming":self.loaded()}),
            &self.base(),
        );
        if self.qwen() {
            let (calls, rejected, ms) = self
                .runtime
                .as_ref()
                .map(Recognition::vad_stats)
                .unwrap_or_default();
            let (calls, rejected, ms) = (
                calls + self.past_vad.0,
                rejected + self.past_vad.1,
                ms + self.past_vad.2,
            );
            v = merge(
                v,
                &json!({"vad_calls":calls,"vad_rejected":rejected,"average_vad_ms":rounded(ms/calls.max(1) as f64,1000.),"speculative_decoding":false,"models_loaded":self.status()["models"]}),
            );
        } else {
            v["inline_audio"] = json!(self.loaded())
        }
        json!({"success":true,"stats":v})
    }
    fn transcribe(&mut self, c: &V) -> R<V> {
        let qwen = self.qwen();
        let mode = if audio::memory(c) { "memory" } else { "path" };
        let (ctx, lang) = options(&c["options"], c.get("hot_words").filter(|v| !v.is_null()))?;
        let (pcm, duration) = audio::load(&self.assets, c, qwen)?;
        if qwen {
            self.total += duration;
            if duration < 0.5 {
                return Ok(
                    json!({"success":true,"text":"","duration":duration,"engine":self.name,"input_mode":mode}),
                );
            }
        }
        let pcm = if let Some(enhancer) = &mut self.enhancer {
            enhancer.enhance(&pcm)?
        } else {
            pcm
        };
        let result = self
            .runtime
            .as_mut()
            .ok_or("engine unavailable")?
            .transcribe(&pcm, &ctx, &lang)?;
        let has_result = !qwen || result.vad_segments > 0;
        if has_result {
            self.count += 1;
            self.ms += result.inference_ms
        }
        if !qwen {
            self.total += duration
        }
        let mut language = result.output.language;
        if language.is_none() {
            language = if qwen {
                Some("unknown".into())
            } else if !lang.is_empty() {
                Some(lang)
            } else {
                None
            }
        }
        let mut v = merge(
            json!({"success":true,"text":result.output.text,"raw_text":result.output.text,"language":language,"input_mode":mode,"duration":duration,"inference_ms":rounded(result.inference_ms,1000.)}),
            &self.base(),
        );
        if qwen {
            v = merge(
                v,
                &json!({"model_type":self.name,"speech_duration":rounded(result.speech_samples as f64/16000.,1000.),"vad_segments":result.vad_segments,"vad_ms":rounded(result.vad_ms,1000.)}),
            );
            if has_result {
                v["confidence"] = json!(0.)
            }
        }
        Ok(v)
    }
    fn stream(&mut self, c: &V, action: &str) -> R<V> {
        let t = Instant::now();
        let id = c["session_id"]
            .as_u64()
            .filter(|n| *n > 0)
            .ok_or("invalid session ID")?;
        let qwen = self.qwen();
        let runtime = self.runtime.as_mut().ok_or("engine unavailable")?;
        if action == "stream_start" {
            let (ctx, lang) = options(c, None)?;
            if id <= self.last_id {
                return Err("stale session".into());
            }
            self.last_id = id;
            if self.active.take().is_some() {
                runtime.cancel()?
            }
            if let Some(enhancer) = &mut self.enhancer {
                enhancer.reset()?
            }
            if let Err(e) = runtime.start(&ctx, &lang) {
                let _ = runtime.cancel();
                return Err(e);
            }
            self.active = Some(Session {
                id,
                samples: 0,
                lang: (!lang.is_empty()).then_some(lang),
                text: String::new(),
                last_preview: String::new(),
                ms: 0.,
            });
        }
        let s = self
            .active
            .as_mut()
            .filter(|s| s.id == id)
            .ok_or("session is not active")?;
        let final_response = action == "stream_finish" || action == "stream_cancel";
        let mut suffix = String::new();
        let mut accepted = 0;
        let output = match action {
            "stream_feed" => {
                let preview = match c.get("preview") {
                    Some(v) => v.as_bool().ok_or("preview must be a boolean")?,
                    None => true,
                };
                if c["offset"].as_u64() != Some(s.samples as u64)
                    || c["sample_rate"].as_u64() != Some(16000)
                    || c["audio_format"] != "pcm_s16le"
                    || (!preview && !qwen)
                {
                    return Err("invalid stream feed".into());
                }
                let pcm = audio::pcm16(&audio::bytes(c)?)?;
                if pcm.is_empty() {
                    return Err("empty input".into());
                }
                accepted = pcm.len();
                let processed = if let Some(enhancer) = &mut self.enhancer {
                    enhancer.feed(&pcm)
                } else {
                    Ok(pcm)
                };
                processed.and_then(|p| {
                    if p.is_empty() {
                        Ok(Transcript {
                            text: s.text.clone(),
                            preview: s.last_preview.clone(),
                            language: s.lang.clone(),
                        })
                    } else {
                        runtime.feed(&p, preview)
                    }
                })
            }
            "stream_finish" if s.samples > 0 => {
                let tail = if let Some(enhancer) = &mut self.enhancer {
                    enhancer.flush()
                } else {
                    Ok(Vec::new())
                };
                tail.and_then(|p| {
                    if !p.is_empty() {
                        runtime.feed(&p, !qwen)?;
                    }
                    runtime.finish()
                })
            }
            "stream_cancel" | "stream_finish" => runtime.cancel().map(|_| Transcript::default()),
            _ => Ok(Transcript::default()),
        };
        let output = if final_response && let Some(enhancer) = &mut self.enhancer {
            let reset = enhancer.reset();
            output.and_then(|v| reset.map(|_| v))
        } else {
            output
        };
        let result = output.and_then(|out| {
            if action != "stream_cancel" && !out.text.starts_with(&s.text) {
                return Err("committed text regressed".into());
            }
            if action == "stream_feed" {
                suffix = out
                    .preview
                    .strip_prefix(&out.text)
                    .ok_or("invalid preview")?
                    .to_owned()
            }
            Ok(out)
        });
        let out = match result {
            Ok(v) => v,
            Err(e) => {
                self.active = None;
                let _ = runtime.cancel();
                if let Some(enhancer) = &mut self.enhancer {
                    let _ = enhancer.reset();
                }
                return Err(e);
            }
        };
        s.last_preview = out.preview;
        s.text = out.text;
        s.samples += accepted;
        s.ms += t.elapsed().as_secs_f64() * 1000.;
        if let Some(l) = out.language.filter(|s| !s.is_empty()) {
            s.lang = Some(l)
        }
        let v = json!({"success":true,"session_id":id,"text":s.text,"tentative_text":suffix,"language":s.lang,"sample_count":s.samples,"final":final_response});
        if action == "stream_finish" {
            self.count += 1;
            self.total += s.samples as f64 / 16000.;
            self.ms += s.ms
        }
        if final_response {
            self.active = None
        }
        Ok(merge(v, &self.base()))
    }
    fn dispatch(&mut self, c: &V, action: &str) -> V {
        match action {
            "exit" => json!({"success":true,"message":"服务器退出"}),
            "status" => self.status(),
            "stats" => self.stats(),
            "cleanup" => json!({"success":true,"message":"内存清理完成"}),
            "set_audio_processing" => {
                let Some(enabled) = c["enabled"].as_bool() else {
                    return json!({"success":false,"error":"enabled 必须为布尔值"});
                };
                if enabled != self.enhance_enabled {
                    if self.active.is_some() {
                        return json!({"success":false,"error":"请在录音结束后修改音频处理设置"});
                    }
                    self.enhancer = if enabled && self.loaded() {
                        match crate::enhancement::open(&self.assets) {
                            Ok(e) => Some(e),
                            Err(e) => return json!({"success":false,"error":e}),
                        }
                    } else {
                        None
                    };
                    self.enhance_enabled = enabled;
                }
                json!({"success":true,"audio_processing_enabled":enabled})
            }
            "set_gpu_idle" => match c.get("seconds") {
                None => json!({"success":false,"error":"缺少 seconds"}),
                Some(v) => {
                    if self.idle == 0 {
                        self.last = Instant::now()
                    }
                    self.idle = idle_seconds(v);
                    json!({"success":true,"enabled":self.idle>0,"gpu_idle_seconds":self.idle})
                }
            },
            "transcribe" | "stream_start" | "stream_feed" | "stream_finish" | "stream_cancel" => {
                let result = match self.init() {
                    Err(e) => e,
                    Ok(()) if action == "transcribe" && self.active.is_some() => {
                        self.err("stream_busy", "流式会话正在进行")
                    }
                    Ok(()) if action == "transcribe" => self.transcribe(c).unwrap_or_else(|e| {
                        merge(
                            self.err("transcription_error", &e),
                            &json!({"input_mode":if audio::memory(c){"memory"}else{"path"}}),
                        )
                    }),
                    Ok(()) => self
                        .stream(c, action)
                        .unwrap_or_else(|e| self.err("stream_error", &e)),
                };
                self.last = Instant::now();
                result
            }
            _ => json!({"success":false,"error":format!("未知命令: {action}")}),
        }
    }
    fn tick(&mut self) {
        if self.idle > 0
            && self.loaded()
            && self.active.is_none()
            && self.last.elapsed() >= Duration::from_secs(self.idle)
            && let Some(r) = &mut self.runtime
        {
            match r.suspend() {
                Ok(()) => {
                    self.enhancer = None;
                    self.suspended = true
                }
                Err(e) => eprintln!("idle: {e}"),
            }
        }
    }
}
fn gpu_backend_choice() -> (Vec<Backend>, bool) {
    let forced = std::env::var("LIGHT_WHISPER_GPU_BACKEND")
        .unwrap_or_else(|_| "auto".into())
        .trim()
        .to_lowercase();
    match forced.as_str() {
        "cuda" => (vec![Backend::Cuda, Backend::Cpu], true),
        "vulkan" => (vec![Backend::Vulkan, Backend::Cpu], true),
        "cpu" => (vec![Backend::Cpu], false),
        _ => (vec![Backend::Cuda, Backend::Vulkan, Backend::Cpu], false),
    }
}
pub fn run(name: &str, family: Family) -> R<()> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let assets = exe
        .parent()
        .ok_or("missing executable directory")?
        .join("_internal");
    let (candidates, forced_gpu) = gpu_backend_choice();
    let mut server = Server {
        name,
        family,
        assets,
        enhance_enabled: std::env::var("LIGHT_WHISPER_AUDIO_PROCESSING").as_deref() == Ok("1"),
        enhancer: None,
        model: None,
        runtime: None,
        suspended: false,
        past_vad: (0, 0, 0.),
        backend: Backend::Cpu,
        candidates,
        forced_gpu,
        device: device::describe(Backend::Cpu, family == Family::Qwen),
        active: None,
        last_id: 0,
        count: 0,
        total: 0.,
        ms: 0.,
        idle: idle_seconds(&json!(std::env::var("LIGHT_WHISPER_GPU_IDLE_SECONDS").ok())),
        last: Instant::now(),
    };
    let ready = match server.init() {
        Ok(()) => merge(
            merge(
                json!({"success":true,"message":"语音引擎初始化成功","model_loaded":true,"native_streaming":true}),
                &server.base(),
            ),
            &server.device,
        ),
        Err(e) => e,
    };
    let mut stdout = io::stdout().lock();
    let mut send = |v: V| -> R<()> {
        writeln!(stdout, "{v}")
            .and_then(|_| stdout.flush())
            .map_err(|e| e.to_string())
    };
    send(ready)?;
    server.last = Instant::now();
    let (tx, rx) = mpsc::sync_channel(1);
    std::thread::spawn(move || {
        for line in io::stdin().lock().lines() {
            let Ok(s) = line else { break };
            if tx.send(s).is_err() {
                break;
            }
        }
    });
    loop {
        server.tick();
        let line = if server.idle == 0
            || server.suspended
            || !server.loaded()
            || server.active.is_some()
        {
            match rx.recv() {
                Ok(s) => s,
                Err(_) => break,
            }
        } else {
            match rx.recv_timeout(
                Duration::from_secs(server.idle)
                    .saturating_sub(server.last.elapsed())
                    .max(Duration::from_millis(10)),
            ) {
                Ok(s) => s,
                Err(mpsc::RecvTimeoutError::Timeout) => continue,
                Err(_) => break,
            }
        };
        if line.trim().is_empty() {
            continue;
        }
        let Ok(c) = serde_json::from_str::<V>(&line) else {
            send(json!({"success":false,"error":"无效的JSON命令"}))?;
            continue;
        };
        let action = c["action"].as_str().unwrap_or("");
        let mut result = server.dispatch(&c, action);
        let id = &c["request_id"];
        if id.is_number() && !id.to_string().contains(['.', 'e', 'E']) || id.is_boolean() {
            result["request_id"] = id.clone()
        }
        send(result)?;
        if action == "exit" {
            break;
        }
    }
    Ok(())
}
