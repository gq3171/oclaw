//! Query expansion with CJK-aware keyword extraction.
//! Port of Node openclaw's query-expansion.ts + hybrid.ts buildFtsQuery.

use std::collections::HashSet;
use std::sync::LazyLock;

// ── Character classification ────────────────────────────────────

/// CJK Unified Ideographs (Chinese/Japanese Kanji).
fn is_cjk(c: char) -> bool {
    matches!(c,
        '\u{3400}'..='\u{9fff}' |
        '\u{f900}'..='\u{faff}' |
        '\u{20000}'..='\u{2a6df}'
    )
}

/// Korean Hangul syllables and Jamo.
fn is_hangul(c: char) -> bool {
    matches!(c, '\u{ac00}'..='\u{d7af}' | '\u{3131}'..='\u{3163}')
}

pub fn has_cjk(s: &str) -> bool {
    s.chars().any(is_cjk)
}

fn is_unicode_letter_or_digit(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

// ── Stop words ──────────────────────────────────────────────────

static STOP_WORDS_EN: LazyLock<HashSet<&'static str>> = LazyLock::new(|| {
    [
        // Articles and determiners
        "a",
        "an",
        "the",
        "this",
        "that",
        "these",
        "those",
        // Pronouns
        "i",
        "me",
        "my",
        "we",
        "our",
        "you",
        "your",
        "he",
        "she",
        "it",
        "they",
        "them",
        // Common verbs
        "is",
        "are",
        "was",
        "were",
        "be",
        "been",
        "being",
        "have",
        "has",
        "had",
        "do",
        "does",
        "did",
        "will",
        "would",
        "could",
        "should",
        "can",
        "may",
        "might",
        // Prepositions
        "in",
        "on",
        "at",
        "to",
        "for",
        "of",
        "with",
        "by",
        "from",
        "about",
        "into",
        "through",
        "during",
        "before",
        "after",
        "above",
        "below",
        "between",
        "under",
        "over",
        // Conjunctions
        "and",
        "or",
        "but",
        "if",
        "then",
        "because",
        "as",
        "while",
        "when",
        "where",
        "what",
        "which",
        "who",
        "how",
        "why",
        // Time references (vague)
        "yesterday",
        "today",
        "tomorrow",
        "earlier",
        "later",
        "recently",
        "ago",
        "just",
        "now",
        // Vague references
        "thing",
        "things",
        "stuff",
        "something",
        "anything",
        "everything",
        "nothing",
        // Question/request words
        "please",
        "help",
        "find",
        "show",
        "get",
        "tell",
        "give",
    ]
    .into_iter()
    .collect()
});

static STOP_WORDS_ZH: LazyLock<HashSet<&'static str>> = LazyLock::new(|| {
    [
        // Pronouns
        "我",
        "我们",
        "你",
        "你们",
        "他",
        "她",
        "它",
        "他们",
        "这",
        "那",
        "这个",
        "那个",
        "这些",
        "那些",
        // Auxiliary words
        "的",
        "了",
        "着",
        "过",
        "得",
        "地",
        "吗",
        "呢",
        "吧",
        "啊",
        "呀",
        "嘛",
        "啦",
        // Verbs (common, vague)
        "是",
        "有",
        "在",
        "被",
        "把",
        "给",
        "让",
        "用",
        "到",
        "去",
        "来",
        "做",
        "说",
        "看",
        "找",
        "想",
        "要",
        "能",
        "会",
        "可以",
        // Prepositions and conjunctions
        "和",
        "与",
        "或",
        "但",
        "但是",
        "因为",
        "所以",
        "如果",
        "虽然",
        "而",
        "也",
        "都",
        "就",
        "还",
        "又",
        "再",
        "才",
        "只",
        // Time (vague)
        "之前",
        "以前",
        "之后",
        "以后",
        "刚才",
        "现在",
        "昨天",
        "今天",
        "明天",
        "最近",
        // Vague references
        "东西",
        "事情",
        "事",
        "什么",
        "哪个",
        "哪些",
        "怎么",
        "为什么",
        "多少",
        // Question/request words
        "请",
        "帮",
        "帮忙",
        "告诉",
    ]
    .into_iter()
    .collect()
});

