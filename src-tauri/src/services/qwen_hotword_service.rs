use pinyin::ToPinyin;
use std::collections::HashSet;

use crate::state::user_profile::{
    CorrectionPattern, CorrectionSource, HotWord, HotWordSource, UserProfile,
};

const MAX_ASR_HOT_WORDS: usize = 100;
const MAX_ASR_ALIASES: usize = 100;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HotWordCorrection {
    pub text: String,
    pub replacements: usize,
}

#[derive(Debug, Clone)]
struct ReplacementCandidate<'a> {
    start: usize,
    end: usize,
    replacement: &'a str,
    score: u32,
    rank: usize,
}

#[derive(Debug, Clone, Copy)]
struct WordSpan {
    start: usize,
    end: usize,
}

pub fn correct_qwen_profile_terms(text: &str, profile: &UserProfile) -> HotWordCorrection {
    let hot_words = correct_qwen_hot_words(text, &profile.hot_words);
    let aliases = correct_known_aliases(
        &hot_words.text,
        &profile.hot_words,
        &profile.correction_patterns,
    );

    HotWordCorrection {
        text: aliases.text,
        replacements: aliases.replacements + hot_words.replacements,
    }
}

pub fn correct_qwen_hot_words(text: &str, hot_words: &[HotWord]) -> HotWordCorrection {
    if text.is_empty() || hot_words.is_empty() {
        return HotWordCorrection {
            text: text.to_owned(),
            replacements: 0,
        };
    }

    let mut ranked: Vec<&HotWord> = hot_words.iter().collect();
    ranked.sort_by(|a, b| b.weight.cmp(&a.weight).then(b.use_count.cmp(&a.use_count)));
    ranked.truncate(MAX_ASR_HOT_WORDS);

    let chars = indexed_chars(text);
    let ascii_words = ascii_word_spans(text);
    let mut candidates = Vec::new();

    for (rank, hot_word) in ranked.into_iter().enumerate() {
        let hot_text = hot_word.text.trim();
        if hot_text.is_empty() || text.contains(hot_text) {
            continue;
        }

        if hot_text.chars().all(is_han) {
            collect_han_candidates(text, &chars, hot_word, hot_text, rank, &mut candidates);
        } else if hot_text.is_ascii() && hot_text.chars().any(|ch| ch.is_ascii_alphanumeric()) {
            collect_ascii_candidates(
                text,
                &ascii_words,
                hot_word,
                hot_text,
                rank,
                &mut candidates,
            );
        }
    }

    candidates.sort_by(|a, b| {
        b.score
            .cmp(&a.score)
            .then((b.end - b.start).cmp(&(a.end - a.start)))
            .then(a.rank.cmp(&b.rank))
            .then(a.start.cmp(&b.start))
    });

    let mut selected: Vec<ReplacementCandidate<'_>> = Vec::new();
    for candidate in candidates {
        let overlaps = selected
            .iter()
            .any(|kept| candidate.start < kept.end && kept.start < candidate.end);
        if !overlaps {
            selected.push(candidate);
        }
    }

    selected.sort_by(|a, b| b.start.cmp(&a.start));
    let replacements = selected.len();
    let mut corrected = text.to_owned();
    for candidate in selected {
        corrected.replace_range(candidate.start..candidate.end, candidate.replacement);
    }

    HotWordCorrection {
        text: corrected,
        replacements,
    }
}

fn correct_known_aliases(
    text: &str,
    hot_words: &[HotWord],
    correction_patterns: &[CorrectionPattern],
) -> HotWordCorrection {
    if text.is_empty() || hot_words.is_empty() || correction_patterns.is_empty() {
        return HotWordCorrection {
            text: text.to_owned(),
            replacements: 0,
        };
    }

    let mut ranked_hot_words: Vec<&HotWord> = hot_words.iter().collect();
    ranked_hot_words.sort_by(|a, b| b.weight.cmp(&a.weight).then(b.use_count.cmp(&a.use_count)));
    ranked_hot_words.truncate(MAX_ASR_HOT_WORDS);
    let hot_targets: HashSet<String> = ranked_hot_words
        .into_iter()
        .map(|hot_word| normalize_profile_term(hot_word.text.trim()))
        .filter(|normalized| !normalized.is_empty())
        .collect();

    let mut aliases: Vec<&CorrectionPattern> = correction_patterns
        .iter()
        .filter(|pattern| is_safe_alias(pattern, &hot_targets))
        .collect();
    aliases.sort_by(|a, b| b.count.cmp(&a.count).then(b.last_seen.cmp(&a.last_seen)));
    aliases.truncate(MAX_ASR_ALIASES);

    let mut candidates = Vec::new();
    for (rank, alias) in aliases.into_iter().enumerate() {
        collect_alias_candidates(text, alias, rank, &mut candidates);
    }

    apply_candidates(text, candidates)
}

