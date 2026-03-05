use crate::state::user_profile::*;
use crate::utils::paths;
use std::collections::hash_map::Entry;

const MAX_CORRECTION_PATTERNS: usize = 500;
const MAX_HOT_WORDS: usize = 300;

/// 加载用户画像（从 JSON 文件）
pub fn load_profile() -> UserProfile {
    let path = paths::get_data_dir().join("user_profile.json");
    let mut profile = match std::fs::read_to_string(&path) {
        Ok(data) => serde_json::from_str(&data).unwrap_or_else(|e| {
            log::warn!("用户画像 JSON 解析失败: {}，使用默认值", e);
            UserProfile::default()
        }),
        Err(_) => {
            log::info!("用户画像文件不存在，使用默认值");
            UserProfile::default()
        }
    };
    let cleanup = cleanup_profile(&mut profile);
    if cleanup.removed_hot_words > 0 || cleanup.removed_corrections > 0 {
        log::info!(
            "加载画像时完成清理：热词移除 {} 条，纠错移除 {} 条",
            cleanup.removed_hot_words,
            cleanup.removed_corrections
        );
    }
    profile
}

/// 清理低质量纠错模式
fn sanitize_corrections(profile: &mut UserProfile) -> usize {
    let before = profile.correction_patterns.len();
    profile.correction_patterns.retain(|p| {
        // 移除过长的整句纠错（非词/短语级别）
        if p.original.chars().count() > 15 || p.corrected.chars().count() > 15 {
            return false;
        }
        // 移除单字→单字的过于泛化纠错（如 "你"→"有"）
        if p.original.chars().count() <= 1
            && p.corrected.chars().count() <= 1
            && p.source == CorrectionSource::Ai
        {
            return false;
        }
        true
    });
    let removed = before - profile.correction_patterns.len();
    if removed > 0 {
        log::info!("清理了 {} 条低质量纠错模式", removed);
    }
    removed
}

/// 用户画像清理统计
#[derive(Debug, Clone, Copy, Default)]
pub struct ProfileCleanupStats {
    pub removed_hot_words: usize,
    pub removed_corrections: usize,
}

/// 清理整个用户画像（热词 + 纠错）
pub fn cleanup_profile(profile: &mut UserProfile) -> ProfileCleanupStats {
    let removed_hot_words = sanitize_hot_words(profile);
    let removed_low_quality = sanitize_corrections(profile);
    let removed_overflow = limit_correction_patterns(profile);
    let removed_corrections = removed_low_quality + removed_overflow;

    if removed_hot_words > 0 || removed_corrections > 0 {
        profile.last_updated = now_secs();
    }

    ProfileCleanupStats {
        removed_hot_words,
        removed_corrections,
    }
}

/// 保存用户画像到 JSON 文件
pub fn save_profile(profile: &UserProfile) -> Result<(), String> {
    let path = paths::get_data_dir().join("user_profile.json");
    let data =
        serde_json::to_string_pretty(profile).map_err(|e| format!("序列化用户画像失败: {}", e))?;
    std::fs::write(&path, data).map_err(|e| format!("写入用户画像文件失败: {}", e))
}

