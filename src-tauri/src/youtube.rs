// youtube.rs - all YouTube logic lives here (moved out of main.rs, code unchanged except
// `pub(crate)` + new `yt_skip` / `is_yt_target` for next / previous).
use super::*;

/// Media-session name list for YouTube (browser/PWA windows). Used by media_native.
pub(crate) const YT_SESSION: &str = "chrome|msedge|brave|youtube";

// ---------------------------------------------------------------------------
// In-window control of the ALREADY OPEN YouTube app window (Windows UI Automation).
// No new window is ever opened by these. Actions: next | prev | pick (N-th video link on the
// current page) | search (type in YouTube search box) | play (search + open first result).
// ---------------------------------------------------------------------------
const YT_UI_SCRIPT: &str = r#"
$ErrorActionPreference = 'SilentlyContinue'
Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes
Add-Type -AssemblyName System.Windows.Forms
$AE = [System.Windows.Automation.AutomationElement]
$TS = [System.Windows.Automation.TreeScope]
$btnT = [System.Windows.Automation.ControlType]::Button
$linkT = [System.Windows.Automation.ControlType]::Hyperlink
$editT = [System.Windows.Automation.ControlType]::Edit
$comboT = [System.Windows.Automation.ControlType]::ComboBox
$act = '__ACTION__'
$arg = '__ARG__'

$win = $null
$wins = $AE::RootElement.FindAll($TS::Children, [System.Windows.Automation.Condition]::TrueCondition)
foreach ($w in $wins) {
  $n = $w.Current.Name
  if ($w.Current.ClassName -ne 'Chrome_WidgetWin_1') { continue }
  if ($n -notlike '*YouTube*' -or $n -like '*YouTube Music*') { continue }
  if ($n -match 'Google Chrome|Brave|Microsoft Edge|Opera|Vivaldi|Firefox') { continue }
  $win = $w
  break
}
if (-not $win) { Write-Output 'no-window'; exit 4 }

function Find-Named($type, $pattern) {
  $cond = New-Object System.Windows.Automation.PropertyCondition($AE::ControlTypeProperty, $type)
  foreach ($e in $win.FindAll($TS::Descendants, $cond)) {
    if ($e.Current.Name -like $pattern) { return $e }
  }
  return $null
}
function Invoke-El($e) {
  $e.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
}
function Get-VideoLinks {
  $cond = New-Object System.Windows.Automation.PropertyCondition($AE::ControlTypeProperty, $linkT)
  $out = @()
  foreach ($e in $win.FindAll($TS::Descendants, $cond)) {
    $n = $e.Current.Name
    if ($n -and ($n -match '\d+\s+(hours?|minutes?|seconds?)') -and ($n -notmatch 'Short')) { $out += $e }
  }
  return $out
}

function Find-Ctl($pattern) {
  # YouTube's Next is a <a> link, Previous/Play/Pause are <button>s: look at both kinds
  foreach ($t in @($btnT, $linkT)) {
    $e = Find-Named $t $pattern
    if ($e) { return $e }
  }
  return $null
}

if ($act -eq 'next' -or $act -eq 'prev' -or $act -eq 'pausevid' -or $act -eq 'resumevid') {
  $p = 'Next (*'
  if ($act -eq 'prev') { $p = 'Previous (*' }
  if ($act -eq 'pausevid') { $p = 'Pause (*' }
  if ($act -eq 'resumevid') { $p = 'Play (*' }
  $b = Find-Ctl $p
  if (-not $b) { Write-Output 'no-button'; exit 3 }
  try { Invoke-El $b; exit 0 } catch { Write-Output 'invoke-failed'; exit 3 }
}

if ($act -eq 'pick') {
  $idx = [int]$arg - 1
  $links = @(Get-VideoLinks)
  if ($links.Count -le $idx) { Write-Output 'no-links'; exit 3 }
  try { Invoke-El $links[$idx]; exit 0 } catch { Write-Output 'invoke-failed'; exit 3 }
}

