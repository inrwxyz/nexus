//! Command normalization, vocabulary action parsing, and timer duration parsing.

use crate::vocab::Vocab;
use crate::wake_word::norm;

#[derive(Clone, Debug)]
pub struct Parsed {
    pub action: String,
    pub target: String,
}

pub fn parse(text: &str, v: &Vocab) -> Option<Parsed> {
    let tokens: Vec<String> = norm(text).split_whitespace().map(String::from).collect();
    let mut best: Option<(usize, usize, String)> = None;

    for (action, phrases) in &v.actions {
        for p in phrases {
            let pt: Vec<&str> = p.split_whitespace().collect();
            if pt.is_empty() || tokens.len() < pt.len() {
                continue;
            }
            for i in 0..=(tokens.len() - pt.len()) {
                let hit = tokens[i..i + pt.len()]
                    .iter()
                    .map(|s| s.as_str())
                    .eq(pt.iter().copied());
                if hit {
                    let better = match &best {
                        None => true,
                        Some((bp, bl, _)) => i < *bp || (i == *bp && pt.len() > *bl),
                    };
                    if better {
                        best = Some((i, pt.len(), action.clone()));
                    }
                    break;
                }
            }
        }
    }

    let (pos, len, action) = best?;
    let rest: Vec<&String> = tokens
        .iter()
        .enumerate()
        .filter(|(i, t)| (*i < pos || *i >= pos + len) && !v.ignore.contains(*t))
        .map(|(_, t)| t)
        .collect();

    Some(Parsed {
        action,
        target: rest.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(" "),
    })
}

pub fn media_nav_intent(lnorm: &str) -> Option<bool> {
    const NEXT: &[&str] = &["next", "skip", "agla", "agli"];
    const PREV: &[&str] = &["previous", "prev", "pichla", "pichli"];
    const FILLER: &[&str] = &[
        "play", "the", "a", "an", "to", "this", "one", "video", "vedio", "song", "gana", "gaan",
        "track", "music", "please", "now", "youtube", "spotify", "on", "in", "koro", "karo",
        "kardo", "kar", "do", "me", "ta",
    ];
    let words: Vec<&str> = lnorm.split_whitespace().collect();
    let has_next = words.iter().any(|w| NEXT.contains(w));
    let has_prev = words.iter().any(|w| PREV.contains(w));
    if has_next == has_prev {
        return None;
    }
    let only_known = words
        .iter()
        .all(|w| NEXT.contains(w) || PREV.contains(w) || FILLER.contains(w));
    if !only_known {
        return None;
    }
    Some(has_next)
}

