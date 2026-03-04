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
}

/// 词频条目
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VocabEntry {
    pub count: u32,
    pub last_seen: u64,
}

/// 用户画像
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserProfile {
    pub hot_words: Vec<HotWord>,
    pub correction_patterns: Vec<CorrectionPattern>,
    pub vocab_frequency: HashMap<String, VocabEntry>,
    pub total_transcriptions: u64,
    /// 上次更新时间（Unix 时间戳）
    pub last_updated: u64,
    /// LLM 后端配置
    pub llm_provider: LlmProviderConfig,
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

impl Default for UserProfile {
    fn default() -> Self {
        Self {
            hot_words: Vec::new(),
            correction_patterns: Vec::new(),
            vocab_frequency: HashMap::new(),
            total_transcriptions: 0,
            last_updated: 0,
            llm_provider: LlmProviderConfig::default(),
        }
    }
}

impl UserProfile {
    /// 获取按权重排序的热词文本列表（用于 ASR 注入）
    pub fn get_hot_word_texts(&self, limit: usize) -> Vec<String> {
        let mut words: Vec<&HotWord> = self.hot_words.iter().collect();
        words.sort_by(|a, b| b.weight.cmp(&a.weight).then(b.use_count.cmp(&a.use_count)));
        words.into_iter().take(limit).map(|w| w.text.clone()).collect()
    }

    /// 获取 Top N 纠错模式（用于 LLM prompt 注入）
    pub fn get_top_corrections(&self, limit: usize) -> Vec<&CorrectionPattern> {
        let mut patterns: Vec<&CorrectionPattern> = self.correction_patterns.iter().collect();
        patterns.sort_by(|a, b| b.count.cmp(&a.count));
        patterns.into_iter().take(limit).collect()
    }
}