if ($act -eq 'search' -or $act -eq 'play') {
  $edit = Find-Named $editT 'Search*'
  if (-not $edit) { $edit = Find-Named $comboT 'Search*' }
  if (-not $edit) { Write-Output 'no-search-box'; exit 3 }
  try { $win.SetFocus() } catch {}
  Start-Sleep -Milliseconds 200
  try { $edit.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue($arg) } catch { Write-Output 'setvalue-failed'; exit 3 }
  try { $edit.SetFocus() } catch {}
  Start-Sleep -Milliseconds 200
  [System.Windows.Forms.SendKeys]::SendWait('{ENTER}')
  if ($act -eq 'search') { exit 0 }
  Start-Sleep -Milliseconds 2500
  $t0 = Get-Date
  while (((Get-Date) - $t0).TotalSeconds -lt 8) {
    $links = @(Get-VideoLinks)
    if ($links.Count -gt 0) { try { Invoke-El $links[0]; exit 0 } catch {} }
    Start-Sleep -Milliseconds 400
  }
  Write-Output 'no-results'
  exit 3
}
if ($act -eq 'back') {
  try { $win.SetFocus() } catch {}
  Start-Sleep -Milliseconds 200
  [System.Windows.Forms.SendKeys]::SendWait('%{LEFT}')
  Start-Sleep -Milliseconds 1500
  $pl = Find-Ctl 'Pause (*'
  if (-not $pl) { $pl = Find-Ctl 'Play (*' }
  if (-not $pl) {
    # landed on a page without a player (search results): go back once more
    [System.Windows.Forms.SendKeys]::SendWait('%{LEFT}')
    Start-Sleep -Milliseconds 800
  }
  exit 0
}
Write-Output 'bad-action'
exit 3
"#;

/// runs the script; returns its exit code (0 ok, 4 = no YouTube app window, 3 = element not found, -1 = powershell failed)
fn yt_ui_run(action: &str, arg: &str) -> i32 {
    let script = YT_UI_SCRIPT
        .replace("__ACTION__", action)
        .replace("__ARG__", &arg.replace('\'', "''"));
    match Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
    {
        Ok(out) => {
            let code = out.status.code().unwrap_or(-1);
            if code != 0 {
                let msg = String::from_utf8_lossy(&out.stdout).trim().to_string();
                println!("⚠️ YouTube window action '{action}' -> {msg} (code {code})");
            }
            code
        }
        Err(e) => {
            println!("⚠️ powershell chalate parlam na: {e}");
            -1
        }
    }
}

fn yt_pwa_hwnd() -> Option<HWND> {
    list_windows()
        .iter()
        .find(|w| {
            let t = w.title.to_lowercase();
            t.contains("youtube")
                && !t.contains("youtube music")
                && !["google chrome", "brave", "microsoft edge", "opera", "vivaldi", "firefox"]
                    .iter()
                    .any(|b| t.contains(b))
        })
        .map(|w| w.hwnd)
}

/// next | prev | pausevid | resumevid : press the player button in the open YouTube app window.
/// A minimized window has no usable page, so if it fails we un-minimize WITHOUT stealing focus,
/// retry, and minimize again.
pub(crate) fn yt_ui_control(action: &str) -> bool {
    let rc = yt_ui_run(action, "");
    if rc == 0 {
        return true;
    }
    if rc == 3 {
        if let Some(hwnd) = yt_pwa_hwnd() {
            if unsafe { IsIconic(hwnd) } != 0 {
                unsafe {
                    ShowWindow(hwnd, SW_SHOWNOACTIVATE);
                }
                std::thread::sleep(Duration::from_millis(600));
                let rc2 = yt_ui_run(action, "");
                unsafe {
                    ShowWindow(hwnd, SW_MINIMIZE);
                }
                return rc2 == 0;
            }
        }
    }
    false
}

/// Pause / resume YouTube: player button first, then the Windows media session.
pub(crate) fn yt_pause_resume(pause: bool) -> bool {
    if yt_ui_control(if pause { "pausevid" } else { "resumevid" }) {
        return true;
    }
    let action = if pause {
        media_native::MediaAction::Pause
    } else {
        media_native::MediaAction::Play
    };
    media_native::control_media(action, Some(YT_SESSION))
}

/// Does `action` inside the YouTube app window that is ALREADY open.
/// Returns true  = a YouTube app window exists and the action was tried (never opens a new window),
///         false = no YouTube app window open (caller may open it normally).
pub(crate) fn yt_in_window(action: &str, arg: &str) -> bool {
    let need_focus = action != "next" && action != "prev";

    if need_focus {
        let wins = list_windows();
        let pwa = wins.iter().find(|w| {
            let t = w.title.to_lowercase();
            t.contains("youtube")
                && !t.contains("youtube music")
                && !["google chrome", "brave", "microsoft edge", "opera", "vivaldi", "firefox"]
                    .iter()
                    .any(|b| t.contains(b))
        });
        match pwa {
            Some(w) => unsafe {
                if IsIconic(w.hwnd) != 0 {
                    ShowWindow(w.hwnd, SW_RESTORE);
                }
                SetForegroundWindow(w.hwnd);
            },
            None => return false,
        }
        std::thread::sleep(Duration::from_millis(250));
    }

    let rc = yt_ui_run(action, arg);

    rc != 4 && rc != -1
}

