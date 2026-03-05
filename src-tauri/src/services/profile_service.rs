use crate::state::user_profile::*;
use crate::utils::paths;

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
    sanitize_corrections(&mut profile);
    profile
}

/// 清理低质量纠错模式
fn sanitize_corrections(profile: &mut UserProfile) {
    let before = profile.correction_patterns.len();
    profile.correction_patterns.retain(|p| {
        // 移除过长的整句纠错（非词/短语级别）
        if p.original.chars().count() > 15 || p.corrected.chars().count() > 15 {
            return false;
        }
        // 移除单字→单字的过于泛化纠错（如 "你"→"有"）
        if p.original.chars().count() <= 1 && p.corrected.chars().count() <= 1 && p.source == CorrectionSource::Ai {
            return false;
        }
        true
    });
    let removed = before - profile.correction_patterns.len();
    if removed > 0 {
        log::info!("清理了 {} 条低质量纠错模式", removed);
    }
}

/// 保存用户画像到 JSON 文件
pub fn save_profile(profile: &UserProfile) -> Result<(), String> {
    let path = paths::get_data_dir().join("user_profile.json");
    let data = serde_json::to_string_pretty(profile)
        .map_err(|e| format!("序列化用户画像失败: {}", e))?;
    std::fs::write(&path, data).map_err(|e| format!("写入用户画像文件失败: {}", e))
}

/// 保存用户画像（异步，用于后台保存）
pub async fn save_profile_async(profile: &UserProfile) -> Result<(), String> {
    let path = paths::get_data_dir().join("user_profile.json");
    let data = serde_json::to_string_pretty(profile)
        .map_err(|e| format!("序列化用户画像失败: {}", e))?;
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

    // 限制纠错模式数量
    if profile.correction_patterns.len() > 500 {
        profile
            .correction_patterns
            .sort_by(|a, b| b.count.cmp(&a.count).then(b.last_seen.cmp(&a.last_seen)));
        profile.correction_patterns.truncate(500);
    }

    // 限制热词数量
    if profile.hot_words.len() > 300 {
        profile
            .hot_words
            .sort_by(|a, b| b.weight.cmp(&a.weight).then(b.use_count.cmp(&a.use_count)));
        profile.hot_words.truncate(300);
    }
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

    // 限制纠错模式数量
    if profile.correction_patterns.len() > 500 {
        profile
            .correction_patterns
            .sort_by(|a, b| b.count.cmp(&a.count).then(b.last_seen.cmp(&a.last_seen)));
        profile.correction_patterns.truncate(500);
    }

    // 限制热词数量
    if profile.hot_words.len() > 300 {
        profile
            .hot_words
            .sort_by(|a, b| b.weight.cmp(&a.weight).then(b.use_count.cmp(&a.use_count)));
        profile.hot_words.truncate(300);
    }
}

/// 判断一个词是否适合作为热词
/// 排除常见的中文停用词和纯标点
fn is_potential_hot_word(word: &str) -> bool {
    let stopwords = [
        "的", "了", "是", "在", "我", "有", "和", "就", "不", "人", "都", "一", "一个", "上",
        "也", "很", "到", "说", "要", "去", "你", "会", "着", "没有", "看", "好", "自己", "这",
        "他", "她", "它", "们", "那", "个", "什么", "怎么", "这个", "那个", "但是", "因为",
        "所以", "如果", "可以", "已经", "还是", "或者", "然后", "其实", "应该", "可能",
        "比较", "现在", "知道", "觉得", "时候", "这样", "那样",
    ];
    if stopwords.contains(&word) {
        return false;
    }
    // 至少包含一个非标点字符
    word.chars()
        .any(|c| c.is_alphanumeric() || c >= '\u{4e00}' && c <= '\u{9fff}')
}

/// 简单分词：按标点和空格切分
fn simple_segment(text: &str) -> Vec<String> {
    let mut segments = Vec::new();
    let mut current = String::new();

    for ch in text.chars() {
        if ch.is_alphanumeric() || (ch >= '\u{4e00}' && ch <= '\u{9fff}') {
            current.push(ch);
        } else {
            if !current.is_empty() {
                segments.push(std::mem::take(&mut current));
            }
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
    let now = now_secs();
    // 检查是否已存在
    if let Some(existing) = profile.hot_words.iter_mut().find(|h| h.text == text) {
        existing.weight = weight.clamp(1, 5);
        existing.source = HotWordSource::User;
        existing.last_used = now;
    } else {
        profile.hot_words.push(HotWord {
            text,
            weight: weight.clamp(1, 5),
            source: HotWordSource::User,
            use_count: 0,
            last_used: now,
        });
    }
    profile.last_updated = now;
}

/// 删除热词
pub fn remove_hot_word(profile: &mut UserProfile, text: &str) {
    profile.hot_words.retain(|h| h.text != text);
    profile.last_updated = now_secs();
}

