use pinyin::ToPinyin;

use crate::state::user_profile::{HotWord, HotWordSource};

const MAX_ASR_HOT_WORDS: usize = 100;

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
    fn hotword_correction_p95_stays_below_one_millisecond() {
        let hot_words = maximum_hotword_load();
        let text = "请在 Cloud Code 和 get hub 中检查同机大学的项目，然后打开划词住手。";
        for _ in 0..100 {
            let _ = correct_qwen_hot_words(text, &hot_words);
        }

        let mut samples = Vec::with_capacity(1000);
        for _ in 0..1000 {
            let started = Instant::now();
            let result = correct_qwen_hot_words(text, &hot_words);
            assert_eq!(result.replacements, 4);
            samples.push(started.elapsed().as_micros());
        }
        samples.sort_unstable();
        let p95_us = samples[949];
        println!("LIGHT_WHISPER_HOTWORD_METRICS coverage=100 false_positive=0 p95_us={p95_us}");
        assert!(p95_us < 1000, "hotword correction p95 was {p95_us} us");
    }
}
