use std::io::{self, Write};
use std::os::windows::process::CommandExt;
use std::path::PathBuf;
use std::process::Command;
use crate::wake_word::{norm, score};
use crate::voice;


#[derive(Clone, Debug)]
pub struct FoundItem {
    pub path: PathBuf,
    pub is_dir: bool,
}

pub fn search_files_and_folders(query: &str, folder_only: bool) -> Vec<FoundItem> {
    let q = norm(query);

    if q.is_empty() {
        return Vec::new();
    }

    let mut search_roots: Vec<std::path::PathBuf> = vec![
        std::path::PathBuf::from(r"E:\"),
        std::path::PathBuf::from(r"D:\"),
    ];
    if let Ok(profile) = std::env::var("USERPROFILE") {
        let home = std::path::PathBuf::from(profile);
        search_roots.push(home.join("Desktop"));
        search_roots.push(home.join("Documents"));
        search_roots.push(home.join("Downloads"));
        search_roots.push(home);
    }

    let mut results = Vec::new();

    for root_path in &search_roots {
        if !root_path.exists() {
            continue;
        }

        scan_dir(root_path, &q, folder_only, 0, 3, &mut results);

        if results.len() >= 15 {
            break;
        }
    }

    results.sort_by(|a, b| {
        let a_str = a.path.file_name().and_then(|s| s.to_str()).unwrap_or("");

        let b_str = b.path.file_name().and_then(|s| s.to_str()).unwrap_or("");

        let a_norm = norm(a_str);

        let b_norm = norm(b_str);

        let bonus_a = if a_norm == q {
            3.0
        } else if a_norm.starts_with(&q) {
            1.8
        } else if a_norm.contains(&format!(" {q}")) {
            1.4
        } else {
            0.0
        };

        let bonus_b = if b_norm == q {
            3.0
        } else if b_norm.starts_with(&q) {
            1.8
        } else if b_norm.contains(&format!(" {q}")) {
            1.4
        } else {
            0.0
        };

        let sa = score(&q, &a_norm) + bonus_a;

        let sb = score(&q, &b_norm) + bonus_b;

        sb.partial_cmp(&sa).unwrap_or(std::cmp::Ordering::Equal)
    });

    results.truncate(8);

    results
}

fn scan_dir(
    dir: &std::path::Path,

    query: &str,

    folder_only: bool,

    depth: usize,

    max_depth: usize,

    results: &mut Vec<FoundItem>,
) {
    if depth >= max_depth || results.len() >= 20 {
        return;
    }

    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,

        Err(_) => return,
    };

    for entry in entries.flatten() {
        let path = entry.path();

        let is_dir = path.is_dir();

        if let Some(name) = path.file_name().and_then(|s| s.to_str()) {
            if name.starts_with('.')
                || name == "node_modules"
                || name == "target"
                || name == "AppData"
                || name == "plugins"
                || name == "venv"
                || name.contains("IntelliJ")
                || name.contains("AndroidStudio")
                || name == "$RECYCLE.BIN"
                || name == "System Volume Information"
                || name == "Windows"
                || name == "Program Files"
                || name == "Program Files (x86)"
            {
                continue;
            }

            let name_norm = norm(name);

            let matched = if query.len() <= 3 {
                name_norm == query
                    || name_norm.starts_with(&format!("{query} "))
                    || name_norm.contains(&format!(" {query}"))
                    || name_norm.starts_with(query)
            } else {
                name_norm.contains(query) || score(query, &name_norm) >= 0.75
            };

            if matched {
                if !folder_only || is_dir {
                    if !results.iter().any(|r| r.path == path) {
                        results.push(FoundItem {
                            path: path.clone(),

                            is_dir,
                        });
                    }
                }
            }

            if is_dir && depth + 1 < max_depth {
                scan_dir(&path, query, folder_only, depth + 1, max_depth, results);
            }
        }
    }
}

pub fn handle_file_explorer(cmd: &str, voice_inst: Option<&voice::Voice>) -> bool {
    let norm_cmd = norm(cmd);

    // Never intercept CLOSE commands

    if norm_cmd.starts_with("close ")
        || norm_cmd.starts_with("band ")
        || norm_cmd.starts_with("quit ")
        || norm_cmd.starts_with("cut ")
        || norm_cmd.starts_with("kete ")
    {
        return false;
    }

    // Standard shell folders

    if norm_cmd == "open downloads" || norm_cmd == "downloads" || norm_cmd == "downloads folder" {
        println!("📁 Opening Downloads folder...");

        let _ = Command::new("explorer.exe").arg("shell:Downloads").spawn();

        return true;
    }

    if norm_cmd == "open documents" || norm_cmd == "documents" || norm_cmd == "documents folder" {
        println!("📁 Opening Documents folder...");

        let _ = Command::new("explorer.exe").arg("shell:Personal").spawn();

        return true;
    }

    if norm_cmd == "open desktop" || norm_cmd == "desktop" || norm_cmd == "desktop folder" {
        println!("📁 Opening Desktop folder...");

        let _ = Command::new("explorer.exe").arg("shell:Desktop").spawn();

        return true;
    }

    if norm_cmd == "open pictures" || norm_cmd == "pictures" || norm_cmd == "photos folder" {
        println!("📁 Opening Pictures folder...");

        let _ = Command::new("explorer.exe")
            .arg("shell:My Pictures")
            .spawn();

        return true;
    }

    if norm_cmd == "open videos" || norm_cmd == "videos folder" {
        println!("📁 Opening Videos folder...");

        let _ = Command::new("explorer.exe").arg("shell:My Video").spawn();

        return true;
    }

    if norm_cmd == "open c drive" || norm_cmd == "c drive" {
        println!("📁 Opening C: Drive...");

        let _ = Command::new("explorer.exe").arg(r"C:\").spawn();

        return true;
    }

    if norm_cmd == "open d drive" || norm_cmd == "d drive" {
        println!("📁 Opening D: Drive...");

        let _ = Command::new("explorer.exe").arg(r"D:\").spawn();

        return true;
    }

    if norm_cmd == "open e drive" || norm_cmd == "e drive" {
        println!("📁 Opening E: Drive...");

        let _ = Command::new("explorer.exe").arg(r"E:\").spawn();

        return true;
    }

    if norm_cmd == "open file explorer" || norm_cmd == "open explorer" || norm_cmd == "explorer" {
        println!("📁 Opening File Explorer...");

        let _ = Command::new("explorer.exe").spawn();

        return true;
    }

    // Custom folder / file search: ONLY match if explicit file/folder keywords are used!

    // Never intercept generic application commands (e.g. "open chrome") or web search!

    let is_folder_req = norm_cmd.contains("folder") || norm_cmd.contains("directory");

    let is_file_req = norm_cmd.contains("file") || norm_cmd.contains("document");

    let has_file_prefix = norm_cmd.starts_with("open folder ")
        || norm_cmd.starts_with("find folder ")
        || norm_cmd.starts_with("search folder ")
        || norm_cmd.starts_with("folder ")
        || norm_cmd.starts_with("open file ")
        || norm_cmd.starts_with("find file ")
        || norm_cmd.starts_with("search file ")
        || norm_cmd.starts_with("file ")
        || ((norm_cmd.starts_with("find ") || norm_cmd.starts_with("search "))
            && (is_folder_req || is_file_req));

    if !has_file_prefix && !is_folder_req && !is_file_req {
        return false;
    }

    // Clean query

    let mut clean = norm_cmd;

    for prefix in &[
        "open folder ",
        "find folder ",
        "search folder ",
        "folder ",
        "open file ",
        "find file ",
        "search file ",
        "file ",
        "find ",
        "search ",
        "kholo ",
        "dikhau ",
    ] {
        if clean.starts_with(prefix) {
            clean = clean[prefix.len()..].to_string();

            break;
        }
    }

    for suffix in &[" folder", " file", " kholo", " karo", " koro", " dhoondo"] {
        if clean.ends_with(suffix) {
            clean = clean[..clean.len() - suffix.len()].to_string();

            break;
        }
    }

    let query = clean.trim();

    if query.is_empty()
        || query == "folder"
        || query == "file"
        || query == "youtube"
        || query == "spotify"
        || query == "chrome"
    {
        return false;
    }

    println!("🔍 Searching PC for: \"{}\"...", query);

    let items = search_files_and_folders(query, is_folder_req && !is_file_req);

    if items.is_empty() {
        println!("❌ No files or folders found matching \"{}\".", query);

        return false;
    }

    // If only 1 found: open immediately!

    if items.len() == 1 {
        let item = &items[0];

        let p_str = item.path.to_string_lossy();

        println!("📁 Found: {p_str}");

        if item.is_dir {
            let _ = Command::new("explorer.exe").arg(&*p_str).spawn();
        } else {
            let _ = Command::new("explorer.exe")
                .args(["/select,", &*p_str])
                .spawn();
        }

        return true;
    }

    // Multiple found: show list and let user pick via voice

    let count = items.len().min(5);

    println!("\nFound {count} matching items for \"{query}\":");

    for (i, it) in items.iter().take(count).enumerate() {
        let kind = if it.is_dir {
            "📁 [Folder]"
        } else {
            "📄 [File]"
        };

        println!("  {}) {} {}", i + 1, kind, it.path.to_string_lossy());
    }

    let mut choice_idx = None;

    if let Some(v) = voice_inst {
        println!("🎤 Say 'number 1', 'number 2', etc. (or press Enter for #1):");

        print!("\x07");

        io::stdout().flush().ok();

        v.reset_buffers();

        if let Ok(Some(heard)) = v.wait_for_utterance() {
            let h = norm(&heard.text);

            println!("🗣️  Choice: \"{}\"", heard.text);

            if h.contains("one") || h.contains("1") || h.contains("first") {
                choice_idx = Some(0);
            } else if h.contains("two") || h.contains("2") || h.contains("second") {
                if count >= 2 {
                    choice_idx = Some(1);
                }
            } else if h.contains("three") || h.contains("3") || h.contains("third") {
                if count >= 3 {
                    choice_idx = Some(2);
                }
            } else if h.contains("four") || h.contains("4") {
                if count >= 4 {
                    choice_idx = Some(3);
                }
            } else if h.contains("five") || h.contains("5") {
                if count >= 5 {
                    choice_idx = Some(4);
                }
            }
        }
    }

    let choice = choice_idx.unwrap_or(0);

    let chosen = &items[choice];

    let p_str = chosen.path.to_string_lossy();

    println!("📁 Opening: {p_str}");

    if chosen.is_dir {
        let _ = Command::new("explorer.exe").arg(&*p_str).spawn();
    } else {
        let _ = Command::new("explorer.exe")
            .args(["/select,", &*p_str])
            .spawn();
    }

    true
}

// ---------- vocab + aliases ----------