static STOP_WORDS_KO: LazyLock<HashSet<&'static str>> = LazyLock::new(|| {
    [
        // Particles (조사)
        "은",
        "는",
        "이",
        "가",
        "을",
        "를",
        "의",
        "에",
        "에서",
        "로",
        "으로",
        "와",
        "과",
        "도",
        "만",
        "까지",
        "부터",
        "한테",
        "에게",
        "께",
        "처럼",
        "같이",
        "보다",
        "마다",
        "밖에",
        "대로",
        // Pronouns (대명사)
        "나",
        "나는",
        "내가",
        "나를",
        "너",
        "우리",
        "저",
        "저희",
        "그",
        "그녀",
        "그들",
        "이것",
        "저것",
        "그것",
        "여기",
        "저기",
        "거기",
        // Common verbs / auxiliaries
        "있다",
        "없다",
        "하다",
        "되다",
        "이다",
        "아니다",
        "보다",
        "주다",
        "오다",
        "가다",
        // Nouns (의존 명사 / vague)
        "것",
        "거",
        "등",
        "수",
        "때",
        "곳",
        "중",
        "분",
        // Adverbs
        "잘",
        "더",
        "또",
        "매우",
        "정말",
        "아주",
        "많이",
        "너무",
        "좀",
        // Conjunctions
        "그리고",
        "하지만",
        "그래서",
        "그런데",
        "그러나",
        "또는",
        "그러면",
        // Question words
        "왜",
        "어떻게",
        "뭐",
        "언제",
        "어디",
        "누구",
        "무엇",
        "어떤",
        // Time (vague)
        "어제",
        "오늘",
        "내일",
        "최근",
        "지금",
        "아까",
        "나중",
        "전에",
        // Request words
        "제발",
        "부탁",
    ]
    .into_iter()
    .collect()
});

/// Korean trailing particles, sorted by descending length for longest-match-first.
const KO_TRAILING_PARTICLES: &[&str] = &[
    "에서", "으로", "에게", "한테", "처럼", "같이", "보다", "까지", "부터", "마다", "밖에", "대로",
    "은", "는", "이", "가", "을", "를", "의", "에", "로", "와", "과", "도", "만",
];

// ── Korean helpers ──────────────────────────────────────────────

/// Strip a trailing Korean particle from a token. Returns `None` if no particle matched.
fn strip_korean_trailing_particle(token: &str) -> Option<String> {
    for &particle in KO_TRAILING_PARTICLES {
        if token.len() > particle.len() && token.ends_with(particle) {
            return Some(token[..token.len() - particle.len()].to_string());
        }
    }
    None
}

