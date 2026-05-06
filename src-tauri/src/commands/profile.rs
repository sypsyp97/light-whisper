use std::sync::atomic::Ordering;

use crate::services::llm_client::{LlmRequestOptions, LlmUserInput};
use crate::services::{codex_oauth_service, llm_client, llm_provider, profile_service};
use crate::state::user_profile::*;
use crate::state::AppState;

#[tauri::command]
pub async fn submit_user_correction(
    app_handle: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    original: String,
    corrected: String,
    raw_original: Option<String>,
) -> Result<(), String> {
    // 用 LLM 结合 ASR 原文和当前显示文本，提取词级纠错
    let corrections = extract_corrections_via_llm(
        &app_handle,
        &state,
        raw_original.as_deref(),
        &original,
        &corrected,
    )
    .await;
    let fallback_corrections = if corrections.is_empty() {
        let mut baselines = Vec::with_capacity(2);
        if let Some(raw) = raw_original.as_deref() {
            baselines.push(raw);
        }
        baselines.push(original.as_str());
        profile_service::collect_diff_correction_pairs(&baselines, &corrected)
    } else {
        Vec::new()
    };

    profile_service::update_profile_and_schedule(state.inner(), |profile| {
        if !corrections.is_empty() {
            profile_service::learn_from_structured(
                profile,
                &corrections,
                &[],
                CorrectionSource::User,
            );
        } else if !fallback_corrections.is_empty() {
            profile_service::learn_from_structured(
                profile,
                &fallback_corrections,
                &[],
                CorrectionSource::User,
            );
        } else {
            profile_service::learn_from_correction(
                profile,
                &original,
                &corrected,
                CorrectionSource::User,
            );
        }
    });
    Ok(())
}

/// 调用 LLM 结合 ASR 原文和当前显示文本，提取词级别纠错
async fn extract_corrections_via_llm(
    app_handle: &tauri::AppHandle,
    state: &AppState,
    raw_original: Option<&str>,
    before: &str,
    after: &str,
) -> Vec<(String, String)> {
    let api_key = match codex_oauth_service::resolve_api_key_for_provider(
        app_handle,
        state,
        &state.active_llm_provider(),
        &state.read_ai_polish_api_key(),
    )
    .await
    {
        Ok(api_key) => api_key,
        Err(err) => {
            log::warn!("用户纠错 LLM 鉴权解析失败: {}", err);
            return Vec::new();
        }
    };
    if api_key.is_empty() {
        return Vec::new();
    }

    let config = state.llm_provider_config();
    let endpoint = llm_provider::endpoint_for_config(&config);

    let prompt = if let Some(raw) = raw_original.filter(|value| !value.trim().is_empty()) {
        format!(
            "对比以下三段文本，提取应该写入学习规则的词级纠错。\n\
             ASR 原文（润色前）：{}\n\
             当前显示文本：{}\n\
             用户修改后：{}\n\n\
             以 JSON 数组输出，每项 {{\"from\":\"原词\",\"to\":\"改后词\"}}。\n\
             优先提取稳定、可复用的识别纠错或术语纠错。\n\
             如果用户最终文本已经和 ASR 原文一致，说明是当前显示文本把内容改坏了，此时提取“当前显示文本 -> 用户修改后”。\n\
             如果用户最终文本修正了 ASR 原文里的错误，也提取“ASR 原文 -> 用户修改后”。\n\
             同一处只保留最直接的一条映射，不要输出整句。如无有效差异输出空数组 []。",
            raw, before, after
        )
    } else {
        format!(
            "对比以下两句话，提取用户修改的词级别纠错。\n\
             修改前：{}\n修改后：{}\n\n\
             以 JSON 数组输出，每项 {{\"from\":\"原词\",\"to\":\"改后词\"}}。\n\
             只输出被改动的词/短语，不要输出整句。如无差异输出空数组 []。",
            before, after
        )
    };

    let system = "你是文本差异提取工具，只输出 JSON。";
    let opts = LlmRequestOptions {
        json_output: true,
        reasoning_mode: config.polish_reasoning_mode(),
        ..Default::default()
    };

    let body = llm_client::build_llm_body(
        &endpoint,
        system,
        &LlmUserInput::from(prompt.as_str()),
        opts,
    );

    let raw = match llm_client::send_llm_request(
        &state.http_client,
        &endpoint,
        &api_key,
        &body,
        prompt.len(),
        None,
        opts,
    )
    .await
    {
        Ok(content) => content,
        Err(err) => {
            log::warn!("用户纠错 LLM 请求失败: {}", err);
            return Vec::new();
        }
    };

    let raw = raw.trim();
    if raw.is_empty() {
        log::warn!("用户纠错 LLM 响应中未找到文本内容");
        return Vec::new();
    }

    log::info!("用户纠错 LLM 原始返回: {}", raw);

    // 解析 JSON 数组或包含数组的对象
    let pairs = parse_correction_pairs(raw);
    if pairs.is_empty() {
        log::info!("LLM 未提取到词级纠错，回退到本地 diff 学习");
        Vec::new()
    } else {
        log::info!("LLM 提取用户纠错: {:?}", pairs);
        pairs
    }
}

