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

#[tauri::command]
pub async fn add_custom_provider(
    state: tauri::State<'_, AppState>,
    name: String,
    base_url: String,
    model: String,
    api_format: ApiFormat,
) -> Result<String, String> {
    let id = format!(
        "custom_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
    );
    let provider = CustomProvider {
        id: id.clone(),
        name,
        base_url,
        model,
        api_format,
    };
    profile_service::update_profile_and_schedule(state.inner(), |profile| {
        profile.llm_provider.custom_providers.push(provider);
    });
    Ok(id)
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
    let found = profile_service::update_profile_and_schedule(state.inner(), |profile| {
        if let Some(cp) = profile
            .llm_provider
            .custom_providers
            .iter_mut()
            .find(|p| p.id == id)
        {
            if let Some(n) = name {
                cp.name = n;
            }
            if let Some(u) = base_url {
                cp.base_url = u;
            }
            if let Some(m) = model {
                cp.model = m;
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
