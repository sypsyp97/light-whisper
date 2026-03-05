use tauri_plugin_keyring::KeyringExt;

use crate::services::{llm_provider, profile_service};
use crate::state::user_profile::*;
use crate::state::AppState;

#[tauri::command]
pub async fn submit_user_correction(
    state: tauri::State<'_, AppState>,
    original: String,
    corrected: String,
) -> Result<(), String> {
    // 用 LLM 对比两句话，提取词级纠错
    let corrections = extract_corrections_via_llm(&state, &original, &corrected).await;

    let profile_clone = {
        let mut profile = state.user_profile.lock().map_err(|e| e.to_string())?;
        profile_service::learn_from_structured(
            &mut profile,
            &corrections,
            &[],
            CorrectionSource::User,
        );
        profile.clone()
    };
    profile_service::save_profile_async(&profile_clone)
        .await
        .map_err(|e| format!("保存用户画像失败: {}", e))
}

/// 调用 LLM 对比润色前后文本，提取词级别纠错
async fn extract_corrections_via_llm(
    state: &AppState,
    before: &str,
    after: &str,
) -> Vec<(String, String)> {
    let api_key = match state.ai_polish_api_key.lock() {
        Ok(k) => k.clone(),
        Err(p) => p.into_inner().clone(),
    };
    if api_key.is_empty() {
        return vec![(before.to_string(), after.to_string())];
    }

    let endpoint = {
        let profile = match state.user_profile.lock() {
            Ok(p) => p.clone(),
            Err(p) => p.into_inner().clone(),
        };
        llm_provider::get_endpoint(
            &profile.llm_provider.active,
            profile.llm_provider.custom_base_url.as_deref(),
            profile.llm_provider.custom_model.as_deref(),
        )
    };

    let prompt = format!(
        "对比以下两句话，提取用户修改的词级别纠错。\n\
         修改前：{}\n修改后：{}\n\n\
         以 JSON 数组输出，每项 {{\"from\":\"原词\",\"to\":\"改后词\"}}。\n\
         只输出被改动的词/短语，不要输出整句。如无差异输出空数组 []。",
        before, after
    );

    let is_responses_api = endpoint.api_url.contains("/v1/responses");
    let body = if is_responses_api {
        serde_json::json!({
            "model": endpoint.model,
            "instructions": "你是文本差异提取工具，只输出 JSON。",
            "input": [
                {"role": "developer", "content": "Output json."},
                {"role": "user", "content": prompt},
            ],
            "text": { "format": { "type": "json_object" } },
            "reasoning": { "effort": "medium" },
        })
    } else {
        serde_json::json!({
            "model": endpoint.model,
            "messages": [
                {"role": "system", "content": "你是文本差异提取工具，只输出 JSON。"},
                {"role": "user", "content": prompt},
            ],
            "response_format": { "type": "json_object" },
        })
    };

    let resp = match state
        .http_client
        .post(&endpoint.api_url)
        .header("Authorization", format!("Bearer {}", api_key))
        .header("Content-Type", "application/json")
        .timeout(std::time::Duration::from_secs(endpoint.timeout_secs))
        .json(&body)
        .send()
        .await
    {
        Ok(r) if r.status().is_success() => r,
        Ok(r) => {
            let status = r.status();
            let body = r.text().await.unwrap_or_default();
            log::warn!("用户纠错 LLM 请求失败 {}: {}", status, body);
            return vec![(before.to_string(), after.to_string())];
        }
        Err(e) => {
            log::warn!("用户纠错 LLM 网络错误: {}", e);
            return vec![(before.to_string(), after.to_string())];
        }
    };

    let json: serde_json::Value = match resp.json().await {
        Ok(j) => j,
        Err(e) => {
            log::warn!("用户纠错 LLM 响应解析失败: {}", e);
            return vec![(before.to_string(), after.to_string())];
        }
    };

    // 从响应中提取文本
    let raw = if is_responses_api {
        json["output"].as_array().and_then(|o| {
            o.iter().find_map(|item| {
                if item["type"].as_str() == Some("message") {
                    item["content"][0]["text"].as_str()
                } else {
                    None
                }
            })
        })
    } else {
        json["choices"][0]["message"]["content"].as_str()
    };

    let raw = match raw {
        Some(s) => s.trim(),
        None => {
            log::warn!("用户纠错 LLM 响应中未找到文本内容");
            return vec![(before.to_string(), after.to_string())];
        }
    };

    log::info!("用户纠错 LLM 原始返回: {}", raw);

    // 解析 JSON 数组或包含数组的对象
    let pairs = parse_correction_pairs(raw);
    if pairs.is_empty() {
        // LLM 认为无差异，但用户确实改了，存整句兜底
        vec![(before.to_string(), after.to_string())]
    } else {
        log::info!("LLM 提取用户纠错: {:?}", pairs);
        pairs
    }
}