fn parse_correction_pairs(raw: &str) -> Vec<(String, String)> {
    let extract = |arr: &[serde_json::Value]| -> Vec<(String, String)> {
        arr.iter()
            .filter_map(|item| {
                let from = item["from"].as_str()?;
                let to = item["to"].as_str()?;
                (!from.is_empty() && !to.is_empty() && from != to)
                    .then(|| (from.to_string(), to.to_string()))
            })
            .collect()
    };

    if let Ok(arr) = serde_json::from_str::<Vec<serde_json::Value>>(raw) {
        return extract(&arr);
    }
    if let Ok(obj) = serde_json::from_str::<serde_json::Value>(raw) {
        if let Some(map) = obj.as_object() {
            for val in map.values() {
                if let Some(arr) = val.as_array() {
                    let pairs = extract(arr);
                    if !pairs.is_empty() {
                        return pairs;
                    }
                }
            }
        }
    }
    Vec::new()
}

#[tauri::command]
pub async fn get_user_profile(state: tauri::State<'_, AppState>) -> Result<UserProfile, String> {
    Ok(state.snapshot_profile())
}

#[tauri::command]
pub async fn add_hot_word(
    state: tauri::State<'_, AppState>,
    text: String,
    weight: u8,
) -> Result<(), String> {
    profile_service::update_profile_and_schedule(state.inner(), |profile| {
        profile_service::add_hot_word(profile, text, weight);
    });
    Ok(())
}

#[tauri::command]
pub async fn remove_hot_word(
    state: tauri::State<'_, AppState>,
    text: String,
) -> Result<(), String> {
    profile_service::update_profile_and_schedule(state.inner(), |profile| {
        profile_service::remove_hot_word(profile, &text);
    });
    Ok(())
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn set_llm_provider_config(
    app_handle: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    active: String,
    custom_base_url: Option<String>,
    custom_model: Option<String>,
    polish_reasoning_mode: Option<LlmReasoningMode>,
    assistant_reasoning_mode: Option<LlmReasoningMode>,
    assistant_use_separate_model: Option<bool>,
    assistant_model: Option<String>,
    assistant_provider: Option<Option<String>>,
    openai_auth_mode: Option<crate::state::user_profile::OpenaiAuthMode>,
) -> Result<(), String> {
    let normalized_base_url = custom_base_url
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    let normalized_model = custom_model
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    let assistant_model_provided = assistant_model.is_some();
    let normalized_assistant_model = assistant_model
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());

    profile_service::update_profile_and_schedule(state.inner(), |profile| {
        profile.llm_provider.active = active.clone();
        if let Some(mode) = polish_reasoning_mode {
            profile.llm_provider.polish_reasoning_mode = Some(mode);
            profile.llm_provider.reasoning_mode = mode;
        }
        if let Some(mode) = assistant_reasoning_mode {
            profile.llm_provider.assistant_reasoning_mode = Some(mode);
        }
        if let Some(enabled) = assistant_use_separate_model {
            profile.llm_provider.assistant_use_separate_model = enabled;
        }
        if assistant_model_provided {
            profile.llm_provider.assistant_model = normalized_assistant_model.clone();
        }
        if let Some(ap) = &assistant_provider {
            profile.llm_provider.assistant_provider = ap.clone();
        }
        if let Some(mode) = openai_auth_mode {
            profile.llm_provider.openai_auth_mode = Some(mode);
        }
        // 自定义 provider → 同步到 custom_providers，不污染旧字段
        if let Some(cp) = profile
            .llm_provider
            .custom_providers
            .iter_mut()
            .find(|p| p.id == active)
        {
            if let Some(ref url) = normalized_base_url {
                cp.base_url = url.clone();
            }
            if let Some(ref model) = normalized_model {
                cp.model = model.clone();
            }
        } else if active == "custom" {
            profile.llm_provider.custom_base_url = normalized_base_url.clone();
            profile.llm_provider.custom_model = normalized_model.clone();
        } else {
            profile.llm_provider.custom_model = normalized_model.clone();
        }
    });
    llm_provider::sync_runtime_api_key(&app_handle, state.inner());
    Ok(())
}

