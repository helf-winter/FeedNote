use std::{collections::BTreeSet, sync::OnceLock};

use jieba_rs::Jieba;
use serde::{Deserialize, Serialize};

pub const INDEX_VERSION: i64 = 1;

const STOP_WORDS: &[&str] = &[
    "一个", "一些", "这个", "那个", "这些", "那些", "可以", "需要", "进行", "已经", "还是", "就是",
    "如果", "然后", "现在", "之后", "之前", "时候", "用户", "内容", "功能", "我们", "你们", "他们",
    "自己", "没有", "不是", "什么", "怎么", "一下", "以及", "the", "and", "for", "with", "from",
    "this", "that", "have", "will",
];

const SYNONYM_GROUPS: &[(&str, &[&str])] = &[
    ("移动端", &["移动端", "手机端", "手机", "小程序", "mobile"]),
    ("运行时", &["运行时", "runtime"]),
    ("前端", &["前端", "web", "react", "vue"]),
    ("桌面端", &["桌面端", "客户端", "desktop"]),
    ("FeedNote", &["feednote", "桌面日记"]),
];

const KNOWN_ENTITIES: &[&str] = &[
    "feednote", "runtime", "react", "vue", "tauri", "electron", "windows", "android", "ios",
];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecallDocument {
    pub tokens: Vec<String>,
    pub entities: Vec<String>,
    pub phrases: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct RecallScore {
    pub score: f64,
    pub matched_entities: Vec<String>,
    pub matched_terms: Vec<String>,
}

pub fn analyze(text: &str) -> RecallDocument {
    let normalized = normalize(text);
    let mut tokens = BTreeSet::new();
    let mut phrases = BTreeSet::new();

    for word in jieba().cut(&normalized, false) {
        let word = clean_term(word);
        if useful_term(&word) {
            tokens.insert(word);
        }
    }

    for run in cjk_runs(&normalized) {
        let chars: Vec<char> = run.chars().collect();
        for width in [2_usize, 3] {
            if chars.len() < width {
                continue;
            }
            for chunk in chars.windows(width) {
                let phrase: String = chunk.iter().collect();
                if useful_term(&phrase) {
                    phrases.insert(phrase);
                }
            }
        }
    }

    let all_terms: Vec<String> = tokens.iter().chain(phrases.iter()).cloned().collect();
    for (canonical, variants) in SYNONYM_GROUPS {
        if variants.iter().any(|variant| {
            all_terms
                .iter()
                .any(|term| term.eq_ignore_ascii_case(variant))
        }) {
            tokens.insert(canonical.to_lowercase());
        }
    }

    let mut entities: BTreeSet<String> = tokens
        .iter()
        .filter(|term| is_entity(term))
        .cloned()
        .collect();
    entities.extend(proper_ascii_entities(text));

    RecallDocument {
        tokens: tokens.into_iter().collect(),
        entities: entities.into_iter().collect(),
        phrases: phrases.into_iter().collect(),
    }
}

pub fn score(
    context: &RecallDocument,
    memo: &RecallDocument,
    bm25_score: f64,
    penalties: &BTreeSet<String>,
) -> Option<RecallScore> {
    let context_entities: BTreeSet<_> = context.entities.iter().cloned().collect();
    let memo_entities: BTreeSet<_> = memo.entities.iter().cloned().collect();
    let matched_entities: Vec<String> = context_entities
        .intersection(&memo_entities)
        .cloned()
        .collect();
    if matched_entities.is_empty() {
        return None;
    }

    let entity_set: BTreeSet<_> = matched_entities.iter().cloned().collect();
    let context_topics: BTreeSet<_> = context
        .tokens
        .iter()
        .chain(context.phrases.iter())
        .filter(|term| !entity_set.contains(*term))
        .cloned()
        .collect();
    let memo_topics: BTreeSet<_> = memo
        .tokens
        .iter()
        .chain(memo.phrases.iter())
        .filter(|term| !entity_set.contains(*term))
        .cloned()
        .collect();
    let raw_matched_terms: Vec<String> =
        context_topics.intersection(&memo_topics).cloned().collect();
    let penalty_hits = matched_entities
        .iter()
        .chain(raw_matched_terms.iter())
        .filter(|term| penalties.contains(*term))
        .count();
    let mut matched_terms: Vec<String> = raw_matched_terms
        .into_iter()
        .filter(|term| !penalties.contains(term))
        .collect();
    matched_terms.sort_by_key(|term| std::cmp::Reverse(term.chars().count()));
    matched_terms.dedup();
    if matched_terms.is_empty() {
        return None;
    }

    let topic_score = (matched_terms.len() as f64 / 3.0).min(1.0);
    let context_phrases: BTreeSet<_> = context.phrases.iter().collect();
    let memo_phrases: BTreeSet<_> = memo.phrases.iter().collect();
    let phrase_score = if context_phrases.intersection(&memo_phrases).next().is_some() {
        1.0
    } else {
        0.0
    };
    let penalty = (penalty_hits as f64 * 0.08).min(0.35);
    let score =
        (0.45 + 0.25 * topic_score + 0.20 * bm25_score.clamp(0.0, 1.0) + 0.10 * phrase_score
            - penalty)
            .clamp(0.0, 1.0);
    (score >= 0.65).then_some(RecallScore {
        score,
        matched_entities,
        matched_terms: matched_terms.into_iter().take(6).collect(),
    })
}

pub fn search_terms(document: &RecallDocument) -> Vec<String> {
    let mut terms: Vec<String> = document
        .entities
        .iter()
        .chain(document.tokens.iter())
        .chain(document.phrases.iter())
        .filter(|term| useful_term(term))
        .cloned()
        .collect();
    terms.sort_by_key(|term| std::cmp::Reverse(term.chars().count()));
    terms.dedup();
    terms.truncate(48);
    terms
}

fn jieba() -> &'static Jieba {
    static JIEBA: OnceLock<Jieba> = OnceLock::new();
    JIEBA.get_or_init(Jieba::new)
}