/// Next / Previous for YouTube.
/// 1) native Windows media session (no focus, no mouse)
/// 2) if YouTube did not accept it: hotkey (Shift+N / Shift+P) inside the SAME already-open
///    YouTube window. Never opens a new window.
pub(crate) fn yt_skip(next: bool) -> bool {
    // 0) press the Next/Previous button inside the open YouTube app window (most reliable)
    if yt_ui_control(if next { "next" } else { "prev" }) {
        return true;
    }

    // Previous video, no playlist (YouTube has no Previous button then): go BACK in this window's history
    if !next && yt_in_window("back", "") {
        return true;
    }

    let action = if next { media_native::MediaAction::Next } else { media_native::MediaAction::Previous };

    if media_native::control_media(action, Some(YT_SESSION)) {
        return true;
    }

    println!(
        "⚠️ YouTube media-session {} accept kore nai, open window-e hotkey pathacchi",
        if next { "next" } else { "previous" }
    );

    let wins = list_windows();
    let hwnd = match wins
        .iter()
        .find(|w| norm(&w.title).contains("youtube") || norm(&w.exe).contains("youtube"))
    {
        Some(w) => w.hwnd,
        None => {
            println!("⚠️ YouTube window pai nai (notun kore khulbo na)");
            return false;
        }
    };

    let prev_fg = unsafe { GetForegroundWindow() } as usize;
    let was_min = unsafe { IsIconic(hwnd) != 0 };

    unsafe {
        if was_min {
            ShowWindow(hwnd, SW_RESTORE);
        }
        SetForegroundWindow(hwnd);
    }
    std::thread::sleep(Duration::from_millis(150));

    if next {
        send_shift_key(0x4E); // Shift+N = next video
    } else {
        send_shift_key(0x50); // Shift+P = previous video (only works when YouTube has a playlist)
    }

    std::thread::sleep(Duration::from_millis(100));

    // put everything back as it was
    unsafe {
        if was_min {
            ShowWindow(hwnd, SW_MINIMIZE);
        }
        if prev_fg != 0 && prev_fg != hwnd as usize {
            SetForegroundWindow(prev_fg as HWND);
        }
    }
    true
}

/// true when a "next/previous/pause" command should go to YouTube
/// (explicit target, or nothing named and only YouTube is playing).
pub(crate) fn is_yt_target(target: Option<&str>) -> bool {
    match target {
        Some(t) => t == YT_SESSION,
        None => media_native::is_playing(Some(YT_SESSION)) && !media_native::is_playing(Some(SP_SESSION)),
    }
}

// Fetch distinct video IDs from YouTube search results via pure HTTPS

// Appends sp=EgIQAQ%253D%253D (Type: Video) and filters out Shorts/Reels carousels

pub(crate) fn fetch_youtube_video_ids(query: &str) -> Vec<String> {
    let url = format!(
        "https://www.youtube.com/results?search_query={}&sp=EgIQAQ%253D%253D",
        url_encode(query)
    );

    let resp = match ureq::get(&url)
        .set(
            "User-Agent",
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36",
        )
        .timeout(Duration::from_secs(4))
        .call()
    {
        Ok(r) => r,

        Err(e) => {
            eprintln!("Could not reach YouTube: {e}");

            return Vec::new();
        }
    };

    let body = match resp.into_string() {
        Ok(b) => b,

        Err(_) => return Vec::new(),
    };

    let mut vids = Vec::new();

    let mut rest = body.as_str();

    while let Some(pos) = rest.find("/watch?v=") {
        let after = &rest[pos + 9..];

        if after.len() >= 11 {
            let id = &after[..11];

            if id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
            {
                let id_str = id.to_string();

                // Skip YouTube Shorts/Reels

                let prefix_chunk = if pos >= 120 {
                    &rest[pos - 120..pos]
                } else {
                    &rest[..pos]
                };

                let is_short = prefix_chunk.contains("shorts") || prefix_chunk.contains("reelItem");

                if !is_short && !vids.contains(&id_str) {
                    vids.push(id_str);

                    if vids.len() >= 10 {
                        break;
                    }
                }
            }
        }

        rest = &rest[pos + 9..];
    }

    vids
}