#[tauri::command]
pub async fn get_llm_reasoning_support(
    provider: String,
    base_url: Option<String>,
    model: Option<String>,
    api_format: Option<ApiFormat>,
) -> Result<llm_provider::LlmReasoningSupport, String> {
    let endpoint = llm_provider::endpoint_for_preview(
        provider.trim(),
        base_url.as_deref(),
        model.as_deref(),
        api_format.unwrap_or(ApiFormat::OpenaiCompat),
    );
    let uses_responses_api = endpoint.api_format == ApiFormat::OpenaiCompat
        && endpoint.api_url.contains("/v1/responses");
    Ok(llm_provider::reasoning_support(
        &endpoint,
        uses_responses_api,
    ))
}

/// 校验单个 provider 字段：name。trim 后非空、字符数不超过 128。
/// 拆出来是为了让 `update_custom_provider` 的 Optional 校验复用同一份语义。
fn validate_provider_name(name: &str) -> Result<String, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("provider 名称不能为空".into());
    }
    if name.chars().count() > 128 {
        return Err("provider 名称过长（最多 128 字符）".into());
    }
    Ok(name.to_string())
}

/// 校验单个 provider 字段：base_url。trim 后非空、是合法 URL、scheme 仅 http/https。
/// 末尾斜杠归一化掉，避免 "https://x/" 与 "https://x" 在后续比较时不一致。
fn validate_provider_base_url(base_url: &str) -> Result<String, String> {
    let url = base_url.trim();
    if url.is_empty() {
        return Err("base_url 不能为空".into());
    }
    let parsed = reqwest::Url::parse(url).map_err(|err| format!("非法 base_url: {}", err))?;
    match parsed.scheme() {
        "http" | "https" => {}
        other => {
            return Err(format!(
                "base_url 仅支持 http/https，收到 scheme: {}",
                other
            ))
        }
    }
    Ok(url.trim_end_matches('/').to_string())
}

/// 校验单个 provider 字段：model。trim 后非空。
fn validate_provider_model(model: &str) -> Result<String, String> {
    let model = model.trim();
    if model.is_empty() {
        return Err("model 不能为空".into());
    }
    Ok(model.to_string())
}

/// 校验并归一化自定义 provider 字段。
///
/// 这里不卡白名单 host（自托管 OpenAI 兼容服务很常见），但要求：
/// - name/model trim 后非空，name 控制在 128 字符内（防止前端 UI 撑爆）
/// - base_url 是合法 URL，scheme 仅允许 http/https
/// - 末尾斜杠归一化掉，避免 "https://x/" 与 "https://x" 在后续比较时不一致
fn normalize_custom_provider_fields(
    name: &str,
    base_url: &str,
    model: &str,
) -> Result<(String, String, String), String> {
    let normalized_name = validate_provider_name(name)?;
    let normalized_url = validate_provider_base_url(base_url)?;
    let normalized_model = validate_provider_model(model)?;
    Ok((normalized_name, normalized_url, normalized_model))
}

