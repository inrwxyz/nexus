//! Fuzzy voice-command intent parser for opening and closing Circle to Search.

fn circle_like_word(w: &str) -> bool {
    matches!(
        w,
        "circle"
            | "cercle"
            | "sircle"
            | "sercla"
            | "sercal"
            | "sercle"
            | "sarcle"
            | "sarkal"
            | "sarcal"
            | "sakal"
            | "circul"
            | "circol"
            | "cycle"
            | "circus"
    ) || (w.len() >= 4 && strsim::jaro_winkler(w, "circle") >= 0.74)
}

fn search_like_word(w: &str) -> bool {
    matches!(w, "search" | "serch" | "sarach" | "seach" | "such" | "serchh")
        || (w.len() >= 4 && strsim::jaro_winkler(w, "search") >= 0.78)
}

/// Maps a normalized voice command phrase into `Some(true)` (show overlay), `Some(false)` (close overlay), or `None`.
pub fn parse_voice_overlay_command(lnorm: &str, overlay_open: bool) -> Option<bool> {
    let q = lnorm.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase();
    if q.is_empty() {
        return None;
    }

    const CS_PHRASES: &[&str] = &[
        "circle to search",
        "circle 2 search",
        "circle the search",
        "circle too search",
        "circle for search",
        "circle in search",
        "circle of search",
        "circle search",
        "circle to such",
        "cercle to search",
        "cercle search",
        "sarcle to search",
        "sarcle search",
        "sarkal to search",
        "sarkal search",
        "sercla to search",
        "sercla to serch",
        "sercal to search",
        "sakal to search",
        "sircle to search",
        "sircle search",
        "cycle to search",
        "cycle search",
        "circul to search",
        "circus to search",
    ];
    let mut has_cs = CS_PHRASES.iter().any(|p| q.contains(p));

    if !has_cs {
        let words: Vec<&str> = q.split_whitespace().collect();
        for i in 0..words.len() {
            if circle_like_word(words[i]) {
                let adj1 = i + 1 < words.len() && search_like_word(words[i + 1]);
                let adj2 = i + 2 < words.len()
                    && matches!(
                        words[i + 1],
                        "to" | "2" | "two" | "too" | "do" | "the" | "for" | "of" | "a" | "in" | "ta" | "er"
                    )
                    && search_like_word(words[i + 2]);
                if adj1 || adj2 {
                    has_cs = true;
                    break;
                }
            }
        }
    }

    if has_cs {
        let padded = format!(" {q} ");
        const OFF_TOKENS: &[&str] = &[
            " off ",
            " close ",
            " stop ",
            " hide ",
            " exit ",
            " quit ",
            " cancel ",
            " disable ",
            " bondho ",
            " bandh ",
            " band ",
            " kete ",
            " kato ",
        ];
        return Some(!OFF_TOKENS.iter().any(|tok| padded.contains(tok)));
    }

    if overlay_open
        && matches!(
            q.as_str(),
            "off" | "turn off" | "close" | "close it" | "exit" | "cancel" | "stop" | "bondho koro" | "band karo"
        )
    {
        return Some(false);
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_circle_search_voice_on_and_off_commands() {
        assert_eq!(parse_voice_overlay_command("on circle to search", false), Some(true));
        assert_eq!(parse_voice_overlay_command("on sercla to serch", false), Some(true));
        assert_eq!(parse_voice_overlay_command("on cycle to search", false), Some(true));
        assert_eq!(parse_voice_overlay_command("turn on circle to search", false), Some(true));
        assert_eq!(parse_voice_overlay_command("circle to search on", false), Some(true));
        assert_eq!(parse_voice_overlay_command("open circle to search", false), Some(true));
        assert_eq!(parse_voice_overlay_command("circle 2 search", false), Some(true));
        assert_eq!(parse_voice_overlay_command("cercle to search", false), Some(true));

        assert_eq!(parse_voice_overlay_command("off circle to search", false), Some(false));
        assert_eq!(parse_voice_overlay_command("off sakal to search", false), Some(false));
        assert_eq!(parse_voice_overlay_command("circle to search off", false), Some(false));
        assert_eq!(parse_voice_overlay_command("close circle to search", false), Some(false));
        assert_eq!(parse_voice_overlay_command("stop circle search", false), Some(false));
        assert_eq!(parse_voice_overlay_command("circle to search bondho koro", false), Some(false));

        assert_eq!(parse_voice_overlay_command("off", true), Some(false));
        assert_eq!(parse_voice_overlay_command("close", true), Some(false));
        assert_eq!(parse_voice_overlay_command("off", false), None);
        assert_eq!(parse_voice_overlay_command("search on youtube cat videos", false), None);
        assert_eq!(parse_voice_overlay_command("search cycle price", false), None);
        assert_eq!(parse_voice_overlay_command("open chrome", false), None);
    }
}