// Helper: extract Chrome profile for an installed PWA from its AppID (e.g. "Profile 4")
pub(crate) fn get_chrome_profile_for_app(app_id: &str) -> Option<String> {
    if let Some(pos) = app_id.rfind(".Profile") {
        let suffix = &app_id[pos + 8..];
        Some(format!("Profile {}", suffix))
    } else if app_id.ends_with(".Default") {
        Some("Default".to_string())
    } else {
        None
    }
}

/// Reads the installed YouTube app shortcut (.lnk) and returns (exe, [--profile-directory=.., --app-id=..]).
/// This is the REAL profile of your installed YouTube app. (The old code tried to read the profile
/// from the shortcut PATH, which never contains it, so YouTube opened in the wrong Chrome profile.)
pub(crate) fn yt_pwa_launcher(apps: &[App]) -> Option<(String, Vec<String>)> {
    let yt = apps.iter().find(|a| a.name.eq_ignore_ascii_case("youtube"))?;
    if !yt.id.to_lowercase().ends_with(".lnk") {
        return None;
    }
    let script = format!(
        "$s=(New-Object -ComObject WScript.Shell).CreateShortcut('{}'); $s.TargetPath; $s.Arguments",
        yt.id.replace('\'', "''")
    );
    let out = Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&out.stdout).to_string();
    let mut lines = text.lines();
    let exe = lines.next()?.trim().to_string();
    let args_line = lines.next().unwrap_or("").to_string();
    if exe.is_empty() {
        return None;
    }
    // split args on spaces, but keep "quoted parts" together
    let mut parts: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut in_q = false;
    for c in args_line.chars() {
        match c {
            '"' => in_q = !in_q,
            ' ' if !in_q => {
                if !cur.is_empty() {
                    parts.push(std::mem::take(&mut cur));
                }
            }
            _ => cur.push(c),
        }
    }
    if !cur.is_empty() {
        parts.push(cur);
    }
    let keep: Vec<String> = parts
        .into_iter()
        .filter(|p| p.starts_with("--profile-directory=") || p.starts_with("--app-id="))
        .collect();
    if !keep.iter().any(|p| p.starts_with("--app-id=")) {
        return None;
    }
    println!("🎯 YouTube app: {exe} {}", keep.join(" "));
    Some((exe, keep))
}