#[tauri::command]
pub async fn add_custom_provider(
    state: tauri::State<'_, AppState>,
    name: String,
    base_url: String,
    model: String,
    api_format: ApiFormat,
) -> Result<String, String> {
    let (name, base_url, model) = normalize_custom_provider_fields(&name, &base_url, &model)?;
    // 用毫秒时间戳生成 id 在 UI 快速点击时可能撞 ID（同一毫秒两次添加）；
    // 拼一段随机后缀 + 在 push 前查重，确保 id 唯一。
    use rand::Rng;
    let mut rng = rand::thread_rng();
    let mut id = format!(
        "custom_{}_{:08x}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis(),
        rng.gen::<u32>()
    );
    let assigned_id = profile_service::update_profile_and_schedule(state.inner(), |profile| {
        // 极小概率撞 ID 时再随机一次。
        while profile
            .llm_provider
            .custom_providers
            .iter()
            .any(|p| p.id == id)
        {
            id = format!("custom_{:016x}", rng.gen::<u64>());
        }
        let provider = CustomProvider {
            id: id.clone(),
            name: name.clone(),
            base_url: base_url.clone(),
            model: model.clone(),
            api_format,
        };
        profile.llm_provider.custom_providers.push(provider);
        id.clone()
    });
    Ok(assigned_id)
}

#[tauri::command]
pub async fn update_custom_provider(
    state: tauri::State<'_, AppState>,
    id: String,
    name: Option<String>,
    base_url: Option<String>,
    model: Option<String>,
    api_format: Option<ApiFormat>,
) -> Result<(), String> {
    // 先校验入参，再进 profile 闭包。这样校验失败不会污染 profile 状态。
    let normalized_name = match name.as_deref() {
        Some(n) => Some(validate_provider_name(n)?),
        None => None,
    };
    let normalized_url = match base_url.as_deref() {
        Some(u) => Some(validate_provider_base_url(u)?),
        None => None,
    };
    let normalized_model = match model.as_deref() {
        Some(m) => Some(validate_provider_model(m)?),
        None => None,
    };

    let found = profile_service::update_profile_and_schedule(state.inner(), |profile| {
        if let Some(cp) = profile
            .llm_provider
            .custom_providers
            .iter_mut()
            .find(|p| p.id == id)
        {
            if let Some(n) = &normalized_name {
                cp.name = n.clone();
            }
            if let Some(u) = &normalized_url {
                cp.base_url = u.clone();
            }
            if let Some(m) = &normalized_model {
                cp.model = m.clone();
            }
            if let Some(f) = api_format {
                cp.api_format = f;
            }
            true
        } else {
            false
        }
    });
    if !found {
        return Err(format!("找不到自定义服务商: {}", id));
    }
    Ok(())
}

#[tauri::command]
pub async fn remove_custom_provider(
    app_handle: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    id: String,
) -> Result<(), String> {
    profile_service::update_profile_and_schedule(state.inner(), |profile| {
        let fallback_provider = profile.llm_provider.fallback_provider_after_removal(&id);
        profile.llm_provider.custom_providers.retain(|p| p.id != id);
        if profile.llm_provider.active == id {
            profile.llm_provider.active = fallback_provider;
        }
        if profile.llm_provider.assistant_provider.as_deref() == Some(&*id) {
            profile.llm_provider.assistant_provider = None;
        }
    });
    llm_provider::sync_runtime_api_key(&app_handle, state.inner());
    Ok(())
}

/// 设置翻译目标语言。返回是否自动开启了 AI 润色。
#[tauri::command]
pub async fn set_translation_target(
    state: tauri::State<'_, AppState>,
    target: Option<String>,
) -> Result<bool, String> {
    let target = target
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    let auto_enabled_polish =
        target.is_some() && !state.profile.ai_polish_enabled.load(Ordering::Acquire);

    if auto_enabled_polish {
        state
            .profile
            .ai_polish_enabled
            .store(true, Ordering::Release);
    }

    profile_service::update_profile_and_schedule(state.inner(), |profile| {
        profile.translation_target = target;
    });
    Ok(auto_enabled_polish)
}