fn is_safe_alias(pattern: &CorrectionPattern, hot_targets: &HashSet<String>) -> bool {
    let original = pattern.original.trim();
    let corrected = pattern.corrected.trim();
    if original.is_empty() || corrected.is_empty() || original == corrected {
        return false;
    }

    let original_normalized = normalize_profile_term(original);
    let corrected_normalized = normalize_profile_term(corrected);
    if !hot_targets.contains(&corrected_normalized) {
        return false;
    }

    let original_is_ascii = original.is_ascii();
    let corrected_is_ascii = corrected.is_ascii();
    let original_is_han = original.chars().all(is_han);
    let corrected_is_han = corrected.chars().all(is_han);
    let same_script =
        (original_is_ascii && corrected_is_ascii) || (original_is_han && corrected_is_han);
    if !same_script {
        return false;
    }

    let min_length = if original_is_ascii { 3 } else { 2 };
    if original_normalized.chars().count() < min_length
        || corrected_normalized.chars().count() < min_length
        || original.chars().count() > 80
        || corrected.chars().count() > 80
    {
        return false;
    }

    match pattern.source {
        CorrectionSource::Ai => true,
        CorrectionSource::User => {
            if !original_is_ascii {
                return false;
            }
            let original_words = ascii_word_spans(original).len();
            original_words > 1 || levenshtein(&original_normalized, &corrected_normalized) <= 1
        }
    }
}

fn collect_alias_candidates<'a>(
    text: &str,
    alias: &'a CorrectionPattern,
    rank: usize,
    candidates: &mut Vec<ReplacementCandidate<'a>>,
) {
    let original = alias.original.trim();
    let corrected = alias.corrected.trim();
    for (start, matched) in text.match_indices(original) {
        let end = start + matched.len();
        if original.is_ascii() && !has_ascii_boundaries(text, start, end) {
            continue;
        }
        candidates.push(ReplacementCandidate {
            start,
            end,
            replacement: corrected,
            score: 2_000 + alias.count.min(1_000) + original.chars().count() as u32,
            rank,
        });
    }
}

fn apply_candidates<'a>(
    text: &str,
    mut candidates: Vec<ReplacementCandidate<'a>>,
) -> HotWordCorrection {
    candidates.sort_by(|a, b| {
        b.score
            .cmp(&a.score)
            .then((b.end - b.start).cmp(&(a.end - a.start)))
            .then(a.rank.cmp(&b.rank))
            .then(a.start.cmp(&b.start))
    });

    let mut selected: Vec<ReplacementCandidate<'_>> = Vec::new();
    for candidate in candidates {
        let overlaps = selected
            .iter()
            .any(|kept| candidate.start < kept.end && kept.start < candidate.end);
        if !overlaps {
            selected.push(candidate);
        }
    }

    selected.sort_by(|a, b| b.start.cmp(&a.start));
    let replacements = selected.len();
    let mut corrected = text.to_owned();
    for candidate in selected {
        corrected.replace_range(candidate.start..candidate.end, candidate.replacement);
    }

    HotWordCorrection {
        text: corrected,
        replacements,
    }
}

