use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// 热词来源
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum HotWordSource {
    /// 用户手动添加
    User,
    /// 从纠错中自动学习
    Learned,
}

/// 纠错来源
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "lowercase")]
pub enum CorrectionSource {
    /// AI 润色自动学习
    #[default]
    Ai,
    /// 用户手动纠错
    User,
}

/// 热词条目
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HotWord {
    pub text: String,
    /// 权重 1-5，越高越优先
    pub weight: u8,
    pub source: HotWordSource,
    pub use_count: u32,
    /// Unix 时间戳（秒）
    pub last_used: u64,
}

/// 纠错模式记录
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CorrectionPattern {
    /// ASR 原始输出片段
    pub original: String,
    /// 纠正后的片段
    pub corrected: String,
    /// 出现次数
    pub count: u32,
    /// 最近一次出现的时间戳
    pub last_seen: u64,
    /// 纠错来源
    #[serde(default)]
    pub source: CorrectionSource,
}

/// 词频条目
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VocabEntry {
    pub count: u32,
    pub last_seen: u64,
}

/// 用户画像
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UserProfile {
    pub hot_words: Vec<HotWord>,
    pub correction_patterns: Vec<CorrectionPattern>,
    pub vocab_frequency: HashMap<String, VocabEntry>,
    pub total_transcriptions: u64,
    /// 上次更新时间（Unix 时间戳）
    pub last_updated: u64,
    /// LLM 后端配置
    pub llm_provider: LlmProviderConfig,
    /// 翻译目标语言（None = 关闭翻译，非空 = 开启并翻译为该语言）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub translation_target: Option<String>,
    /// 翻译模式独立热键
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub translation_hotkey: Option<String>,
    /// 用户自定义润色指令
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub custom_prompt: Option<String>,
    /// 助手模式独立热键
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assistant_hotkey: Option<String>,
    /// 助手模式附加系统提示词
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assistant_system_prompt: Option<String>,
    /// 助手模式是否附带当前屏幕截图作为上下文
    #[serde(default)]
    pub assistant_screen_context_enabled: bool,
    /// 用户手动删除后，不再自动学习回来的热词黑名单
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub blocked_hot_words: Vec<String>,
}

/// API 协议格式
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "snake_case")]
pub enum ApiFormat {
    #[default]
    OpenaiCompat,
    Anthropic,
}

/// 推理/思考模式（跨供应商抽象层）
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum LlmReasoningMode {
    /// 不下发任何推理参数，走供应商默认行为
    #[default]
    ProviderDefault,
    /// 尽量关闭或压低思考
    Off,
    /// 偏轻量
    Light,
    /// 标准
    Balanced,
    /// 偏深度
    Deep,
}

/// 用户自定义的 LLM 服务商
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CustomProvider {
    pub id: String,
    pub name: String,
    pub base_url: String,
    pub model: String,
    #[serde(default)]
    pub api_format: ApiFormat,
}

/// LLM 后端配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmProviderConfig {
    /// 当前使用的后端: 预置 key 或 custom provider id
    pub active: String,
    /// 旧字段，迁移兼容
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub custom_base_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub custom_model: Option<String>,
    /// 跨供应商的思考模式抽象层
    #[serde(default)]
    pub reasoning_mode: LlmReasoningMode,
    /// AI 润色链路的独立思考模式；为空时回退到旧的 reasoning_mode
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub polish_reasoning_mode: Option<LlmReasoningMode>,
    /// AI 助手链路的独立思考模式；为空时回退到旧的 reasoning_mode
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assistant_reasoning_mode: Option<LlmReasoningMode>,
    /// AI 助手是否使用不同于润色的独立模型
    #[serde(default)]
    pub assistant_use_separate_model: bool,
    /// AI 助手独立模型；仅在 assistant_use_separate_model = true 时生效
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assistant_model: Option<String>,
    /// 用户自定义服务商列表
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub custom_providers: Vec<CustomProvider>,
}

impl Default for LlmProviderConfig {
    fn default() -> Self {
        Self {
            active: "cerebras".to_string(),
            custom_base_url: None,
            custom_model: None,
            reasoning_mode: LlmReasoningMode::ProviderDefault,
            polish_reasoning_mode: None,
            assistant_reasoning_mode: None,
            assistant_use_separate_model: false,
            assistant_model: None,
            custom_providers: Vec::new(),
        }
    }
}

impl LlmProviderConfig {
    fn is_builtin_provider(provider: &str) -> bool {
        matches!(
            provider,
            "cerebras" | "openai" | "deepseek" | "siliconflow" | "custom"
        )
    }

    pub fn resolve_active_provider(&self) -> String {
        if Self::is_builtin_provider(&self.active)
            || self.custom_providers.iter().any(|p| p.id == self.active)
        {
            return self.active.clone();
        }

        self.custom_providers
            .last()
            .map(|provider| provider.id.clone())
            .unwrap_or_else(|| "cerebras".to_string())
    }

    pub fn fallback_provider_after_removal(&self, removed_id: &str) -> String {
        if self.active != removed_id {
            return self.resolve_active_provider();
        }

        if let Some(index) = self
            .custom_providers
            .iter()
            .position(|p| p.id == removed_id)
        {
            if index > 0 {
                return self.custom_providers[index - 1].id.clone();
            }

            if let Some(next) = self
                .custom_providers
                .iter()
                .enumerate()
                .rev()
                .find(|(idx, _)| *idx != index)
                .map(|(_, provider)| provider.id.clone())
            {
                return next;
            }
        }

        "cerebras".to_string()
    }