/// 保存用户画像（异步，用于后台保存）
pub async fn save_profile_async(profile: &UserProfile) -> Result<(), String> {
    let path = paths::get_data_dir().join("user_profile.json");
    let data =
        serde_json::to_string_pretty(profile).map_err(|e| format!("序列化用户画像失败: {}", e))?;
    tokio::fs::write(&path, data)
        .await
        .map_err(|e| format!("写入用户画像文件失败: {}", e))
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn normalize_hot_word_text(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn normalize_hot_word_key(text: &str) -> Option<(String, String)> {
    let normalized = normalize_hot_word_text(text);
    if normalized.is_empty() {
        None
    } else {
        let key = normalized.to_lowercase();
        Some((normalized, key))
    }
}

fn hot_word_priority(word: &HotWord) -> (u8, u8, u32, u64, usize) {
    let source = match word.source {
        HotWordSource::User => 1,
        HotWordSource::Learned => 0,
    };
    (
        source,
        word.weight,
        word.use_count,
        word.last_used,
        word.text.chars().count(),
    )
}

fn merge_hot_word(existing: &mut HotWord, candidate: HotWord) {
    if hot_word_priority(&candidate) > hot_word_priority(existing) {
        existing.text = candidate.text.clone();
    }

    existing.weight = existing.weight.max(candidate.weight.clamp(1, 5));
    existing.use_count = existing.use_count.max(candidate.use_count);
    existing.last_used = existing.last_used.max(candidate.last_used);
    if candidate.source == HotWordSource::User {
        existing.source = HotWordSource::User;
    }
}

fn limit_correction_patterns(profile: &mut UserProfile) -> usize {
    let before = profile.correction_patterns.len();
    if profile.correction_patterns.len() > MAX_CORRECTION_PATTERNS {
        profile
            .correction_patterns
            .sort_by(|a, b| b.count.cmp(&a.count).then(b.last_seen.cmp(&a.last_seen)));
        profile
            .correction_patterns
            .truncate(MAX_CORRECTION_PATTERNS);
    }
    before.saturating_sub(profile.correction_patterns.len())
}

fn limit_hot_words(profile: &mut UserProfile) {
    profile
        .hot_words
        .sort_by(|a, b| b.weight.cmp(&a.weight).then(b.use_count.cmp(&a.use_count)));
    if profile.hot_words.len() > MAX_HOT_WORDS {
        profile.hot_words.truncate(MAX_HOT_WORDS);
    }
}

fn sanitize_hot_words(profile: &mut UserProfile) -> usize {
    let before = profile.hot_words.len();
    let mut deduped: std::collections::HashMap<String, HotWord> = std::collections::HashMap::new();

    for mut hot_word in std::mem::take(&mut profile.hot_words) {
        let Some((normalized_text, normalized_key)) = normalize_hot_word_key(&hot_word.text) else {
            continue;
        };

        hot_word.text = normalized_text;
        hot_word.weight = hot_word.weight.clamp(1, 5);

        match deduped.entry(normalized_key) {
            Entry::Vacant(slot) => {
                slot.insert(hot_word);
            }
            Entry::Occupied(mut slot) => {
                merge_hot_word(slot.get_mut(), hot_word);
            }
        }
    }

    profile.hot_words = deduped.into_values().collect();
    limit_hot_words(profile);
    before.saturating_sub(profile.hot_words.len())
}

/// 从纠正结果中学习
///
/// 对比 ASR 原始文本与 LLM 纠正后的文本，提取差异模式，
/// 更新纠错模式记录和词频统计。
pub fn learn_from_correction(
    profile: &mut UserProfile,
    original: &str,
    polished: &str,
    source: CorrectionSource,
) {
    if original == polished || original.is_empty() || polished.is_empty() {
        return;
    }

    let now = now_secs();
    profile.total_transcriptions += 1;
    profile.last_updated = now;

    // 用户纠错初始 count 设为 3，立即获得高优先级
    let initial_count = match source {
        CorrectionSource::User => 3,
        CorrectionSource::Ai => 1,
    };

    // 提取逐字符差异片段
    let diff_pairs = extract_diff_segments(original, polished);
    for (orig_seg, pol_seg) in &diff_pairs {
        if orig_seg.is_empty() || pol_seg.is_empty() {
            continue;
        }
        // 跳过过长的差异片段（非词/短语级别的纠错）
        if orig_seg.chars().count() > 12 || pol_seg.chars().count() > 12 {
            continue;
        }
        // 查找是否已有此纠错模式
        if let Some(pattern) = profile
            .correction_patterns
            .iter_mut()
            .find(|p| p.original == *orig_seg && p.corrected == *pol_seg)
        {
            pattern.count += 1;
            pattern.last_seen = now;
            // 用户来源可以升级，但不降级
            if source == CorrectionSource::User {
                pattern.source = CorrectionSource::User;
            }
        } else {
            profile.correction_patterns.push(CorrectionPattern {
                original: orig_seg.clone(),
                corrected: pol_seg.clone(),
                count: initial_count,
                last_seen: now,
                source: source.clone(),
            });
        }
    }

    // 更新纠正后文本的词频（简单按标点/空格分词）
    let words = simple_segment(polished);
    for word in &words {
        if word.len() < 2 {
            continue; // 跳过单字
        }
        let entry = profile
            .vocab_frequency
            .entry(word.clone())
            .or_insert(VocabEntry {
                count: 0,
                last_seen: 0,
            });
        entry.count += 1;
        entry.last_seen = now;
    }

    // 自动提升高频词汇为热词（阈值: 5 次使用，且不在现有热词列表中）
    const AUTO_PROMOTE_THRESHOLD: u32 = 5;
    let existing_hot: std::collections::HashSet<&str> =
        profile.hot_words.iter().map(|h| h.text.as_str()).collect();

    let new_hot_words: Vec<HotWord> = profile
        .vocab_frequency
        .iter()
        .filter(|(word, entry)| {
            entry.count >= AUTO_PROMOTE_THRESHOLD
                && !existing_hot.contains(word.as_str())
                && word.chars().count() >= 2
                && is_potential_hot_word(word)
        })
        .map(|(word, entry)| HotWord {
            text: word.clone(),
            weight: 2,
            source: HotWordSource::Learned,
            use_count: entry.count,
            last_used: entry.last_seen,
        })
        .collect();

    profile.hot_words.extend(new_hot_words);

    // 限制数量并清理重复热词
    let _ = limit_correction_patterns(profile);
    sanitize_hot_words(profile);
}

/// 从 LLM 结构化输出中学习
///
/// 直接使用 LLM 提取的纠错对和术语，无需字符 diff。
pub fn learn_from_structured(
    profile: &mut UserProfile,
    corrections: &[(String, String)],
    key_terms: &[String],
    source: CorrectionSource,
) {
    let now = now_secs();
    profile.total_transcriptions += 1;
    profile.last_updated = now;

    let initial_count = match source {
        CorrectionSource::User => 3,
        CorrectionSource::Ai => 1,
    };

    // 写入纠错模式（跳过过长的整句纠错，只保留词/短语级别）
    for (orig, corrected) in corrections {
        if orig.is_empty() || corrected.is_empty() || orig == corrected {
            continue;
        }
        if orig.chars().count() > 12 || corrected.chars().count() > 12 {
            log::debug!("跳过过长纠错模式: \"{}\" → \"{}\"", orig, corrected);
            continue;
        }
        if let Some(pattern) = profile
            .correction_patterns
            .iter_mut()
            .find(|p| p.original == *orig && p.corrected == *corrected)
        {
            pattern.count += 1;
            pattern.last_seen = now;
            if source == CorrectionSource::User {
                pattern.source = CorrectionSource::User;
            }
        } else {
            profile.correction_patterns.push(CorrectionPattern {
                original: orig.clone(),
                corrected: corrected.clone(),
                count: initial_count,
                last_seen: now,
                source: source.clone(),
            });
        }
    }

    // 写入 LLM 提取的术语到词频
    for term in key_terms {
        if term.chars().count() < 2 || !is_potential_hot_word(term) {
            continue;
        }
        let entry = profile
            .vocab_frequency
            .entry(term.clone())
            .or_insert(VocabEntry {
                count: 0,
                last_seen: 0,
            });
        entry.count += 1;
        entry.last_seen = now;
    }

    // 自动提升高频术语为热词
    let existing_hot: std::collections::HashSet<&str> =
        profile.hot_words.iter().map(|h| h.text.as_str()).collect();
    let new_hot_words: Vec<HotWord> = profile
        .vocab_frequency
        .iter()
        .filter(|(word, entry)| {
            entry.count >= 3
                && !existing_hot.contains(word.as_str())
                && word.chars().count() >= 2
                && is_potential_hot_word(word)
        })
        .map(|(word, entry)| HotWord {
            text: word.clone(),
            weight: 2,
            source: HotWordSource::Learned,
            use_count: entry.count,
            last_used: entry.last_seen,
        })
        .collect();
    profile.hot_words.extend(new_hot_words);

    // 限制数量并清理重复热词
    let _ = limit_correction_patterns(profile);
    sanitize_hot_words(profile);
}

/// 判断一个词是否适合作为热词
/// 排除常见的中文停用词和纯标点
fn is_potential_hot_word(word: &str) -> bool {
    let stopwords = [
        "的", "了", "是", "在", "我", "有", "和", "就", "不", "人", "都", "一", "一个", "上", "也",
        "很", "到", "说", "要", "去", "你", "会", "着", "没有", "看", "好", "自己", "这", "他",
        "她", "它", "们", "那", "个", "什么", "怎么", "这个", "那个", "但是", "因为", "所以",
        "如果", "可以", "已经", "还是", "或者", "然后", "其实", "应该", "可能", "比较", "现在",
        "知道", "觉得", "时候", "这样", "那样",
    ];
    if stopwords.contains(&word) {
        return false;
    }
    // 至少包含一个非标点字符
    word.chars()
        .any(|c| c.is_alphanumeric() || ('\u{4e00}'..='\u{9fff}').contains(&c))
}

/// 简单分词：按标点和空格切分
fn simple_segment(text: &str) -> Vec<String> {
    let mut segments = Vec::new();
    let mut current = String::new();

    for ch in text.chars() {
        if ch.is_alphanumeric() || ('\u{4e00}'..='\u{9fff}').contains(&ch) {
            current.push(ch);
        } else if !current.is_empty() {
            segments.push(std::mem::take(&mut current));
        }
    }
    if !current.is_empty() {
        segments.push(current);
    }
    segments
}

/// 提取两段文本之间的差异片段对
///
/// 使用简单的最长公共子序列（LCS）思路来找出差异。
/// 返回 (原始片段, 纠正后片段) 的列表。
fn extract_diff_segments(original: &str, polished: &str) -> Vec<(String, String)> {
    let orig_chars: Vec<char> = original.chars().collect();
    let pol_chars: Vec<char> = polished.chars().collect();
    let mut diffs = Vec::new();

    // 简化实现：找出连续不同的片段
    let mut i = 0;
    let mut j = 0;
    let olen = orig_chars.len();
    let plen = pol_chars.len();

    while i < olen && j < plen {
        if orig_chars[i] == pol_chars[j] {
            i += 1;
            j += 1;
        } else {
            // 找到差异起始点，尝试找到重新同步的位置
            let mut orig_end = i + 1;
            let mut pol_end = j + 1;

            // 向前搜索同步点（限制搜索范围为 20 字符）
            let max_search = 20;
            let mut found = false;

            'outer: for di in 0..max_search.min(olen - i) {
                for dj in 0..max_search.min(plen - j) {
                    if i + di < olen
                        && j + dj < plen
                        && orig_chars[i + di] == pol_chars[j + dj]
                        && (di > 0 || dj > 0)
                    {
                        orig_end = i + di;
                        pol_end = j + dj;
                        found = true;
                        break 'outer;
                    }
                }
            }

            if found {
                // 确保至少一个方向有推进，否则跳过避免死循环
                if orig_end == i && pol_end == j {
                    i += 1;
                    j += 1;
                } else {
                    let orig_seg: String = orig_chars[i..orig_end].iter().collect();
                    let pol_seg: String = pol_chars[j..pol_end].iter().collect();
                    if !orig_seg.is_empty() && !pol_seg.is_empty() && orig_seg.len() <= 30 {
                        diffs.push((orig_seg, pol_seg));
                    }
                    // 至少推进 1 步防止卡住
                    i = if orig_end > i { orig_end } else { i + 1 };
                    j = if pol_end > j { pol_end } else { j + 1 };
                }
            } else {
                // 无法同步，跳过
                break;
            }
        }
    }

    diffs
}

/// 添加用户手动热词
pub fn add_hot_word(profile: &mut UserProfile, text: String, weight: u8) {
    let Some((normalized_text, normalized_key)) = normalize_hot_word_key(&text) else {
        return;
    };
    let now = now_secs();

    // 检查是否已存在（大小写不敏感，忽略多余空白）
    if let Some(existing) = profile.hot_words.iter_mut().find(|h| {
        normalize_hot_word_key(&h.text)
            .map(|(_, key)| key == normalized_key)
            .unwrap_or(false)
    }) {
        existing.text = normalized_text;
        existing.weight = weight.clamp(1, 5);
        existing.source = HotWordSource::User;
        existing.last_used = now;
    } else {
        profile.hot_words.push(HotWord {
            text: normalized_text,
            weight: weight.clamp(1, 5),
            source: HotWordSource::User,
            use_count: 0,
            last_used: now,
        });
    }
    sanitize_hot_words(profile);
    profile.last_updated = now;
}

/// 删除热词
pub fn remove_hot_word(profile: &mut UserProfile, text: &str) {
    let before = profile.hot_words.len();

    if let Some((_, normalized_key)) = normalize_hot_word_key(text) {
        profile.hot_words.retain(|h| {
            normalize_hot_word_key(&h.text)
                .map(|(_, key)| key != normalized_key)
                .unwrap_or(false)
        });
    } else {
        profile.hot_words.retain(|h| h.text != text);
    }

    let removed_by_delete = before.saturating_sub(profile.hot_words.len());
    let removed_by_cleanup = sanitize_hot_words(profile);
    if removed_by_delete > 0 || removed_by_cleanup > 0 {
        profile.last_updated = now_secs();
    }
}