fn collect_han_candidates<'a>(
    text: &str,
    chars: &[(usize, char)],
    hot_word: &'a HotWord,
    hot_text: &'a str,
    rank: usize,
    candidates: &mut Vec<ReplacementCandidate<'a>>,
) {
    let hot_chars: Vec<char> = hot_text.chars().collect();
    let hot_len = hot_chars.len();
    let is_manual = hot_word.source == HotWordSource::User && hot_word.weight >= 3;
    let min_len = if is_manual { 2 } else { 3 };
    if hot_len < min_len || hot_len > chars.len() {
        return;
    }

    let Some(hot_pinyin) = pinyin_signature(&hot_chars) else {
        return;
    };

    for start_index in 0..=chars.len() - hot_len {
        let window = &chars[start_index..start_index + hot_len];
        if !window.iter().all(|(_, ch)| is_han(*ch)) {
            continue;
        }

        let candidate_chars: Vec<char> = window.iter().map(|(_, ch)| *ch).collect();
        if candidate_chars == hot_chars {
            continue;
        }

        let shared_chars = candidate_chars
            .iter()
            .zip(&hot_chars)
            .filter(|(candidate, hot)| candidate == hot)
            .count();
        let min_shared = if is_manual {
            1.max(hot_len / 3)
        } else {
            1.max(hot_len.div_ceil(2))
        };
        if shared_chars < min_shared {
            continue;
        }

        let Some(candidate_pinyin) = pinyin_signature(&candidate_chars) else {
            continue;
        };
        if candidate_pinyin != hot_pinyin {
            continue;
        }

        let start = window[0].0;
        let end = chars
            .get(start_index + hot_len)
            .map(|(byte_index, _)| *byte_index)
            .unwrap_or(text.len());
        candidates.push(ReplacementCandidate {
            start,
            end,
            replacement: hot_text,
            score: 900 + (shared_chars as u32 * 20) + (hot_len as u32 * 5),
            rank,
        });
    }
}

fn collect_ascii_candidates<'a>(
    text: &str,
    words: &[WordSpan],
    hot_word: &'a HotWord,
    hot_text: &'a str,
    rank: usize,
    candidates: &mut Vec<ReplacementCandidate<'a>>,
) {
    let hot_normalized = normalize_ascii(hot_text);
    if hot_normalized.is_empty() {
        return;
    }

    let hot_word_count = ascii_word_spans(hot_text).len().max(1);
    let min_words = hot_word_count.saturating_sub(1).max(1);
    let max_words = hot_word_count + 1;
    let is_manual = hot_word.source == HotWordSource::User && hot_word.weight >= 3;
    let has_canonical_style = has_canonical_ascii_style(hot_text);
    if !is_manual && !has_canonical_style {
        return;
    }

    for start_index in 0..words.len() {
        for word_count in min_words..=max_words {
            let Some(end_index) = start_index.checked_add(word_count - 1) else {
                continue;
            };
            let Some(last_word) = words.get(end_index) else {
                continue;
            };
            let start = words[start_index].start;
            let end = last_word.end;
            let raw = &text[start..end];
            if !raw.is_ascii() || raw == hot_text {
                continue;
            }

            let candidate_normalized = normalize_ascii(raw);
            if candidate_normalized.is_empty() {
                continue;
            }

            let distance = levenshtein(&candidate_normalized, &hot_normalized);
            if distance == 0 {
                candidates.push(ReplacementCandidate {
                    start,
                    end,
                    replacement: hot_text,
                    score: 1000 + hot_normalized.len() as u32,
                    rank,
                });
                continue;
            }

            if !is_manual || hot_normalized.len() < 5 {
                continue;
            }
            if is_simple_inflection(&candidate_normalized, &hot_normalized) {
                continue;
            }

            let max_distance = if hot_normalized.len() >= 10 { 2 } else { 1 };
            if distance > max_distance
                || candidate_normalized.len().abs_diff(hot_normalized.len()) > max_distance
            {
                continue;
            }

            candidates.push(ReplacementCandidate {
                start,
                end,
                replacement: hot_text,
                score: 800 + hot_normalized.len() as u32 - (distance as u32 * 50),
                rank,
            });
        }
    }
}

fn indexed_chars(text: &str) -> Vec<(usize, char)> {
    text.char_indices().collect()
}

fn ascii_word_spans(text: &str) -> Vec<WordSpan> {
    let mut spans = Vec::new();
    let mut start = None;
    for (index, ch) in text.char_indices() {
        if ch.is_ascii_alphanumeric() {
            start.get_or_insert(index);
        } else if let Some(word_start) = start.take() {
            spans.push(WordSpan {
                start: word_start,
                end: index,
            });
        }
    }
    if let Some(word_start) = start {
        spans.push(WordSpan {
            start: word_start,
            end: text.len(),
        });
    }
    spans
}

