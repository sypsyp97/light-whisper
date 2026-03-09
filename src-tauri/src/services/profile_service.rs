use crate::state::user_profile::*;
use crate::utils::paths;
use std::collections::hash_map::Entry;

const MAX_CORRECTION_PATTERNS: usize = 500;
const MAX_HOT_WORDS: usize = 300;
const MAX_SEGMENT_CHARS: usize = 12;
const AUTO_PROMOTE_THRESHOLD: u32 = 5;

// ============================================================
// 持久化
// ============================================================

pub fn load_profile() -> UserProfile {
    let path = paths::get_data_dir().join("user_profile.json");
    let mut profile = std::fs::read_to_string(&path)
        .ok()
        .and_then(|data| serde_json::from_str(&data).ok())
        .unwrap_or_else(|| {
            log::info!("用户画像文件不存在或解析失败，使用默认值");
            UserProfile::default()
        });
    migrate_custom_provider(&mut profile);
    let stats = cleanup_profile(&mut profile);
    if stats.removed_hot_words > 0 || stats.removed_corrections > 0 {
        log::info!(
            "加载画像时清理：热词 -{}, 纠错 -{}",
            stats.removed_hot_words,
            stats.removed_corrections
        );
    }
    profile
}

/// 迁移旧版单 custom provider 到 custom_providers 列表
fn migrate_custom_provider(profile: &mut UserProfile) {
    let config = &mut profile.llm_provider;
    if config.active != "custom" || !config.custom_providers.is_empty() {
        return;
    }
    let base_url = config.custom_base_url.clone().unwrap_or_default();
    let model = config.custom_model.clone().unwrap_or_default();
    if base_url.is_empty() && model.is_empty() {
        return;
    }
    let provider = CustomProvider {
        id: "custom_migrated".to_string(),
        name: "自定义兼容".to_string(),
        base_url,
        model,
        api_format: ApiFormat::default(),
    };
    config.custom_providers.push(provider);
    config.active = "custom_migrated".to_string();
    config.custom_base_url = None;
    config.custom_model = None;
    log::info!("已迁移旧版 custom provider 到 custom_providers");
}

pub fn save_profile(profile: &UserProfile) -> Result<(), String> {
    let path = paths::get_data_dir().join("user_profile.json");
    let data = serde_json::to_string_pretty(profile).map_err(|e| format!("序列化失败: {}", e))?;
    std::fs::write(&path, data).map_err(|e| format!("写入失败: {}", e))
}

pub async fn save_profile_async(profile: &UserProfile) -> Result<(), String> {
    let path = paths::get_data_dir().join("user_profile.json");
    let data = serde_json::to_string_pretty(profile).map_err(|e| format!("序列化失败: {}", e))?;
    tokio::fs::write(&path, data)
        .await
        .map_err(|e| format!("写入失败: {}", e))
}

// ============================================================
// 清理
// ============================================================

#[derive(Debug, Clone, Copy, Default)]
pub struct ProfileCleanupStats {
    pub removed_hot_words: usize,
    pub removed_corrections: usize,
}

pub fn cleanup_profile(profile: &mut UserProfile) -> ProfileCleanupStats {
    let removed_hot_words = sanitize_hot_words(profile);
    let removed_corrections = sanitize_corrections(profile) + limit_correction_patterns(profile);
    if removed_hot_words > 0 || removed_corrections > 0 {
        profile.last_updated = now_secs();
    }
    ProfileCleanupStats {
        removed_hot_words,
        removed_corrections,
    }
}

fn sanitize_corrections(profile: &mut UserProfile) -> usize {
    let before = profile.correction_patterns.len();
    profile.correction_patterns.retain(|p| {
        let too_long =
            p.original.chars().count() > 15 || p.corrected.chars().count() > 15;
        let trivial_ai = p.original.chars().count() <= 1
            && p.corrected.chars().count() <= 1
            && p.source == CorrectionSource::Ai;
        !too_long && !trivial_ai
    });
    before - profile.correction_patterns.len()
}

fn limit_correction_patterns(profile: &mut UserProfile) -> usize {
    if profile.correction_patterns.len() <= MAX_CORRECTION_PATTERNS {
        return 0;
    }
    let before = profile.correction_patterns.len();
    profile
        .correction_patterns
        .sort_by(|a, b| b.count.cmp(&a.count).then(b.last_seen.cmp(&a.last_seen)));
    profile.correction_patterns.truncate(MAX_CORRECTION_PATTERNS);
    before - profile.correction_patterns.len()
}