    pub fn polish_reasoning_mode(&self) -> LlmReasoningMode {
        self.polish_reasoning_mode.unwrap_or(self.reasoning_mode)
    }

    pub fn assistant_reasoning_mode(&self) -> LlmReasoningMode {
        self.assistant_reasoning_mode.unwrap_or(self.reasoning_mode)
    }

    pub fn assistant_model(&self) -> Option<&str> {
        if !self.assistant_use_separate_model {
            return None;
        }
        self.assistant_model
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
    }
}

impl UserProfile {
    /// 获取按权重排序的热词文本列表（用于 ASR 注入）
    pub fn get_hot_word_texts(&self, limit: usize) -> Vec<String> {
        let mut words: Vec<&HotWord> = self.hot_words.iter().collect();
        words.sort_by(|a, b| b.weight.cmp(&a.weight).then(b.use_count.cmp(&a.use_count)));
        words
            .into_iter()
            .take(limit)
            .map(|w| w.text.clone())
            .collect()
    }

    /// 根据输入文本动态检索相关纠错模式
    ///
    /// 策略：精确子串匹配优先（input 中包含 pattern.original），
    /// 不足 limit 条时用高频纠错兜底。User 来源始终优先于 Ai。
    pub fn get_relevant_corrections(&self, input: &str, limit: usize) -> Vec<&CorrectionPattern> {
        let source_ord = |s: &CorrectionSource| match s {
            CorrectionSource::User => 0,
            CorrectionSource::Ai => 1,
        };

        // 第一轮：精确子串命中
        let mut matched: Vec<&CorrectionPattern> = self
            .correction_patterns
            .iter()
            .filter(|p| !p.original.is_empty() && input.contains(&p.original))
            .collect();
        matched.sort_by(|a, b| {
            source_ord(&a.source)
                .cmp(&source_ord(&b.source))
                .then(b.count.cmp(&a.count))
        });
        matched.truncate(limit);

        if matched.len() >= limit {
            return matched;
        }

        // 第二轮：高频兜底，补足剩余名额
        let remaining = limit - matched.len();
        let matched_set: std::collections::HashSet<(&str, &str)> = matched
            .iter()
            .map(|p| (p.original.as_str(), p.corrected.as_str()))
            .collect();

        let mut fallback: Vec<&CorrectionPattern> = self
            .correction_patterns
            .iter()
            .filter(|p| !matched_set.contains(&(p.original.as_str(), p.corrected.as_str())))
            .collect();
        fallback.sort_by(|a, b| {
            source_ord(&a.source)
                .cmp(&source_ord(&b.source))
                .then(b.count.cmp(&a.count))
        });

        matched.extend(fallback.into_iter().take(remaining));
        matched
    }
}

#[cfg(test)]
mod tests {
    use super::{ApiFormat, CustomProvider, LlmProviderConfig, LlmReasoningMode};

    fn custom_provider(id: &str) -> CustomProvider {
        CustomProvider {
            id: id.to_string(),
            name: id.to_string(),
            base_url: format!("https://{id}.example.com"),
            model: format!("model-{id}"),
            api_format: ApiFormat::OpenaiCompat,
        }
    }

    #[test]
    fn resolves_invalid_active_provider_to_latest_custom_provider() {
        let config = LlmProviderConfig {
            active: "missing".to_string(),
            custom_base_url: None,
            custom_model: None,
            reasoning_mode: Default::default(),
            polish_reasoning_mode: None,
            assistant_reasoning_mode: None,
            assistant_use_separate_model: false,
            assistant_model: None,
            custom_providers: vec![custom_provider("a"), custom_provider("b")],
        };

        assert_eq!(config.resolve_active_provider(), "b");
    }

    #[test]
    fn falls_back_to_previous_provider_after_removal() {
        let config = LlmProviderConfig {
            active: "b".to_string(),
            custom_base_url: None,
            custom_model: None,
            reasoning_mode: Default::default(),
            polish_reasoning_mode: None,
            assistant_reasoning_mode: None,
            assistant_use_separate_model: false,
            assistant_model: None,
            custom_providers: vec![
                custom_provider("a"),
                custom_provider("b"),
                custom_provider("c"),
            ],
        };

        assert_eq!(config.fallback_provider_after_removal("b"), "a");
    }

    #[test]
    fn falls_back_to_last_remaining_provider_when_removing_first() {
        let config = LlmProviderConfig {
            active: "a".to_string(),
            custom_base_url: None,
            custom_model: None,
            reasoning_mode: Default::default(),
            polish_reasoning_mode: None,
            assistant_reasoning_mode: None,
            assistant_use_separate_model: false,
            assistant_model: None,
            custom_providers: vec![
                custom_provider("a"),
                custom_provider("b"),
                custom_provider("c"),
            ],
        };

        assert_eq!(config.fallback_provider_after_removal("a"), "c");
    }

    #[test]
    fn split_reasoning_modes_fall_back_to_legacy_mode() {
        let config = LlmProviderConfig {
            active: "openai".to_string(),
            custom_base_url: None,
            custom_model: None,
            reasoning_mode: LlmReasoningMode::Light,
            polish_reasoning_mode: None,
            assistant_reasoning_mode: None,
            assistant_use_separate_model: false,
            assistant_model: None,
            custom_providers: Vec::new(),
        };

        assert_eq!(config.polish_reasoning_mode(), LlmReasoningMode::Light);
        assert_eq!(config.assistant_reasoning_mode(), LlmReasoningMode::Light);
    }
}