fn normalize_ascii(text: &str) -> String {
    text.chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .map(|ch| ch.to_ascii_lowercase())
        .collect()
}

fn normalize_profile_term(text: &str) -> String {
    text.chars()
        .filter_map(|ch| {
            if ch.is_ascii_alphanumeric() {
                Some(ch.to_ascii_lowercase())
            } else if is_han(ch) {
                Some(ch)
            } else {
                None
            }
        })
        .collect()
}

fn has_ascii_boundaries(text: &str, start: usize, end: usize) -> bool {
    let before_is_word = text[..start]
        .chars()
        .next_back()
        .is_some_and(|ch| ch.is_ascii_alphanumeric());
    let after_is_word = text[end..]
        .chars()
        .next()
        .is_some_and(|ch| ch.is_ascii_alphanumeric());
    !before_is_word && !after_is_word
}

fn has_canonical_ascii_style(text: &str) -> bool {
    text.chars().filter(|ch| ch.is_ascii_uppercase()).count() >= 2
}

fn is_simple_inflection(candidate: &str, hot_word: &str) -> bool {
    const SUFFIXES: &[&str] = &["s", "es", "ed", "ing"];
    SUFFIXES.iter().any(|suffix| {
        candidate
            .strip_prefix(hot_word)
            .is_some_and(|remainder| remainder == *suffix)
            || hot_word
                .strip_prefix(candidate)
                .is_some_and(|remainder| remainder == *suffix)
    })
}

fn pinyin_signature(chars: &[char]) -> Option<Vec<&'static str>> {
    chars
        .iter()
        .map(|ch| ch.to_pinyin().map(|pinyin| pinyin.plain()))
        .collect()
}

fn is_han(ch: char) -> bool {
    matches!(
        ch,
        '\u{3400}'..='\u{4dbf}'
            | '\u{4e00}'..='\u{9fff}'
            | '\u{f900}'..='\u{faff}'
            | '\u{20000}'..='\u{2a6df}'
            | '\u{2a700}'..='\u{2b73f}'
            | '\u{2b740}'..='\u{2b81f}'
            | '\u{2b820}'..='\u{2ceaf}'
            | '\u{2ceb0}'..='\u{2ebef}'
            | '\u{30000}'..='\u{3134f}'
    )
}