// ============================================================
// 热词管理
// ============================================================

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
    (!normalized.is_empty()).then(|| {
        let key = normalized.to_lowercase();
        (normalized, key)
    })
}

fn hot_word_priority(w: &HotWord) -> (u8, u8, u32, u64, usize) {
    let src = if w.source == HotWordSource::User { 1 } else { 0 };
    (src, w.weight, w.use_count, w.last_used, w.text.chars().count())
}

fn merge_hot_word(existing: &mut HotWord, candidate: HotWord) {
    if hot_word_priority(&candidate) > hot_word_priority(existing) {
        existing.text = candidate.text;
    }
    existing.weight = existing.weight.max(candidate.weight.clamp(1, 5));
    existing.use_count = existing.use_count.max(candidate.use_count);
    existing.last_used = existing.last_used.max(candidate.last_used);
    if candidate.source == HotWordSource::User {
        existing.source = HotWordSource::User;
    }
}

fn sanitize_hot_words(profile: &mut UserProfile) -> usize {
    let before = profile.hot_words.len();
    let mut deduped = std::collections::HashMap::new();

    for mut hw in std::mem::take(&mut profile.hot_words) {
        let Some((text, key)) = normalize_hot_word_key(&hw.text) else { continue };
        hw.text = text;
        hw.weight = hw.weight.clamp(1, 5);
        match deduped.entry(key) {
            Entry::Vacant(slot) => { slot.insert(hw); }
            Entry::Occupied(mut slot) => merge_hot_word(slot.get_mut(), hw),
        }
    }

    profile.hot_words = deduped.into_values().collect();
    profile
        .hot_words
        .sort_by(|a, b| b.weight.cmp(&a.weight).then(b.use_count.cmp(&a.use_count)));
    profile.hot_words.truncate(MAX_HOT_WORDS);
    before.saturating_sub(profile.hot_words.len())
}