/// Prevent bogus one-syllable stems from words like "논의" → "논".
fn is_useful_korean_stem(stem: &str) -> bool {
    if stem.chars().any(is_hangul) {
        return stem.chars().count() >= 2;
    }
    // Keep stripped ASCII stems for mixed tokens like "API를" → "api".
    stem.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

// ── Keyword validation ──────────────────────────────────────────

fn is_valid_keyword(token: &str) -> bool {
    if token.is_empty() {
        return false;
    }
    // Skip very short English words (likely stop words or fragments)
    if token.chars().all(|c| c.is_ascii_alphabetic()) && token.len() < 3 {
        return false;
    }
    // Skip pure numbers
    if token.chars().all(|c| c.is_ascii_digit()) {
        return false;
    }
    // Skip tokens that are all punctuation/symbols
    if token
        .chars()
        .all(|c| c.is_ascii_punctuation() || (!c.is_alphanumeric() && !c.is_ascii()))
    {
        // More precise: check Unicode categories
        if !token.chars().any(is_unicode_letter_or_digit) {
            return false;
        }
    }
    true
}

fn is_stop_word(token: &str) -> bool {
    STOP_WORDS_EN.contains(token) || STOP_WORDS_ZH.contains(token) || STOP_WORDS_KO.contains(token)
}

// ── Tokenizer ───────────────────────────────────────────────────

/// Split text on whitespace and Unicode punctuation, returning non-empty segments.
fn split_segments(text: &str) -> Vec<String> {
    let normalized = text.to_lowercase();
    let mut segments = Vec::new();
    let mut current = String::new();

    for c in normalized.chars() {
        if c.is_whitespace()
            || (c.is_ascii_punctuation() && c != '_')
            || matches!(c,
                '\u{3000}'..='\u{303f}' | // CJK punctuation
                '\u{ff00}'..='\u{ffef}' | // Fullwidth forms
                '\u{2000}'..='\u{206f}'   // General punctuation
            )
        {
            if !current.is_empty() {
                segments.push(std::mem::take(&mut current));
            }
        } else {
            current.push(c);
        }
    }
    if !current.is_empty() {
        segments.push(current);
    }
    segments
}

/// Tokenize text with CJK-aware splitting.
/// For Chinese: extract unigrams + bigrams from CJK character sequences.
/// For Korean: keep words, strip trailing particles, emit stems.
/// For English/other: keep as single tokens.
fn tokenize(text: &str) -> Vec<String> {
    let segments = split_segments(text);
    let mut tokens = Vec::new();

    for segment in &segments {
        let has_cjk_chars = segment.chars().any(is_cjk);
        let has_hangul_chars = segment.chars().any(is_hangul);

        if has_cjk_chars {
            // Mixed segment (e.g. "请帮我找api文档"): split into CJK runs and non-CJK runs
            let mut cjk_run: Vec<char> = Vec::new();
            let mut ascii_run = String::new();

            let flush_cjk = |run: &mut Vec<char>, toks: &mut Vec<String>| {
                if run.is_empty() {
                    return;
                }
                // Unigrams
                for &c in run.iter() {
                    toks.push(c.to_string());
                }
                // Bigrams
                for i in 0..run.len().saturating_sub(1) {
                    let mut bigram = String::new();
                    bigram.push(run[i]);
                    bigram.push(run[i + 1]);
                    toks.push(bigram);
                }
                run.clear();
            };
            let flush_ascii = |run: &mut String, toks: &mut Vec<String>| {
                if run.is_empty() {
                    return;
                }
                toks.push(std::mem::take(run));
            };

            for c in segment.chars() {
                if is_cjk(c) {
                    flush_ascii(&mut ascii_run, &mut tokens);
                    cjk_run.push(c);
                } else if c.is_alphanumeric() || c == '_' {
                    flush_cjk(&mut cjk_run, &mut tokens);
                    ascii_run.push(c);
                } else {
                    flush_cjk(&mut cjk_run, &mut tokens);
                    flush_ascii(&mut ascii_run, &mut tokens);
                }
            }
            flush_cjk(&mut cjk_run, &mut tokens);
            flush_ascii(&mut ascii_run, &mut tokens);
        } else if has_hangul_chars {
            // Korean: keep word, strip particles, emit useful stems
            let seg_lower = segment.as_str();
            let stem = strip_korean_trailing_particle(seg_lower);
            let stem_is_stop = stem.as_deref().is_some_and(|s| STOP_WORDS_KO.contains(s));

            if !STOP_WORDS_KO.contains(seg_lower) && !stem_is_stop {
                tokens.push(segment.clone());
            }
            if let Some(ref s) = stem
                && !STOP_WORDS_KO.contains(s.as_str())
                && is_useful_korean_stem(s)
            {
                tokens.push(s.clone());
            }
        } else {
            // English / other: keep as single token
            tokens.push(segment.clone());
        }
    }

    tokens
}

// ── Public API ──────────────────────────────────────────────────

/// Extract meaningful keywords from a conversational query.
///
/// Mirrors Node's `extractKeywords()` from query-expansion.ts.
/// - "that thing we discussed about the API" → ["discussed", "api"]
/// - "之前讨论的那个方案" → ["讨", "论", "方", "案", "讨论", "方案"]
pub fn extract_keywords(query: &str) -> Vec<String> {
    let tokens = tokenize(query);
    let mut keywords = Vec::new();
    let mut seen = HashSet::new();

    for token in &tokens {
        if is_stop_word(token) {
            continue;
        }
        if !is_valid_keyword(token) {
            continue;
        }
        if seen.contains(token.as_str()) {
            continue;
        }
        seen.insert(token.clone());
        keywords.push(token.clone());
    }

    keywords
}

/// Build a safe FTS5 query from raw user input.
///
/// Mirrors Node's `buildFtsQuery()` from hybrid.ts:
/// - Extract Unicode letter/number/underscore tokens
/// - Quote each token (stripping internal quotes)
/// - Join with AND
///
/// Returns `None` if no valid tokens found.
pub fn build_fts5_query(raw: &str) -> Option<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();

    for c in raw.chars() {
        if is_unicode_letter_or_digit(c) {
            current.push(c);
        } else if !current.is_empty() {
            tokens.push(std::mem::take(&mut current));
        }
    }
    if !current.is_empty() {
        tokens.push(current);
    }

    if tokens.is_empty() {
        return None;
    }

    let quoted: Vec<String> = tokens
        .iter()
        .map(|t| {
            let clean: String = t.chars().filter(|&c| c != '"').collect();
            format!("\"{}\"", clean)
        })
        .collect();

    Some(quoted.join(" AND "))
}