// Helper: open YouTube in the dedicated YouTube PWA app window if installed
pub(crate) fn open_youtube_app(apps: &[App], url: Option<&str>) {
    let chrome_path = r"C:\Program Files\Google\Chrome\Application\chrome.exe";
    let chrome_exists = std::path::Path::new(chrome_path).exists();
    let yt_open = is_app_open("youtube");

    let yt_app = apps.iter().find(|a| a.name.eq_ignore_ascii_case("youtube"));

    match url {
        Some(target_url) => {
            println!("🚀 Opening in YouTube App: {target_url}");
            let pwa = yt_pwa_launcher(apps);
            let profile_arg = pwa
                .as_ref()
                .and_then(|(_, a)| a.iter().find(|x| x.starts_with("--profile-directory=")))
                .map(|x| x.trim_start_matches("--profile-directory=").to_string())
                .or_else(|| yt_app.and_then(|a| get_chrome_profile_for_app(&a.id)));
            if let Some((exe, base_args)) = pwa {
                // installed YouTube app itself (right profile). Best-effort: ask it to open this URL.
                let mut cmd = Command::new(&exe);
                cmd.args(&base_args);
                cmd.arg(format!("--app-launch-url-for-shortcuts-menu-item={target_url}"));
                cmd.creation_flags(CREATE_NO_WINDOW);
                let _ = cmd.spawn();
            } else if chrome_exists { let mut cmd = Command::new(&chrome_path);
                if let Some(ref prof) = profile_arg {
                    cmd.arg(format!("--profile-directory={prof}"));
                }
                cmd.arg(format!("--app={target_url}"));
                let _ = cmd.spawn();
            } else if let Some(yt) = yt_app {
                launch(yt);
            } else {
                let _ = Command::new("explorer.exe")
                    .arg(&target_url)
                    .spawn();
            }

            std::thread::sleep(Duration::from_millis(300));
            let wins = list_windows();
            if let Some(w) = wins.iter().find(|w| {
                norm(&w.title).contains("youtube") || norm(&w.exe).contains("youtube")
            }) {
                unsafe {
                    if IsIconic(w.hwnd) != 0 {
                        ShowWindow(w.hwnd, SW_RESTORE);
                    } else {
                        ShowWindow(w.hwnd, SW_SHOW);
                    }
                    SetForegroundWindow(w.hwnd);
                }
            }
        }
        None => {
            if yt_open {
                println!("🪟 YouTube already running: Activating window...");
                let wins = list_windows();
                if let Some(w) = wins.iter().find(|w| {
                    norm(&w.title).contains("youtube") || norm(&w.exe).contains("youtube")
                }) {
                    unsafe {
                        if IsIconic(w.hwnd) != 0 {
                            ShowWindow(w.hwnd, SW_RESTORE);
                        } else {
                            ShowWindow(w.hwnd, SW_SHOW);
                        }
                        SetForegroundWindow(w.hwnd);
                    }
                }
                return;
            }

            if let Some(yt) = yt_app {
                println!("🚀 Launching installed YouTube App: {}", yt.name);
                launch_app_smart(yt);
            } else if chrome_exists {
                println!("🚀 Launching YouTube App via Chrome...");
                let profile_arg = yt_app.and_then(|a| get_chrome_profile_for_app(&a.id));
                let mut cmd = Command::new(&chrome_path);
                if let Some(ref prof) = profile_arg {
                    cmd.arg(format!("--profile-directory={prof}"));
                }
                cmd.arg("--app=https://www.youtube.com");
                let _ = cmd.spawn();
            } else {
                println!("🌐 Opening YouTube: https://www.youtube.com");
                let _ = Command::new("explorer.exe")
                    .arg("https://www.youtube.com")
                    .spawn();
            }
        }
    }
}

// Open YouTube (just opens the app, does NOT play random music)
pub(crate) fn open_youtube(apps: &[App]) {
    println!("🚀 Opening YouTube App...");
    open_youtube_app(apps, None);
}

// Play YouTube: matches top song and starts playing directly (like phone Google Assistant)
// If empty or vague ("music", "song"), plays trending music
pub(crate) fn play_youtube(apps: &[App], query: Option<&str>) {
    let query_str = query.unwrap_or("").trim();

    let is_vague = query_str.is_empty()
        || query_str == "music"
        || query_str == "song"
        || query_str == "a song"
        || query_str == "something"
        || query_str == "gana"
        || query_str == "on";

    let search_term = if is_vague {
        println!("🎵 No specific song named - playing top trending official music video (no reels/shorts)...");
        "top trending songs official music video"
    } else {
        query_str
    };

    // already-open YouTube app window: search + play first result INSIDE it (no new window)
    if yt_in_window("play", search_term) {
        return;
    }
    println!(
        "🔍 Finding best match on YouTube for: \"{}\"...",
        search_term
    );

    let vids = fetch_youtube_video_ids(search_term);

    if let Some(first) = vids.first() {
        let url = format!("https://www.youtube.com/watch?v={first}");
        println!("▶️ Auto-playing YouTube match: {url}");
        open_youtube_app(apps, Some(&url));

        if let Ok(mut lock) = LAST_YOUTUBE_VIDEOS.lock() {
            *lock = vids;
        }
        return;
    }

    // Fallback: if video ID extraction failed, open search results
    let url = format!(
        "https://www.youtube.com/results?search_query={}",
        url_encode(search_term)
    );
    open_youtube_app(apps, Some(&url));
}

// Search YouTube: opens the search results page and caches video IDs for "play first video" / "number 2"
pub(crate) fn search_youtube(apps: &[App], query: &str) {
    let url = format!(
        "https://www.youtube.com/results?search_query={}&sp=EgIQAQ%253D%253D",
        url_encode(query)
    );
    println!("🔍 Searching YouTube: \"{}\"", query);
    if yt_in_window("search", query) {
        return;
    }
    open_youtube_app(apps, Some(&url));

    let q = query.to_string();
    std::thread::spawn(move || {
        let vids = fetch_youtube_video_ids(&q);
        if let Ok(mut lock) = LAST_YOUTUBE_VIDEOS.lock() {
            *lock = vids;
        }
    });
}