pub fn add_hot_word(profile: &mut UserProfile, text: String, weight: u8) {
    let Some((normalized_text, normalized_key)) = normalize_hot_word_key(&text) else { return };
    let now = now_secs();

    if let Some(existing) = profile.hot_words.iter_mut().find(|h| {
        normalize_hot_word_key(&h.text)
            .map(|(_, k)| k == normalized_key)
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

pub fn remove_hot_word(profile: &mut UserProfile, text: &str) {
    if let Some((_, key)) = normalize_hot_word_key(text) {
        profile.hot_words.retain(|h| {
            normalize_hot_word_key(&h.text)
                .map(|(_, k)| k != key)
                .unwrap_or(false)
        });
    } else {
        profile.hot_words.retain(|h| h.text != text);
    }
    sanitize_hot_words(profile);
    profile.last_updated = now_secs();
}

// ============================================================
// 学习
// ============================================================

/// 纠错模式 upsert：已有则递增，否则插入新条目
fn upsert_correction(
    patterns: &mut Vec<CorrectionPattern>,
    orig: &str,
    corrected: &str,
    initial_count: u32,
    source: &CorrectionSource,
    now: u64,
) {
    if orig.is_empty()
        || corrected.is_empty()
        || orig == corrected
        || orig.chars().count() > MAX_SEGMENT_CHARS
        || corrected.chars().count() > MAX_SEGMENT_CHARS
    {
        return;
    }
    if let Some(p) = patterns
        .iter_mut()
        .find(|p| p.original == orig && p.corrected == corrected)
    {
        p.count += 1;
        p.last_seen = now;
        if *source == CorrectionSource::User {
            p.source = CorrectionSource::User;
        }
    } else {
        patterns.push(CorrectionPattern {
            original: orig.to_string(),
            corrected: corrected.to_string(),
            count: initial_count,
            last_seen: now,
            source: source.clone(),
        });
    }
}

/// 更新词频统计
fn update_vocab_frequency(
    vocab: &mut std::collections::HashMap<String, VocabEntry>,
    words: impl Iterator<Item = String>,
    now: u64,
) {
    for word in words {
        if word.chars().count() < 2 || !is_potential_hot_word(&word) {
            continue;
        }
        let entry = vocab.entry(word).or_insert(VocabEntry { count: 0, last_seen: 0 });
        entry.count += 1;
        entry.last_seen = now;
    }
}

/// 将高频词汇自动提升为热词
fn promote_vocab_to_hot_words(profile: &mut UserProfile, threshold: u32) {
    let existing: std::collections::HashSet<&str> =
        profile.hot_words.iter().map(|h| h.text.as_str()).collect();

    let new: Vec<HotWord> = profile
        .vocab_frequency
        .iter()
        .filter(|(w, e)| {
            e.count >= threshold
                && !existing.contains(w.as_str())
                && w.chars().count() >= 2
                && is_potential_hot_word(w)
        })
        .map(|(w, e)| HotWord {
            text: w.clone(),
            weight: 2,
            source: HotWordSource::Learned,
            use_count: e.count,
            last_used: e.last_seen,
        })
        .collect();

    profile.hot_words.extend(new);
}

/// 学习的公共收尾：限制数量、去重
fn finalize_learning(profile: &mut UserProfile) {
    limit_correction_patterns(profile);
    sanitize_hot_words(profile);
}

/// 从 ASR 原始文本与纠正后文本的字符 diff 中学习
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
    let initial_count = if source == CorrectionSource::User { 3 } else { 1 };
    profile.total_transcriptions += 1;
    profile.last_updated = now;

    for (orig_seg, pol_seg) in extract_diff_segments(original, polished) {
        upsert_correction(
            &mut profile.correction_patterns,
            &orig_seg,
            &pol_seg,
            initial_count,
            &source,
            now,
        );
    }

    update_vocab_frequency(
        &mut profile.vocab_frequency,
        simple_segment(polished).into_iter(),
        now,
    );
    promote_vocab_to_hot_words(profile, AUTO_PROMOTE_THRESHOLD);
    finalize_learning(profile);
}

/// 从 LLM 结构化输出中学习
pub fn learn_from_structured(
    profile: &mut UserProfile,
    corrections: &[(String, String)],
    key_terms: &[String],
    source: CorrectionSource,
) {
    let now = now_secs();
    let initial_count = if source == CorrectionSource::User { 3 } else { 1 };
    profile.total_transcriptions += 1;
    profile.last_updated = now;

    for (orig, corrected) in corrections {
        upsert_correction(
            &mut profile.correction_patterns,
            orig,
            corrected,
            initial_count,
            &source,
            now,
        );
    }

    update_vocab_frequency(
        &mut profile.vocab_frequency,
        key_terms.iter().cloned(),
        now,
    );
    promote_vocab_to_hot_words(profile, 3);
    finalize_learning(profile);
}

// ============================================================
// 辅助函数
// ============================================================

fn is_potential_hot_word(word: &str) -> bool {
    const STOPWORDS: &[&str] = &[
        "的", "了", "是", "在", "我", "有", "和", "就", "不", "人", "都", "一", "一个", "上", "也",
        "很", "到", "说", "要", "去", "你", "会", "着", "没有", "看", "好", "自己", "这", "他",
        "她", "它", "们", "那", "个", "什么", "怎么", "这个", "那个", "但是", "因为", "所以",
        "如果", "可以", "已经", "还是", "或者", "然后", "其实", "应该", "可能", "比较", "现在",
        "知道", "觉得", "时候", "这样", "那样",
    ];
    !STOPWORDS.contains(&word)
        && word
            .chars()
            .any(|c| c.is_alphanumeric() || ('\u{4e00}'..='\u{9fff}').contains(&c))
}

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

fn extract_diff_segments(original: &str, polished: &str) -> Vec<(String, String)> {
    let orig: Vec<char> = original.chars().collect();
    let pol: Vec<char> = polished.chars().collect();
    let (olen, plen) = (orig.len(), pol.len());
    let mut diffs = Vec::new();
    let (mut i, mut j) = (0, 0);

    while i < olen && j < plen {
        if orig[i] == pol[j] {
            i += 1;
            j += 1;
            continue;
        }

        let max_search = 20;
        let mut found = false;
        let (mut oi, mut oj) = (i + 1, j + 1);

        'outer: for di in 0..max_search.min(olen - i) {
            for dj in 0..max_search.min(plen - j) {
                if (di > 0 || dj > 0) && orig[i + di] == pol[j + dj] {
                    oi = i + di;
                    oj = j + dj;
                    found = true;
                    break 'outer;
                }
            }
        }

        if !found {
            break;
        }
        if oi == i && oj == j {
            i += 1;
            j += 1;
            continue;
        }

        let orig_seg: String = orig[i..oi].iter().collect();
        let pol_seg: String = pol[j..oj].iter().collect();
        if !orig_seg.is_empty() && !pol_seg.is_empty() && orig_seg.len() <= 30 {
            diffs.push((orig_seg, pol_seg));
        }
        i = if oi > i { oi } else { i + 1 };
        j = if oj > j { oj } else { j + 1 };
    }

    diffs
}
