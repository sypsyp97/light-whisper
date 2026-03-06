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
}

/// LLM 后端配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmProviderConfig {
    /// 当前使用的后端: "cerebras", "deepseek", "custom"
    pub active: String,
    /// 自定义 OpenAI 兼容端点 URL
    pub custom_base_url: Option<String>,
    /// 自定义模型名
    pub custom_model: Option<String>,
}

impl Default for LlmProviderConfig {
    fn default() -> Self {
        Self {
            active: "cerebras".to_string(),
            custom_base_url: None,
            custom_model: None,
        }
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