/// Convert FTS5 BM25 rank to a 0..1 score.
///
/// Mirrors Node's `bm25RankToScore()` from hybrid.ts:
/// `1 / (1 + max(0, rank))`
pub fn bm25_rank_to_score(rank: f64) -> f64 {
    let normalized = if rank.is_finite() {
        rank.max(0.0)
    } else {
        999.0
    };
    1.0 / (1.0 + normalized)
}

// ── Tests ───────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_fts5_query_basic() {
        assert_eq!(
            build_fts5_query("hello world"),
            Some("\"hello\" AND \"world\"".to_string())
        );
    }

    #[test]
    fn test_build_fts5_query_chinese() {
        assert_eq!(
            build_fts5_query("你叫什么?"),
            Some("\"你叫什么\"".to_string())
        );
    }

    #[test]
    fn test_build_fts5_query_special_chars() {
        // FTS5 special chars like ?, *, ^ should be stripped
        assert_eq!(
            build_fts5_query("test? foo* bar^"),
            Some("\"test\" AND \"foo\" AND \"bar\"".to_string())
        );
    }

    #[test]
    fn test_build_fts5_query_empty() {
        assert_eq!(build_fts5_query("???"), None);
        assert_eq!(build_fts5_query(""), None);
        assert_eq!(build_fts5_query("   "), None);
    }

    #[test]
    fn test_build_fts5_query_strips_quotes() {
        assert_eq!(
            build_fts5_query(r#"say "hello""#),
            Some("\"say\" AND \"hello\"".to_string())
        );
    }

    #[test]
    fn test_extract_keywords_english() {
        let kw = extract_keywords("that thing we discussed about the API");
        assert!(kw.contains(&"discussed".to_string()));
        assert!(kw.contains(&"api".to_string()));
        assert!(!kw.contains(&"the".to_string()));
        assert!(!kw.contains(&"thing".to_string()));
    }

    #[test]
    fn test_extract_keywords_chinese() {
        let kw = extract_keywords("之前讨论的那个方案");
        // Should contain bigrams like "讨论", "方案"
        assert!(kw.contains(&"讨论".to_string()));
        assert!(kw.contains(&"方案".to_string()));
        // Stop words should be filtered
        assert!(!kw.contains(&"之前".to_string()));
        assert!(!kw.contains(&"的".to_string()));
    }

    #[test]
    fn test_extract_keywords_mixed() {
        let kw = extract_keywords("请帮我找API文档");
        assert!(kw.contains(&"api".to_string()) || kw.iter().any(|k| k.contains("api")));
    }

    #[test]
    fn test_bm25_rank_to_score() {
        assert!((bm25_rank_to_score(0.0) - 1.0).abs() < f64::EPSILON);
        assert!((bm25_rank_to_score(1.0) - 0.5).abs() < f64::EPSILON);
        assert!(bm25_rank_to_score(f64::NAN) < 0.01);
        assert!(bm25_rank_to_score(f64::INFINITY) < 0.01);
    }

    #[test]
    fn test_has_cjk() {
        assert!(has_cjk("你好"));
        assert!(has_cjk("hello你好"));
        assert!(!has_cjk("hello world"));
        assert!(!has_cjk("한국어")); // Korean is not CJK
    }

    #[test]
    fn test_korean_particle_stripping() {
        let stem = strip_korean_trailing_particle("프로젝트를");
        assert_eq!(stem, Some("프로젝트".to_string()));
    }

    #[test]
    fn test_korean_stop_words_filtered() {
        let kw = extract_keywords("이것은 프로젝트입니다");
        assert!(!kw.contains(&"이것".to_string()));
    }
}
