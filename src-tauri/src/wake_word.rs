use std::io::{self, BufRead, Write};

pub const WAKE_WORDS: &[&str] = &[
    "hey google",
    "ok google",
    "okay google",
    "google",
    "hey inex",
    "ok inex",
    "inex",
    "hey index",
    "ok index",
    "index",
    "hey in x",
    "in x",
    "hey annex",
    "annex",
    "hey innex",
    "innex",
    "hey nx",
    "nx",
    "hey computer",
    "computer",
    "hey jarvis",
    "jarvis",
    "alexa",
];

pub fn ask(prompt: &str) -> String {
    print!("{prompt}");

    io::stdout().flush().ok();

    let mut s = String::new();

    io::stdin().lock().read_line(&mut s).ok();

    s
}

pub fn norm(s: &str) -> String {
    s.to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn strip_phrases(text: &str, phrases: &[&str]) -> String {
    let words: Vec<&str> = text.split_whitespace().collect();
    let mut ps: Vec<Vec<&str>> = phrases.iter().map(|p| p.split_whitespace().collect()).collect();
    ps.sort_by(|a, b| b.len().cmp(&a.len()));
    let mut out: Vec<&str> = Vec::new();
    let mut i = 0;
    while i < words.len() {
        let mut hit = 0;
        for p in &ps {
            if !p.is_empty()
                && i + p.len() <= words.len()
                && p.iter().enumerate().all(|(k, w)| words[i + k] == *w)
            {
                hit = p.len();
                break;
            }
        }
        if hit > 0 { i += hit; } else { out.push(words[i]); i += 1; }
    }
    out.join(" ")
}

pub fn url_encode(s: &str) -> String {
    let mut out = String::new();

    for b in s.bytes() {
        match b {
            b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }

            b' ' => out.push('+'),

            _ => {
                out.push_str(&format!("%{:02X}", b));
            }
        }
    }

    out
}

// 0.0 .. 1.0 similarity between what was said and a candidate name.

pub fn score(query: &str, cand: &str) -> f64 {
    let q = norm(query);

    let c = norm(cand);

    if q.is_empty() || c.is_empty() {
        return 0.0;
    }

    if q == c {
        return 1.0;
    }

    let mut best = strsim::jaro_winkler(&q, &c);

    for w in c.split_whitespace() {
        best = best.max(strsim::jaro_winkler(&q, w));
    }

    if q.len() >= 3 && c.contains(&q) {
        best = best.max(0.95);
    }

    if c.len() >= 3 && q.contains(&c) {
        best = best.max(0.9);
    }

    best
}

pub fn pick(n: usize, f: impl Fn(usize) -> f64) -> Option<(usize, f64)> {
    (0..n)
        .map(|i| (i, f(i)))
        .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
}

pub fn is_only_wake_words(text: &str) -> bool {
    let mut rest = norm(text);

    for &ww in WAKE_WORDS {
        rest = rest.replace(ww, " ");
    }

    rest.split_whitespace().next().is_none()
}

pub fn get_active_wake_words(chosen: &str) -> Vec<&'static str> {
    match chosen.to_lowercase().as_str() {
        "jarvis" | "hey jarvis" => vec![
            "hey jarvis",
            "ok jarvis",
            "okay jarvis",
            "hi jarvis",
            "a jarvis",
            "jarvis",
            "hey jarvish",
            "ok jarvish",
            "jarvish",
            "hey jarves",
            "jarves",
            "hey jervis",
            "jervis",
            "hey jarviz",
            "jarviz",
            "hey darvis",
            "darvis",
            "hey charvis",
            "charvis",
            "hey harvis",
            "harvis",
            "hey travis",
            "ok travis",
            "travis",
            "hey service",
            "service",
            "hey java",
            "java",
            "hey job is",
            "job is",
            "hey harvest",
            "harvest",
            "hey jar vis",
            "jar vis",
            "hey jar was",
            "jar was",
            "jharvis",
            "tarvis",
        ],
        "pc" | "hey pc" | "computer" | "hey computer" => vec![
            "hey pc",
            "ok pc",
            "okay pc",
            "hi pc",
            "a pc",
            "pc",
            "hey p c",
            "ok p c",
            "okay p c",
            "hi p c",
            "p c",
            "hey computer",
            "ok computer",
            "okay computer",
            "hi computer",
            "computer",
            "hey bc",
            "ok bc",
            "hey b c",
            "b c",
            "apc",
            "hey apc",
            "hey pcs",
            "pcs",
        ],
        "alexa" | "hey alexa" => vec![
            "hey alexa",
            "ok alexa",
            "okay alexa",
            "hi alexa",
            "a alexa",
            "alexa",
            "hey alaxa",
            "alaxa",
            "hey elixa",
            "elixa",
            "hey alexia",
            "alexia",
            "hey alexis",
            "alexis",
            "hey aleksa",
            "aleksa",
            "hey alex",
            "ok alex",
            "alex",
            "hey lexa",
            "lexa",
            "a lex a",
        ],
        "inex" | "hey inex" => vec![
            "hey inex",
            "ok inex",
            "inex",
            "hey lenex",
            "ok lenex",
            "lenex",
            "hey linux",
            "ok linux",
            "linux",
            "hey lynx",
            "lynx",
            "hey lenox",
            "lenox",
            "hey index",
            "ok index",
            "index",
            "hey in x",
            "in x",
            "hey annex",
            "annex",
            "hey innex",
            "innex",
            "hey nx",
            "nx",
        ],
        _ => vec![
            "hey google",
            "ok google",
            "okay google",
            "google",
            "hi google",
            "hai google",
            "k google",
            "a google",
            "hey googel",
            "o google",
        ],
    }
}