fn normalize(text: &str) -> String {
    text.chars()
        .map(|character| match character {
            '\u{3000}' => ' ',
            '\u{ff01}'..='\u{ff5e}' => {
                char::from_u32(character as u32 - 0xfee0).unwrap_or(character)
            }
            _ => character,
        })
        .collect::<String>()
        .to_lowercase()
}

fn clean_term(term: &str) -> String {
    term.trim_matches(|character: char| !character.is_alphanumeric() && !is_cjk(character))
        .to_string()
}

fn useful_term(term: &str) -> bool {
    let length = term.chars().count();
    length >= 2
        && !STOP_WORDS.contains(&term)
        && term
            .chars()
            .any(|character| character.is_alphanumeric() || is_cjk(character))
}

fn is_entity(term: &str) -> bool {
    if KNOWN_ENTITIES.contains(&term) {
        return true;
    }
    let has_ascii = term
        .chars()
        .any(|character| character.is_ascii_alphabetic());
    if has_ascii {
        return term.chars().count() >= 3
            && term
                .chars()
                .any(|character| character.is_ascii_digit() || matches!(character, '-' | '_'));
    }
    let length = term.chars().count();
    length >= 3
        && (term.ends_with("公司")
            || term.ends_with("集团")
            || term.ends_with("项目")
            || term.ends_with("产品")
            || term.ends_with("系统")
            || term.ends_with("平台")
            || term.ends_with("应用"))
}

fn proper_ascii_entities(text: &str) -> BTreeSet<String> {
    text.split(|character: char| {
        !character.is_ascii_alphanumeric() && !matches!(character, '-' | '_')
    })
    .filter(|term| term.len() >= 3)
    .filter(|term| {
        let uppercase = term
            .chars()
            .filter(|character| character.is_ascii_uppercase())
            .count();
        uppercase >= 2
            || term
                .chars()
                .skip(1)
                .any(|character| character.is_ascii_uppercase())
    })
    .map(str::to_lowercase)
    .collect()
}

fn cjk_runs(text: &str) -> Vec<String> {
    let mut runs = Vec::new();
    let mut current = String::new();
    for character in text.chars() {
        if is_cjk(character) {
            current.push(character);
        } else if !current.is_empty() {
            runs.push(std::mem::take(&mut current));
        }
    }
    if !current.is_empty() {
        runs.push(current);
    }
    runs
}

fn is_cjk(character: char) -> bool {
    matches!(character, '\u{3400}'..='\u{4dbf}' | '\u{4e00}'..='\u{9fff}')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn synonyms_and_phrases_bridge_mobile_wording() {
        let current = analyze("继续做 FeedNote 手机端开发");
        let memo = analyze("FeedNote 移动端有一个离线同步的创新想法");
        let score = score(&current, &memo, 1.0, &BTreeSet::new()).unwrap();
        assert!(score.score >= 0.65);
        assert!(score.matched_entities.contains(&"feednote".to_string()));
        assert!(score.matched_terms.contains(&"移动端".to_string()));
    }

    #[test]
    fn entity_without_a_second_topic_does_not_recall() {
        let current = analyze("FeedNote");
        let memo = analyze("FeedNote 移动端创新想法");
        assert!(score(&current, &memo, 1.0, &BTreeSet::new()).is_none());
    }

    #[test]
    fn generic_topic_overlap_without_entity_does_not_recall() {
        let current = analyze("今天继续进行移动端开发");
        let memo = analyze("以后可以尝试手机端离线同步");
        assert!(score(&current, &memo, 1.0, &BTreeSet::new()).is_none());

        let current = analyze("continue mobile development and offline sync");
        let memo = analyze("mobile offline sync idea");
        assert!(score(&current, &memo, 1.0, &BTreeSet::new()).is_none());
    }

    #[test]
    fn camel_case_product_names_are_entities() {
        let current = analyze("继续推进 NoteBridge 离线同步");
        let memo = analyze("NoteBridge 移动端离线同步方案");
        assert!(score(&current, &memo, 1.0, &BTreeSet::new()).is_some());
    }
}