#[tauri::command]
pub async fn set_translation_hotkey(
    app_handle: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    shortcut: Option<String>,
) -> Result<(), String> {
    let normalized = shortcut
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);

    crate::commands::hotkey::register_translation_hotkey_inner(app_handle, normalized.clone())
        .map_err(|err| err.to_string())?;

    profile_service::update_profile_and_schedule(state.inner(), |profile| {
        profile.translation_hotkey = normalized;
    });
    Ok(())
}

#[tauri::command]
pub async fn set_openai_fast_mode(
    state: tauri::State<'_, AppState>,
    enabled: bool,
) -> Result<(), String> {
    profile_service::update_profile_and_schedule(state.inner(), |profile| {
        profile.llm_provider.openai_fast_mode = enabled;
    });
    Ok(())
}

#[tauri::command]
pub async fn set_custom_prompt(
    state: tauri::State<'_, AppState>,
    prompt: Option<String>,
) -> Result<(), String> {
    let prompt = prompt
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    profile_service::update_profile_and_schedule(state.inner(), |p| {
        p.custom_prompt = prompt;
    });
    Ok(())
}

#[tauri::command]
pub async fn export_user_profile(state: tauri::State<'_, AppState>) -> Result<String, String> {
    serde_json::to_string_pretty(&state.snapshot_profile())
        .map_err(|e| format!("序列化失败: {}", e))
}

#[tauri::command]
pub async fn import_user_profile(
    app_handle: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    json_data: String,
) -> Result<(), String> {
    let imported: UserProfile =
        serde_json::from_str(&json_data).map_err(|e| format!("解析画像数据失败: {}", e))?;
    let (_, profile) = state.update_profile(|profile| {
        *profile = imported;
        profile_service::normalize_profile(profile);
    });
    profile_service::save_profile_async(&profile)
        .await
        .map_err(|e| format!("保存用户画像失败: {}", e))?;
    llm_provider::sync_runtime_api_key(&app_handle, state.inner());
    Ok(())
}

/// LLM 审核核心逻辑，供命令和定期任务共用
pub async fn run_correction_validation(
    app_handle: &tauri::AppHandle,
    state: &AppState,
) -> Result<u32, String> {
    let config = state.llm_provider_config();
    let endpoint = if config.validation_use_separate_model {
        llm_provider::validation_endpoint_for_config(&config)
    } else {
        llm_provider::endpoint_for_config(&config)
    };

    let api_key = llm_provider::load_api_key_for_provider(app_handle, &endpoint.provider);
    if api_key.is_empty() {
        return Err("未配置 API Key，无法审核纠错规则".into());
    }

    let ai_corrections: Vec<(String, String)> = state.with_profile(|p| {
        p.correction_patterns
            .iter()
            .filter(|c| c.source == CorrectionSource::Ai)
            .map(|c| (c.original.clone(), c.corrected.clone()))
            .collect()
    });

    if ai_corrections.is_empty() {
        update_validation_timestamp(state);
        return Ok(0);
    }

    let mut all_invalid: std::collections::HashSet<(String, String)> =
        std::collections::HashSet::new();

    for chunk in ai_corrections.chunks(40) {
        let mut rules_text = String::new();
        for (i, (orig, corrected)) in chunk.iter().enumerate() {
            rules_text.push_str(&format!("{}. \"{}\" → \"{}\"\n", i + 1, orig, corrected));
        }

        let prompt = format!(
            "以下是语音识别自动纠错系统学到的 {} 条纠错规则。请逐条审核。\n\n\
             合理的规则：同音字/近音字纠正、专有名词大小写、常见 ASR 误识别修复\n\
             不合理的规则：语义无关的替换、对话碎片误学、过度泛化（如常见词映射到不相关的词）\n\n\
             规则列表：\n{}\n\
             以 JSON 数组输出不合理的编号，例如 [2,5,7]。如果全部合理输出 []。只输出 JSON。",
            chunk.len(),
            rules_text
        );

        let opts = LlmRequestOptions {
            json_output: true,
            reasoning_mode: config.polish_reasoning_mode(),
            ..Default::default()
        };

        let body = llm_client::build_llm_body(
            &endpoint,
            "你是纠错规则质量审核工具，只输出 JSON。",
            &LlmUserInput::from(prompt.as_str()),
            opts,
        );

        let raw = match llm_client::send_llm_request(
            &state.http_client,
            &endpoint,
            &api_key,
            &body,
            prompt.len(),
            None,
            opts,
        )
        .await
        {
            Ok(content) => content,
            Err(err) => {
                log::warn!("纠错审核 LLM 请求失败: {}", err);
                continue;
            }
        };

        let invalid_indices = parse_invalid_indices(raw.trim());
        for idx in invalid_indices {
            if idx >= 1 && idx <= chunk.len() {
                let (ref orig, ref corrected) = chunk[idx - 1];
                all_invalid.insert((orig.clone(), corrected.clone()));
            }
        }
    }

    if all_invalid.is_empty() {
        update_validation_timestamp(state);
        return Ok(0);
    }

    let removed = all_invalid.len() as u32;
    log::info!("LLM 审核删除 {} 条 AI 纠错规则", removed);

    profile_service::update_profile_and_schedule(state, |profile| {
        profile.correction_patterns.retain(|p| {
            p.source == CorrectionSource::User
                || !all_invalid.contains(&(p.original.clone(), p.corrected.clone()))
        });
    });
    update_validation_timestamp(state);

    Ok(removed)
}

