use crate::audio_native;
use crate::wake_word::norm;

pub fn set_volume_percent(percent: u32) {
    audio_native::master(audio_native::Change::Set(percent.clamp(0, 100)));
}

pub fn set_app_volume(app: &str, action: &str) -> bool {
    use audio_native::Change;
    let targets: Vec<&str> = if app == "youtube" {
        vec!["chrome", "msedge"]
    } else {
        vec![app]
    };
    let ch = if action == "mute" || action == "unmute" {
        Change::ToggleMute
    } else if let Some(d) = action.strip_prefix('+') {
        Change::Delta(d.parse().unwrap_or(10))
    } else if let Some(d) = action.strip_prefix('-') {
        Change::Delta(-(d.parse::<i32>().unwrap_or(10)))
    } else if let Ok(v) = action.parse::<u32>() {
        Change::Set(v.min(100))
    } else {
        return false;
    };
    let ok = audio_native::app(&targets, ch);
    if !ok {
        println!("⚠️ {app}: audio session nai (kichu baja-che na?)");
    }
    ok
}

pub fn parse_volume_percent(text: &str) -> Option<u32> {
    let lower = text.to_lowercase();

    let is_explicit_volume =
        lower.contains("volume") || lower.contains("sound") || lower.contains("awaaz");

    let has_percent = lower.contains('%') || lower.contains("percent");

    let words: Vec<&str> = lower.split_whitespace().collect();

    if !is_explicit_volume && !has_percent {
        // If just a single word like "100%" or "50%" or "80"

        if words.len() == 1 {
            let clean: String = words[0].chars().filter(|c| c.is_ascii_digit()).collect();

            if let Ok(val) = clean.parse::<u32>() {
                if val <= 100 && (val % 5 == 0 || val == 100) {
                    return Some(val);
                }
            }
        }

        return None;
    }

    // Check word numbers

    for (i, &w) in words.iter().enumerate() {
        let val = match w {
            "zero" => Some(0),

            "ten" => Some(10),

            "fifteen" => Some(15),

            "twenty" => Some(20),

            "twenty-five" | "twentyfive" => Some(25),

            "thirty" => Some(30),

            "thirty-five" | "thirtyfive" => Some(35),

            "forty" => Some(40),

            "forty-five" | "fortyfive" => Some(45),

            "fifty" => Some(50),

            "fifty-five" | "fiftyfive" => Some(55),

            "sixty" => Some(60),

            "sixty-five" | "sixtyfive" => Some(65),

            "seventy" => Some(70),

            "seventy-five" | "seventyfive" => Some(75),

            "eighty" => Some(80),

            "eighty-five" | "eightyfive" => Some(85),

            "ninety" => Some(90),

            "ninety-five" | "ninetyfive" => Some(95),

            "hundred" => Some(100),

            _ => None,
        };

        if let Some(v) = val {
            if i > 0 && words[i - 1] == "one" && v == 100 {
                return Some(100);
            }

            return Some(v);
        }
    }

    // Check digit numbers: "50", "50%", "100%"

    for word in &words {
        let clean: String = word.chars().filter(|c| c.is_ascii_digit()).collect();

        if !clean.is_empty() {
            if let Ok(val) = clean.parse::<u32>() {
                if val <= 100 {
                    return Some(val);
                }
            }
        }
    }

    None
}

// Unified handler for app-specific and master system volume

pub fn handle_volume_command(line: &str) -> bool {
    let lnorm = norm(line);

    // 1. Check if a specific target app is mentioned

    let app_target = if lnorm.contains("spotify") {
        Some("spotify")
    } else if lnorm.contains("youtube") {
        Some("youtube")
    } else if lnorm.contains("chrome") {
        Some("chrome")
    } else if lnorm.contains("discord") {
        Some("discord")
    } else {
        None
    };

    if let Some(app) = app_target {
        // App Mute

        if lnorm.contains("mute") || lnorm.contains("chup") || lnorm.contains("silent") {
            println!("🔇 {app}: Mute toggle");

            return set_app_volume(app, "mute");
        }

        // App Volume Up: "spotify volume up", "spotify sound barao", "spotify er sound barao"

        if lnorm.contains("volume up")
            || lnorm.contains("sound up")
            || lnorm.contains("sound barao")
            || lnorm.contains("er sound barao")
            || lnorm.contains("awaaz badhao")
            || lnorm.contains("louder")
        {
            println!("🔊 {app} Volume: +15%");

            return set_app_volume(app, "+15");
        }

        // App Volume Down: "spotify volume down", "spotify sound kom koro", "spotify er sound kom koro", "sound kam koro"

        if lnorm.contains("volume down")
            || lnorm.contains("sound down")
            || lnorm.contains("sound kom koro")
            || lnorm.contains("er sound kom koro")
            || lnorm.contains("sound kam koro")
            || lnorm.contains("awaaz kam karo")
            || lnorm.contains("quieter")
        {
            println!("🔉 {app} Volume: -15%");

            return set_app_volume(app, "-15");
        }

        // App Volume %: "spotify volume 50%", "spotify 80%"

        if let Some(pct) = parse_volume_percent(&lnorm) {
            println!("🔊 Setting {app} Volume to {}%", pct);

            return set_app_volume(app, &pct.to_string());
        }
    }

    // 2. Master PC Volume percentage

    if let Some(pct) = parse_volume_percent(&lnorm) {
        println!("🔊 Setting Master Volume to {}%", pct);

        set_volume_percent(pct);

        return true;
    }

    // 3. Master Volume Up

    if lnorm.contains("volume up")
        || lnorm.contains("sound up")
        || lnorm.contains("louder")
        || lnorm.contains("awaaz badhao")
        || lnorm.contains("sound barao")
    {
        println!("🔊 Volume: +10%");
        audio_native::master(audio_native::Change::Delta(10));
        return true;
    }

    // 4. Master Volume Down

    if lnorm.contains("volume down")
        || lnorm.contains("sound down")
        || lnorm.contains("quieter")
        || lnorm.contains("awaaz kam karo")
        || lnorm.contains("sound kom koro")
        || lnorm.contains("sound kam koro")
    {
        println!("🔉 Volume: -10%");
        audio_native::master(audio_native::Change::Delta(-10));
        return true;
    }

    // 5. Master Mute

    if lnorm.contains("mute") || lnorm.contains("unmute") || lnorm.contains("silence") {
        println!("🔇 Volume: Mute toggle");
        audio_native::master(audio_native::Change::ToggleMute);
        return true;
    }

    false
}