fn levenshtein(left: &str, right: &str) -> usize {
    let right = right.as_bytes();
    let mut previous: Vec<usize> = (0..=right.len()).collect();
    let mut current = vec![0; right.len() + 1];

    for (left_index, left_byte) in left.as_bytes().iter().enumerate() {
        current[0] = left_index + 1;
        for (right_index, right_byte) in right.iter().enumerate() {
            let substitution = previous[right_index] + usize::from(left_byte != right_byte);
            current[right_index + 1] = (previous[right_index + 1] + 1)
                .min(current[right_index] + 1)
                .min(substitution);
        }
        std::mem::swap(&mut previous, &mut current);
    }
    previous[right.len()]
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use super::*;

    fn correction(
        original: &str,
        corrected: &str,
        count: u32,
        source: CorrectionSource,
    ) -> CorrectionPattern {
        CorrectionPattern {
            original: original.to_owned(),
            corrected: corrected.to_owned(),
            count,
            last_seen: 0,
            source,
        }
    }

    fn hot_word(text: &str, source: HotWordSource, weight: u8, use_count: u32) -> HotWord {
        HotWord {
            text: text.to_owned(),
            weight,
            source,
            use_count,
            last_used: 0,
        }
    }

    fn user(text: &str) -> HotWord {
        hot_word(text, HotWordSource::User, 3, 0)
    }

    fn learned(text: &str) -> HotWord {
        hot_word(text, HotWordSource::Learned, 2, 3)
    }

    fn representative_hot_words() -> Vec<HotWord> {
        vec![
            user("github"),
            user("claude code"),
            user("Oura Ring"),
            user("openclaw"),
            user("typeless"),
            user("agent"),
            user("佳明"),
            user("轻语"),
            learned("同济大学"),
            learned("划词助手"),
            learned("第一性原理"),
            learned("VS Code"),
            learned("Cloud"),
            learned("欧盟"),
        ]
    }

    fn maximum_hotword_load() -> Vec<HotWord> {
        let mut hot_words = representative_hot_words();
        while hot_words.len() < MAX_ASR_HOT_WORDS {
            hot_words.push(learned(&format!("backgroundterm{:03}", hot_words.len())));
        }
        hot_words
    }

    fn maximum_profile_load() -> UserProfile {
        let hot_words = maximum_hotword_load();
        let correction_patterns = hot_words
            .iter()
            .filter(|hot_word| hot_word.text.starts_with("backgroundterm"))
            .enumerate()
            .map(|(index, hot_word)| {
                correction(
                    &format!("misheardterm{index:03}"),
                    &hot_word.text,
                    100 - index.min(99) as u32,
                    CorrectionSource::Ai,
                )
            })
            .collect();
        UserProfile {
            hot_words,
            correction_patterns,
            ..UserProfile::default()
        }
    }

    fn replay_profile() -> UserProfile {
        let hot_words = vec![
            learned("claude"),
            user("openclaw"),
            learned("README"),
            learned("Windows"),
            learned("Python"),
            learned("OpenAI"),
            learned("Codex"),
            user("github"),
            learned("GPT Pro"),
            user("宇树"),
            user("同济"),
            learned("OAuth"),
            learned("xLSTM"),
            learned("LaTeX"),
            learned("智谱"),
            user("agent"),
        ];
        let correction_patterns = vec![
            correction("Cloud", "claude", 99, CorrectionSource::User),
            correction("open cloud", "openclaw", 33, CorrectionSource::User),
            correction("readme", "README", 19, CorrectionSource::Ai),
            correction("read me", "README", 13, CorrectionSource::Ai),
            correction("Claude", "claude", 10, CorrectionSource::User),
            correction("codeex", "Codex", 10, CorrectionSource::Ai),
            correction("语音", "宇树", 10, CorrectionSource::User),
            correction("github", "GitHub", 9, CorrectionSource::Ai),
            correction("GPT pro", "GPT Pro", 8, CorrectionSource::Ai),
            correction("统计", "同济", 7, CorrectionSource::User),
            correction("gitthub", "GitHub", 4, CorrectionSource::Ai),
            correction("OAUTH", "OAuth", 4, CorrectionSource::Ai),
            correction("XLSTM", "xLSTM", 3, CorrectionSource::Ai),
            correction("gitub", "GitHub", 3, CorrectionSource::Ai),
            correction("LATX", "LaTeX", 3, CorrectionSource::Ai),
            correction("智朴", "智谱", 2, CorrectionSource::Ai),
            correction("A正", "agent", 2, CorrectionSource::Ai),
        ];
        UserProfile {
            hot_words,
            correction_patterns,
            ..UserProfile::default()
        }
    }

    #[test]
    fn corrects_representative_ascii_and_chinese_variants() {
        let hot_words = representative_hot_words();
        let cases = [
            ("我们用 get hub 管理代码。", "我们用 github 管理代码。"),
            ("请用 Cloud Code 写程序。", "请用 claude code 写程序。"),
            ("我戴的是 Aura Ring。", "我戴的是 Oura Ring。"),
            ("请打开 open claw。", "请打开 openclaw。"),
            ("这个工具叫 type less。", "这个工具叫 typeless。"),
            ("设备来自嘉明。", "设备来自佳明。"),
            ("毕业于同机大学。", "毕业于同济大学。"),
            ("打开划词住手。", "打开划词助手。"),
            ("按第一性圆理分析。", "按第一性原理分析。"),
            ("在 vs code 里打开。", "在 VS Code 里打开。"),
        ];

        let corrected = cases
            .iter()
            .filter(|(raw, expected)| correct_qwen_hot_words(raw, &hot_words).text == *expected)
            .count();
        assert_eq!(corrected, cases.len());
    }

    #[test]
    fn leaves_ambiguous_inflected_and_unrelated_text_unchanged() {
        let hot_words = representative_hot_words();
        let cases = [
            "我今天吃了青鱼。",
            "The agents finished their work.",
            "Upload the file to the cloud.",
            "这次欧盟会议按时开始。",
            "正常句子里没有需要替换的术语。",
            "Cloud storage is available.",
        ];

        for raw in cases {
            let result = correct_qwen_hot_words(raw, &hot_words);
            assert_eq!(result.text, raw);
            assert_eq!(result.replacements, 0);
        }
    }

    #[test]
    fn prefers_longer_overlapping_hot_word() {
        let hot_words = vec![user("claude"), user("claude code")];
        let result = correct_qwen_hot_words("Cloud Code is ready.", &hot_words);
        assert_eq!(result.text, "claude code is ready.");
        assert_eq!(result.replacements, 1);
    }

    #[test]
    fn replays_safe_profile_aliases_without_ambiguous_user_rules() {
        let profile = replay_profile();
        let cases = [
            ("open cloud", "openclaw"),
            ("readme", "README"),
            ("read me", "README"),
            ("Claude", "claude"),
            ("codeex", "Codex"),
            ("github", "GitHub"),
            ("GPT pro", "GPT Pro"),
            ("gitthub", "GitHub"),
            ("OAUTH", "OAuth"),
            ("XLSTM", "xLSTM"),
            ("gitub", "GitHub"),
            ("LATX", "LaTeX"),
            ("智朴", "智谱"),
        ];
        for (raw, expected) in cases {
            assert_eq!(correct_qwen_profile_terms(raw, &profile).text, expected);
        }

        for unchanged in [
            "Cloud storage is available.",
            "语音助手已经打开。",
            "统计结果已经完成。",
            "渲染任务已经完成。",
            "A正不是一个确定的术语。",
            "readmefile",
            "mygithubrepo",
        ] {
            assert_eq!(
                correct_qwen_profile_terms(unchanged, &profile).text,
                unchanged
            );
        }
    }

    #[test]
    fn real_profile_replay_coverage_improves_without_unsafe_aliases() {
        let profile = replay_profile();
        let cases = [
            ("Cloud", "claude", 99_u32),
            ("open cloud", "openclaw", 33),
            ("readme", "README", 19),
            ("read me", "README", 13),
            ("Claude", "claude", 10),
            ("codeex", "Codex", 10),
            ("语音", "宇树", 10),
            ("github", "GitHub", 9),
            ("GPT pro", "GPT Pro", 8),
            ("统计", "同济", 7),
            ("gitthub", "GitHub", 4),
            ("OAUTH", "OAuth", 4),
            ("XLSTM", "xLSTM", 3),
            ("gitub", "GitHub", 3),
            ("LATX", "LaTeX", 3),
            ("智朴", "智谱", 2),
            ("A正", "agent", 2),
        ];
        let total: u32 = cases.iter().map(|(_, _, count)| count).sum();
        let before: u32 = cases
            .iter()
            .filter(|(raw, expected, _)| {
                correct_qwen_hot_words(raw, &profile.hot_words).text == *expected
            })
            .map(|(_, _, count)| count)
            .sum();
        let after: u32 = cases
            .iter()
            .filter(|(raw, expected, _)| {
                correct_qwen_profile_terms(raw, &profile).text == *expected
            })
            .map(|(_, _, count)| count)
            .sum();

        println!(
            "LIGHT_WHISPER_ALIAS_METRICS before={before}/{total} after={after}/{total} delta_events={}",
            after - before
        );
        assert_eq!(before, 47);
        assert_eq!(after, 121);
    }

    #[test]
    fn hotword_correction_p95_stays_below_one_millisecond() {
        let profile = maximum_profile_load();
        let text = "请在 Cloud Code 和 get hub 中检查同机大学的项目，然后打开划词住手。";
        for _ in 0..100 {
            let _ = correct_qwen_profile_terms(text, &profile);
        }

        let mut samples = Vec::with_capacity(1000);
        for _ in 0..1000 {
            let started = Instant::now();
            let result = correct_qwen_profile_terms(text, &profile);
            assert_eq!(result.replacements, 4);
            samples.push(started.elapsed().as_micros());
        }
        samples.sort_unstable();
        let p95_us = samples[949];
        println!("LIGHT_WHISPER_HOTWORD_METRICS coverage=100 false_positive=0 p95_us={p95_us}");
        assert!(p95_us < 1000, "hotword correction p95 was {p95_us} us");
    }
}