pub(crate) fn normalize_command(raw_cmd: &str) -> String {
    let line = raw_cmd.trim();
    if line.is_empty() {
        return String::new();
    }

    let mut lnorm = norm(line);

    // Strip embedded/trailing wake phrases and common ASR typos.
    for (pat, repl) in [
        ("hey google", " "),
        ("ok google", " "),
        ("okay google", " "),
        ("okey google", " "),
        ("hey inex", " "),
        ("ok inex", " "),
        ("hey aura", " "),
        ("ok aura", " "),
        ("okay aura", " "),
        ("okey aura", " "),
        ("hey lenex", " "),
        ("ok lenex", " "),
        ("hey linux", " "),
        ("ok linux", " "),
        ("hey jarvis", " "),
        ("ok jarvis", " "),
        ("hey alexa", " "),
        ("ok alexa", " "),
        ("hey pc", " "),
        ("ok pc", " "),
        ("hey p c", " "),
        ("ok p c", " "),
        ("hey computer", " "),
        ("lenex", "inex"),
        ("index", "inex"),
        ("spotyfi", "spotify"),
        ("spottify", "spotify"),
        ("spotfy", "spotify"),
        ("discards", "discord"),
        ("discard", "discord"),
        ("discort", "discord"),
        ("puse", "pause"),
        ("prevuse", "previous"),
        ("privious", "previous"),
    ] {
        lnorm = lnorm.replace(pat, repl);
    }

    let mut words: Vec<&str> = lnorm.split_whitespace().collect();
    if let Some(&first) = words.first() {
        if first == "okey" || first == "okay" || first == "ok" {
            words.remove(0);
            lnorm = words.join(" ");
        }
    }

    if lnorm.starts_with("plato ") {
        lnorm = format!("play to{}", &lnorm[5..]);
    } else if lnorm.starts_with("ple ") {
        lnorm = format!("play{}", &lnorm[3..]);
    } else if lnorm.starts_with("played ") {
        lnorm = format!("play{}", &lnorm[6..]);
    }

    for (from, to) in [
        ("minimises", "minimize"),
        ("minimised", "minimize"),
        ("minimising", "minimize"),
        ("minimise", "minimize"),
        ("minimizing", "minimize"),
        ("maximises", "maximize"),
        ("maximised", "maximize"),
        ("maximising", "maximize"),
        ("maximise", "maximize"),
        ("maximizing", "maximize"),
        ("focuses", "focus"),
        ("focused", "focus"),
        ("closes", "close"),
        ("closed", "close"),
    ] {
        if lnorm.contains(from) {
            lnorm = lnorm.replace(from, to);
        }
    }

    lnorm.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn parse_word_number(w: &str) -> Option<u32> {
    if let Ok(n) = w.parse::<u32>() {
        return Some(n);
    }
    match w {
        "a" | "an" | "one" | "ek" => Some(1),
        "two" | "dui" | "do" => Some(2),
        "three" | "teen" | "tin" => Some(3),
        "four" | "char" => Some(4),
        "five" | "panch" => Some(5),
        "six" | "chhoy" | "chhe" => Some(6),
        "seven" | "sat" | "saat" => Some(7),
        "eight" | "aat" | "aath" => Some(8),
        "nine" | "noy" | "nau" => Some(9),
        "ten" | "das" | "dosh" => Some(10),
        "twelve" | "baro" => Some(12),
        "fifteen" | "ponero" | "pandrah" => Some(15),
        "twenty" | "kuri" | "bees" => Some(20),
        "twenty-five" | "pocheesh" => Some(25),
        "thirty" | "tirish" | "tees" => Some(30),
        "forty" | "challish" | "chalis" => Some(40),
        "forty-five" => Some(45),
        "fifty" | "ponchash" | "pachas" => Some(50),
        "sixty" => Some(60),
        _ => None,
    }
}

fn parse_compact_duration_token(w: &str) -> Option<u32> {
    let digits_len = w.bytes().take_while(|b| b.is_ascii_digit()).count();
    if digits_len == 0 || digits_len == w.len() {
        return None;
    }
    let n: u32 = w[..digits_len].parse().ok()?;
    let unit = &w[digits_len..];
    if unit == "h" || unit == "hr" || unit == "hrs" || unit.starts_with("hour") {
        Some(n.saturating_mul(3600))
    } else if unit == "m" || unit == "min" || unit == "mins" || unit.starts_with("minute") {
        Some(n.saturating_mul(60))
    } else if unit == "s" || unit == "sec" || unit == "secs" || unit.starts_with("second") {
        Some(n)
    } else {
        None
    }
}

fn extract_duration_seconds(l: &str) -> (u32, bool, Option<u32>) {
    let words: Vec<&str> = l.split_whitespace().collect();
    let mut total_secs: u32 = 0;
    let mut parsed_any = false;
    let mut standalone_num: Option<u32> = None;

    for (i, &w) in words.iter().enumerate() {
        if let Some(secs) = parse_compact_duration_token(w) {
            total_secs = total_secs.saturating_add(secs);
            parsed_any = true;
            continue;
        }
        if let Some(n) = parse_word_number(w) {
            if let Some(&next) = words.get(i + 1) {
                if next.starts_with("hour") || next == "hr" || next == "hrs" || next == "ghanta" {
                    total_secs = total_secs.saturating_add(n.saturating_mul(3600));
                    parsed_any = true;
                    continue;
                } else if next.starts_with("min") || next == "m" {
                    total_secs = total_secs.saturating_add(n.saturating_mul(60));
                    parsed_any = true;
                    continue;
                } else if next.starts_with("sec") || next == "s" {
                    total_secs = total_secs.saturating_add(n);
                    parsed_any = true;
                    continue;
                }
            }
            // Don't treat "a" in "set a timer" as 1 minute unless paired with a unit above.
            if w != "a" && w != "an" && standalone_num.is_none() {
                standalone_num = Some(n);
            }
        }
    }

    (total_secs, parsed_any, standalone_num)
}

/// Parses a normalized voice/companion command into timer seconds when it is a timer trigger.
pub(crate) fn parse_timer_seconds(l: &str) -> Option<u32> {
    if !l.contains("timer")
        || l.contains("shutdown")
        || l.contains("shut down")
        || l.contains("turn off")
        || l.contains("restart")
        || l.contains("reboot")
    {
        return None;
    }

    let is_intent = l.contains("set")
        || l.contains("start")
        || l.contains("on")
        || l.contains("create")
        || l.contains("lagao")
        || l.contains("karo")
        || l.contains("koro")
        || l.contains("chalu")
        || l.starts_with("timer")
        || l.ends_with("timer")
        || l == "the timer"
        || l == "timer";

    if !is_intent {
        return None;
    }

    let (mut total_secs, parsed_any, standalone_num) = extract_duration_seconds(l);
    if !parsed_any {
        if let Some(n) = standalone_num {
            total_secs = n.saturating_mul(60);
        }
    }

    Some(if total_secs == 0 { 300 } else { total_secs })
}

/// Parses timed PC power commands like `"shutdown in 10 minutes"` or `"restart after 5s"` -> `(reboot, secs)`.
pub(crate) fn parse_power_timer(l: &str) -> Option<(bool, u32)> {
    if l.contains("cancel") || l.contains("abort") || l.contains("stop") {
        return None;
    }
    let is_reboot = l.contains("restart") || l.contains("reboot");
    let is_shutdown = l.contains("shutdown") || l.contains("shut down") || l.contains("turn off pc");
    if !is_reboot && !is_shutdown {
        return None;
    }

    let (total_secs, parsed_any, standalone_num) = extract_duration_seconds(l);
    let secs = if parsed_any {
        total_secs.clamp(1, 604_800)
    } else if l.contains("timer") || l.contains(" in ") || l.contains(" after ") {
        standalone_num.map(|n| n.clamp(1, 604_800)).unwrap_or(5)
    } else {
        return None;
    };
    Some((is_reboot, secs))
}

/// Returns true when the raw command targets Inex Island's native timer HUD.
pub fn is_timer_command(raw_cmd: &str) -> bool {
    let lnorm = normalize_command(raw_cmd);
    parse_timer_seconds(&lnorm).is_some()
}

pub(crate) fn handle_timer_command(l: &str) -> Option<bool> {
    let total_secs = parse_timer_seconds(l)?;
    println!("⏱️ Timer requested for Inex Island: {}s", total_secs);
    crate::island_bridge::notify_timer(total_secs);
    Some(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timer_commands_parse_cleanly() {
        assert_eq!(parse_timer_seconds(&normalize_command("set the timer")), Some(300));
        assert_eq!(parse_timer_seconds(&normalize_command("set da timer")), Some(300));
        assert_eq!(parse_timer_seconds(&normalize_command("set a timer")), Some(300));
        assert_eq!(parse_timer_seconds(&normalize_command("set a timer for 10 minutes")), Some(600));
        assert_eq!(parse_timer_seconds(&normalize_command("timer 30 seconds")), Some(30));
        assert_eq!(parse_timer_seconds(&normalize_command("set timer 5m")), Some(300));
        assert_eq!(parse_timer_seconds(&normalize_command("set 2 minute timer")), Some(120));
        assert_eq!(parse_timer_seconds(&normalize_command("timer 1 hour")), Some(3600));
        assert!(is_timer_command("Set the timer"));
        assert!(!is_timer_command("volume up"));
        assert!(!is_timer_command("shutdown timer 10 minutes"));

        assert_eq!(parse_power_timer(&normalize_command("shutdown in 10 seconds")), Some((false, 10)));
        assert_eq!(parse_power_timer(&normalize_command("shutdown pc after 30 minutes")), Some((false, 1800)));
        assert_eq!(parse_power_timer(&normalize_command("restart in 5m")), Some((true, 300)));
        assert_eq!(parse_power_timer(&normalize_command("shutdown in 1 second")), Some((false, 1)));
        assert_eq!(parse_power_timer(&normalize_command("cancel shutdown in 5 seconds")), None);
    }
}