pub fn collapse_pc_tokens(s: &str) -> String {
    let mut padded = format!(" {s} ");
    padded = padded.replace(" p c ", " pc ").replace(" b c ", " bc ");
    padded.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub fn find_word_phrase(haystack: &str, phrase: &str) -> Option<(usize, usize)> {
    let mut offset = 0usize;
    while let Some(pos) = haystack[offset..].find(phrase) {
        let start = offset + pos;
        let end = start + phrase.len();
        let left_ok = start == 0 || haystack.as_bytes()[start - 1] == b' ';
        let right_ok = end == haystack.len() || haystack.as_bytes()[end] == b' ';
        if left_ok && right_ok {
            return Some((start, end));
        }
        offset = start + phrase.len().max(1);
    }
    None
}

pub fn extract_command(
    raw_text: &str,
    require_wake_word: bool,
    chosen_wake_word: &str,
) -> Option<String> {
    let lower = raw_text.to_lowercase();
    let cleaned = lower
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { ' ' })
        .collect::<String>();
    let mut norm_text = collapse_pc_tokens(&cleaned);

    if !require_wake_word {
        return Some(norm_text);
    }

    let active_words = get_active_wake_words(chosen_wake_word);
    let mut matched_wake_word = false;

    // 1. Exact or prefix word match against any active wake word or phonetic variant
    for &ww in &active_words {
        if norm_text == ww {
            norm_text.clear();
            matched_wake_word = true;
            break;
        }
        if let Some((start, end)) = find_word_phrase(&norm_text, ww) {
            if start == 0 {
                norm_text = norm_text[end..].trim().to_string();
                matched_wake_word = true;
                break;
            }
        }
    }

    // 2. Check if wake word appears anywhere on word boundaries in the sentence
    if !matched_wake_word {
        for &ww in &active_words {
            if let Some((_, end)) = find_word_phrase(&norm_text, ww) {
                norm_text = norm_text[end..].trim().to_string();
                matched_wake_word = true;
                break;
            }
        }
    }

    // 3. Fuzzy similarity on initial 1 or 2 words (slight STT distortion)
    if !matched_wake_word {
        let words: Vec<&str> = norm_text.split_whitespace().collect();
        if !words.is_empty() {
            let primary_key = match chosen_wake_word.to_lowercase().as_str() {
                "jarvis" | "hey jarvis" => "jarvis",
                "alexa" | "hey alexa" => "alexa",
                "inex" | "hey inex" => "inex",
                "pc" | "hey pc" | "computer" | "hey computer" => "pc",
                _ => "google",
            };

            // Only run partial-similarity fuzzy matching on words >= 4 chars (2-char "pc" uses active_words above)
            if primary_key.len() >= 4 {
                let sc0 = strsim::jaro_winkler(words[0], primary_key);
                if sc0 >= 0.78 {
                    matched_wake_word = true;
                    norm_text = words[1..].join(" ");
                } else if words.len() >= 2 {
                    let prefix_greet = matches!(words[0], "hey" | "ok" | "okay" | "hi" | "a" | "o");
                    let sc1 = strsim::jaro_winkler(words[1], primary_key);
                    let w2 = format!("{} {}", words[0], words[1]);
                    let target2 = format!("hey {}", primary_key);
                    let sc2 = strsim::jaro_winkler(&w2, &target2);
                    if (prefix_greet && sc1 >= 0.78) || sc2 >= 0.80 {
                        matched_wake_word = true;
                        norm_text = words[2..].join(" ");
                    }
                }
            }
        }
    }

    if matched_wake_word {
        loop {
            let mut stripped = false;
            for &ww in &active_words {
                if norm_text == ww {
                    norm_text.clear();
                    stripped = true;
                    break;
                }
                if let Some((start, end)) = find_word_phrase(&norm_text, ww) {
                    if start == 0 {
                        norm_text = norm_text[end..].trim().to_string();
                        stripped = true;
                        break;
                    }
                }
            }
            if !stripped {
                break;
            }
        }

        if norm_text.is_empty() {
            return Some(String::new()); // Wake word only
        }

        return Some(norm_text);
    }

    None
}