fn parse_correction_pairs(raw: &str) -> Vec<(String, String)> {
    // 尝试直接解析为数组
    if let Ok(arr) = serde_json::from_str::<Vec<serde_json::Value>>(raw) {
        return arr
            .iter()
            .filter_map(|item| {
                let from = item["from"].as_str()?;
                let to = item["to"].as_str()?;
                if !from.is_empty() && !to.is_empty() && from != to {
                    Some((from.to_string(), to.to_string()))
                } else {
                    None
                }
            })
            .collect();
    }
    // json_object 模式可能返回 {"corrections": [...]} 或其他 key
    if let Ok(obj) = serde_json::from_str::<serde_json::Value>(raw) {
        // 遍历对象中所有值为数组的字段
        if let Some(map) = obj.as_object() {
            for (_key, val) in map {
                if let Some(arr) = val.as_array() {
                    let pairs: Vec<_> = arr
                        .iter()
                        .filter_map(|item| {
                            let from = item["from"].as_str()?;
                            let to = item["to"].as_str()?;
                            if !from.is_empty() && !to.is_empty() && from != to {
                                Some((from.to_string(), to.to_string()))
                            } else {
                                None
                            }
                        })
                        .collect();
                    if !pairs.is_empty() {
                        return pairs;
                    }
                }
            }
        }
    }
    vec![]
}

#[tauri::command]
pub async fn get_user_profile(state: tauri::State<'_, AppState>) -> Result<UserProfile, String> {
    let profile = state.user_profile.lock().map_err(|e| e.to_string())?;
    Ok(profile.clone())
}

#[tauri::command]
pub async fn add_hot_word(
    state: tauri::State<'_, AppState>,
    text: String,
    weight: u8,
) -> Result<(), String> {
    let mut profile = state.user_profile.lock().map_err(|e| e.to_string())?;
    profile_service::add_hot_word(&mut profile, text, weight);
    profile_service::save_profile(&profile)
}

#[tauri::command]
pub async fn remove_hot_word(
    state: tauri::State<'_, AppState>,
    text: String,
) -> Result<(), String> {
    let mut profile = state.user_profile.lock().map_err(|e| e.to_string())?;
    profile_service::remove_hot_word(&mut profile, &text);
    profile_service::save_profile(&profile)
}

#[tauri::command]
pub async fn set_llm_provider_config(
    app_handle: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    active: String,
    custom_base_url: Option<String>,
    custom_model: Option<String>,
) -> Result<(), String> {
    {
        let mut profile = state.user_profile.lock().map_err(|e| e.to_string())?;
        profile.llm_provider = LlmProviderConfig {
            active: active.clone(),
            custom_base_url,
            custom_model,
        };
        profile_service::save_profile(&profile)?;
    }

    // 从 keyring 加载新 provider 的 API Key 到内存
    let keyring_user = llm_provider::keyring_user_for_provider(&active);
    let new_key = app_handle
        .keyring()
        .get_password("light-whisper", keyring_user)
        .ok()
        .flatten()
        .unwrap_or_default();
    match state.ai_polish_api_key.lock() {
        Ok(mut key) => *key = new_key,
        Err(poisoned) => *poisoned.into_inner() = new_key,
    }

    Ok(())
}

#[tauri::command]
pub async fn export_user_profile(state: tauri::State<'_, AppState>) -> Result<String, String> {
    let profile = state.user_profile.lock().map_err(|e| e.to_string())?;
    serde_json::to_string_pretty(&*profile).map_err(|e| format!("序列化失败: {}", e))
}

#[tauri::command]
pub async fn import_user_profile(
    state: tauri::State<'_, AppState>,
    json_data: String,
) -> Result<(), String> {
    let imported: UserProfile =
        serde_json::from_str(&json_data).map_err(|e| format!("解析画像数据失败: {}", e))?;
    let mut profile = state.user_profile.lock().map_err(|e| e.to_string())?;
    *profile = imported;
    profile_service::cleanup_profile(&mut profile);
    profile_service::save_profile(&profile)
}