/// LLM 审核 AI 来源的纠错规则（Tauri command 入口）
#[tauri::command]
pub async fn validate_corrections(
    app_handle: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<u32, String> {
    run_correction_validation(&app_handle, state.inner()).await
}

#[tauri::command]
pub async fn set_correction_validation_config(
    state: tauri::State<'_, AppState>,
    enabled: bool,
    use_separate_model: Option<bool>,
    provider: Option<Option<String>>,
    model: Option<Option<String>>,
) -> Result<(), String> {
    profile_service::update_profile_and_schedule(state.inner(), |profile| {
        profile.correction_validation_enabled = enabled;
        if let Some(sep) = use_separate_model {
            profile.llm_provider.validation_use_separate_model = sep;
        }
        if let Some(p) = provider {
            profile.llm_provider.validation_provider = p;
        }
        if let Some(m) = model {
            profile.llm_provider.validation_model = m;
        }
    });
    Ok(())
}

#[tauri::command]
pub async fn remove_correction(
    state: tauri::State<'_, AppState>,
    original: String,
    corrected: String,
) -> Result<(), String> {
    profile_service::update_profile_and_schedule(state.inner(), |profile| {
        profile
            .correction_patterns
            .retain(|p| !(p.original == original && p.corrected == corrected));
    });
    Ok(())
}

fn update_validation_timestamp(state: &AppState) {
    profile_service::update_profile_and_schedule(state, |profile| {
        profile.last_correction_validation = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
    });
}

fn parse_invalid_indices(raw: &str) -> Vec<usize> {
    if let Ok(arr) = serde_json::from_str::<Vec<serde_json::Value>>(raw) {
        return arr
            .iter()
            .filter_map(|v| {
                v.as_u64()
                    .map(|n| n as usize)
                    .or_else(|| v.as_f64().map(|n| n as usize))
            })
            .collect();
    }
    if let Ok(obj) = serde_json::from_str::<serde_json::Value>(raw) {
        if let Some(map) = obj.as_object() {
            for val in map.values() {
                if let Some(arr) = val.as_array() {
                    return arr
                        .iter()
                        .filter_map(|v| {
                            v.as_u64()
                                .map(|n| n as usize)
                                .or_else(|| v.as_f64().map(|n| n as usize))
                        })
                        .collect();
                }
            }
        }
    }
    Vec::new()
}

#[cfg(test)]
mod validator_tests {
    use super::*;

    // --- validate_provider_name ------------------------------------------
    //
    // 目的：把 trim、空值、长度上限三件事按合同钉死。这些字符串从前端
    // 直接落到 profile 里，没人会再清洗一遍——校验不严会让用户保存到
    // 一个让 UI 显示崩溃（128 字符限制）或永远跑不通（空字符串）的
    // provider。

    #[test]
    fn validate_provider_name_rejects_empty_string() {
        // 完全空串必须报"不能为空"——前端直接显示此错误。
        let err = validate_provider_name("").expect_err("空字符串必须 Err");
        assert_eq!(err, "provider 名称不能为空");
    }

    #[test]
    fn validate_provider_name_rejects_whitespace_only() {
        // 全空白等价于空串：必须 trim 后再判，否则用户用空格"保存"也算通过。
        let err = validate_provider_name("   ").expect_err("纯空白必须 Err");
        assert_eq!(err, "provider 名称不能为空");
    }

    #[test]
    fn validate_provider_name_accepts_exactly_128_chars() {
        // 128 是上限（含），不是 127。这条边界把 off-by-one 钉死。
        let name: String = "a".repeat(128);
        let result = validate_provider_name(&name);
        assert_eq!(
            result.as_deref(),
            Ok(name.as_str()),
            "128 字符必须接受（边界含上限）"
        );
    }

    #[test]
    fn validate_provider_name_rejects_129_chars() {
        // 上限 +1 一定要 Err，且错误文案对应"过长"。
        let name: String = "a".repeat(129);
        let err = validate_provider_name(&name).expect_err("129 字符必须 Err");
        assert_eq!(err, "provider 名称过长（最多 128 字符）");
    }

    #[test]
    fn validate_provider_name_trims_outer_whitespace() {
        // 返回值必须是 trim 后的字符串。否则保存进去的名字会带前后空格，
        // 后续比较/显示会出现幽灵空白。
        let result = validate_provider_name("  abc  ");
        assert_eq!(result.as_deref(), Ok("abc"), "返回值必须是 trim 后的字符串");
    }

    #[test]
    fn validate_provider_name_counts_chars_not_bytes() {
        // 100 个 ASCII + 30 个 CJK 共 130 char（每个 CJK 在 UTF-8 里占 3 字节，
        // 总字节 100 + 30*3 = 190）。如果实现错把 .len() 当字符数：
        //   - 190 > 128 → 这条会 Err，看起来"对"——但 100 ASCII 也会错判
        // 如果实现按 .chars().count()：
        //   - 130 > 128 → Err，符合合同
        // 这条断言只钉一种情况，但一同盯着字符数语义。
        let mut name = String::with_capacity(190);
        for _ in 0..100 {
            name.push('a');
        }
        for _ in 0..30 {
            name.push('文'); // 多字节 CJK
        }
        assert_eq!(name.chars().count(), 130, "前置条件：130 个字符");
        let err = validate_provider_name(&name).expect_err("130 字符必须 Err");
        assert_eq!(
            err, "provider 名称过长（最多 128 字符）",
            "字符计数必须基于 chars() 而非 bytes"
        );
    }

    // --- validate_provider_base_url --------------------------------------
    //
    // 目的：拒绝非法 URL，禁止 http/https 之外的 scheme（防止 file://
    // 之类被无意保存触发后续 reqwest 行为不一致），并把末尾斜杠归一化掉
    // 以避免 "https://x/" 与 "https://x" 在比较/拼接时不一致。

    #[test]
    fn validate_provider_base_url_rejects_empty_string() {
        let err = validate_provider_base_url("").expect_err("空 base_url 必须 Err");
        assert_eq!(err, "base_url 不能为空");
    }

    #[test]
    fn validate_provider_base_url_rejects_whitespace_only() {
        // 同 name：必须先 trim 再判空，否则空格能"骗"过校验。
        let err = validate_provider_base_url("   ").expect_err("纯空白 base_url 必须 Err");
        assert_eq!(err, "base_url 不能为空");
    }

    #[test]
    fn validate_provider_base_url_rejects_garbage_string() {
        // 不能解析为 URL 时必须用"非法 base_url:"前缀，方便前端 grep 错误类型。
        let err =
            validate_provider_base_url("not a url at all").expect_err("非 URL 字符串必须 Err");
        assert!(
            err.starts_with("非法 base_url: "),
            "错误必须以 \"非法 base_url: \" 开头；got {:?}",
            err
        );
    }

    #[test]
    fn validate_provider_base_url_rejects_ftp_scheme() {
        // ftp 是合法 URL scheme 但我们不支持。错误必须显式提示 scheme 名称，
        // 否则用户很难懂为什么"看起来像 URL"的字符串被拒了。
        let err = validate_provider_base_url("ftp://example.com").expect_err("ftp scheme 必须 Err");
        assert!(
            err.contains("仅支持 http/https"),
            "错误必须包含\"仅支持 http/https\"；got {:?}",
            err
        );
        assert!(
            err.contains("ftp"),
            "错误必须显式提到收到的 scheme \"ftp\"；got {:?}",
            err
        );
    }

    #[test]
    fn validate_provider_base_url_rejects_file_scheme() {
        // file:// 也是常见误填，必须拒掉以防止保存后引发 reqwest 行为异常。
        let err = validate_provider_base_url("file:///x").expect_err("file scheme 必须 Err");
        assert!(
            err.contains("file"),
            "错误必须提到 \"file\" scheme；got {:?}",
            err
        );
    }

    #[test]
    fn validate_provider_base_url_accepts_plain_http() {
        // 自托管（局域网/dev）经常用 http；不能误拒。
        let result = validate_provider_base_url("http://example.com");
        assert_eq!(
            result.as_deref(),
            Ok("http://example.com"),
            "http URL 必须被接受且原样返回"
        );
    }

    #[test]
    fn validate_provider_base_url_strips_single_trailing_slash() {
        // 末尾斜杠归一化：避免 "https://x/" 与 "https://x" 后续在
        // identity 比较/字符串拼接时出现"看起来一样但 != " 的诡异 bug。
        let result = validate_provider_base_url("https://example.com/");
        assert_eq!(
            result.as_deref(),
            Ok("https://example.com"),
            "末尾单斜杠必须被剥掉"
        );
    }

    #[test]
    fn validate_provider_base_url_strips_multiple_trailing_slashes() {
        // 用户可能粘贴 "https://x///"，trim_end_matches('/') 必须把所有
        // 尾随斜杠一次性剥掉，而不是只剥一个。
        let result = validate_provider_base_url("https://example.com///");
        assert_eq!(
            result.as_deref(),
            Ok("https://example.com"),
            "所有尾随斜杠都必须被一次剥光"
        );
    }

    #[test]
    fn validate_provider_base_url_trims_outer_whitespace_before_parse() {
        // 复制粘贴常带前后空格；这些必须在 parse 前 trim 掉，
        // 否则 reqwest::Url::parse 会因为前导空白直接报"非法 base_url"。
        let result = validate_provider_base_url("  https://example.com  ");
        assert_eq!(
            result.as_deref(),
            Ok("https://example.com"),
            "外层空白必须先 trim 再 parse"
        );
    }

    // --- validate_provider_model -----------------------------------------
    //
    // model 校验合同最简单：trim 后非空即可，且返回 trim 后的值。
    // 这里没有长度上限——provider 自己规定 model 名长度。

    #[test]
    fn validate_provider_model_rejects_empty_string() {
        let err = validate_provider_model("").expect_err("空 model 必须 Err");
        assert_eq!(err, "model 不能为空");
    }

    #[test]
    fn validate_provider_model_rejects_whitespace_only() {
        let err = validate_provider_model("   ").expect_err("纯空白 model 必须 Err");
        assert_eq!(err, "model 不能为空");
    }

    #[test]
    fn validate_provider_model_trims_outer_whitespace() {
        let result = validate_provider_model("  gpt-4  ");
        assert_eq!(
            result.as_deref(),
            Ok("gpt-4"),
            "返回值必须是 trim 后的字符串"
        );
    }
}
