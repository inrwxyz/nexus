// spotify.rs - all Spotify logic lives here (moved out of main.rs, code NOT changed,
// only `pub(crate)` added so main.rs can call it).
use super::*;

/// Media-session name list for Spotify (used by media_native).
pub(crate) const SP_SESSION: &str = "spotify";

/// true when a "next/previous/pause" command should go to Spotify
/// (explicit target, or nothing named and only Spotify is playing).
pub(crate) fn is_sp_target(target: Option<&str>) -> bool {
    match target {
        Some(t) => t == SP_SESSION,
        None => media_native::is_playing(Some(SP_SESSION)) && !media_native::is_playing(Some(YT_SESSION)),
    }
}

/// Spotify "previous": 1st press only restarts the current song, so press twice to really go back.
/// (moved here from media_native.rs, same behaviour)
pub(crate) fn sp_previous() -> bool {
    let first = media_native::control_media(media_native::MediaAction::Previous, Some(SP_SESSION));
    std::thread::sleep(Duration::from_millis(250));
    let second = media_native::control_media(media_native::MediaAction::Previous, Some(SP_SESSION));
    first || second
}

// Helper: check if Spotify has an open/visible window on desktop
pub(crate) fn is_spotify_window_open() -> bool {
    let wins = list_windows();
    if wins.iter().any(|w| {
        let exe = norm(&w.exe);
        let title = norm(&w.title);
        exe == "spotify" || exe.contains("spotify") || title.contains("spotify")
    }) {
        return true;
    }
    find_spotify_hwnd().is_some()
}

// Open Spotify app window natively without triggering playback
pub(crate) fn open_spotify_app() {
    println!("🚀 Opening Spotify App...");
    if let Some(hwnd) = find_spotify_hwnd() {
        unsafe {
            if IsIconic(hwnd) != 0 {
                ShowWindow(hwnd, SW_RESTORE);
            } else {
                ShowWindow(hwnd, SW_SHOW);
            }
            SetForegroundWindow(hwnd);
        }
        return;
    }
    let wins = list_windows();
    if let Some(w) = wins.iter().find(|w| {
        let exe = norm(&w.exe);
        let title = norm(&w.title);
        exe.contains("spotify") || title.contains("spotify")
    }) {
        unsafe {
            if IsIconic(w.hwnd) != 0 {
                ShowWindow(w.hwnd, SW_RESTORE);
            } else {
                ShowWindow(w.hwnd, SW_SHOW);
            }
            SetForegroundWindow(w.hwnd);
        }
    } else {
        let _ = Command::new("explorer.exe")
            .arg("spotify:")
            .spawn();
    }
}

// Natively triggers the big green circular Play button in Spotify via Windows UI Automation
pub(crate) fn trigger_spotify_play_button() -> bool {
    let hwnd_val = find_spotify_hwnd().map(|h| h as usize).unwrap_or(0);

    let script = format!(r#"
Add-Type -AssemblyName UIAutomationClient -ErrorAction SilentlyContinue
Add-Type -AssemblyName UIAutomationTypes -ErrorAction SilentlyContinue

$hwnd = [IntPtr]{hwnd_val}
$target = $null
$btnCond = New-Object System.Windows.Automation.PropertyCondition([System.Windows.Automation.AutomationElement]::ControlTypeProperty, [System.Windows.Automation.ControlType]::Button)

if ($hwnd -ne [IntPtr]::Zero) {{
    try {{
        $elem = [System.Windows.Automation.AutomationElement]::FromHandle($hwnd)
        if ($elem) {{
            $buttons = $elem.FindAll([System.Windows.Automation.TreeScope]::Descendants, $btnCond)
            foreach ($b in $buttons) {{
                $n = $b.Current.Name
                $aid = $b.Current.AutomationId
                if ($aid -eq "play-button" -or ($n -like "Play *" -and $n -notlike "Play / Pause*") -or $n -eq "Play") {{
                    $target = $b
                    break
                }}
            }}
        }}
    }} catch {{}}
}}

if (-not $target) {{
    $sp = Get-Process Spotify -ErrorAction SilentlyContinue
    if ($sp) {{
        $root = [System.Windows.Automation.AutomationElement]::RootElement
        $pids = [int[]]$sp.Id
        $conds = New-Object 'System.Collections.Generic.List[System.Windows.Automation.Condition]'
        foreach ($spPid in $pids) {{
            $conds.Add((New-Object System.Windows.Automation.PropertyCondition([System.Windows.Automation.AutomationElement]::ProcessIdProperty, $spPid)))
        }}
        $cond = if ($conds.Count -gt 1) {{
            New-Object System.Windows.Automation.OrCondition($conds.ToArray())
        }} elseif ($conds.Count -eq 1) {{
            $conds[0]
        }} else {{
            exit 1
        }}
        $spWins = $root.FindAll([System.Windows.Automation.TreeScope]::Children, $cond)
        foreach ($w in $spWins) {{
            $buttons = $w.FindAll([System.Windows.Automation.TreeScope]::Descendants, $btnCond)
            foreach ($b in $buttons) {{
                $n = $b.Current.Name
                $aid = $b.Current.AutomationId
                if ($aid -eq "play-button" -or ($n -like "Play *" -and $n -notlike "Play / Pause*") -or $n -eq "Play") {{
                    $target = $b
                    break
                }}
            }}
            if ($target) {{ break }}
        }}
    }}
}}

if ($target) {{
    try {{
        $pattern = $target.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern)
        $pattern.Invoke()
        exit 0
    }} catch {{
        $r = $target.Current.BoundingRectangle
        $x = [int]($r.X + $r.Width / 2)
        $y = [int]($r.Y + $r.Height / 2)
        Write-Output "$x,$y"
        exit 2
    }}
}}
exit 3
"#);

    let output = Command::new("powershell")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            &script,
        ])
        .creation_flags(CREATE_NO_WINDOW)
        .output();

    if let Ok(out) = output {
        if out.status.code() == Some(0) {
            println!("✅ Successfully invoked Spotify play button via InvokePattern");
            return true;
        } else if out.status.code() == Some(2) {
            let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if let Some((xs, ys)) = s.split_once(',') {
                if let (Ok(x), Ok(y)) = (xs.trim().parse::<i32>(), ys.trim().parse::<i32>()) {
                    println!("🎯 Clicking Spotify play button at ({}, {}) in native Rust", x, y);
                    unsafe {
                        SetCursorPos(x, y);
                        let mut inputs: [INPUT; 2] = std::mem::zeroed();
                        inputs[0].r#type = INPUT_MOUSE;
                        inputs[0].Anonymous.mi.dwFlags = MOUSEEVENTF_LEFTDOWN;
                        inputs[1].r#type = INPUT_MOUSE;
                        inputs[1].Anonymous.mi.dwFlags = MOUSEEVENTF_LEFTUP;
                        SendInput(2, inputs.as_mut_ptr(), std::mem::size_of::<INPUT>() as i32);
                    }
                    return true;
                }
            }
        }
    }

    false
}

pub(crate) fn spotify_uia_play_search(query: &str) -> bool {
    let script = r#"
Add-Type -AssemblyName UIAutomationClient -ErrorAction SilentlyContinue
Add-Type -AssemblyName UIAutomationTypes -ErrorAction SilentlyContinue
$AE = [System.Windows.Automation.AutomationElement]
$TS = [System.Windows.Automation.TreeScope]
$btn = New-Object System.Windows.Automation.PropertyCondition($AE::ControlTypeProperty, [System.Windows.Automation.ControlType]::Button)
$tokens = ('__Q__'.ToLower() -split '\s+') | Where-Object { $_.Length -ge 2 }
$start = Get-Date
$seen = @()
while (((Get-Date) - $start).TotalSeconds -lt 14) {
  $pids = @(Get-Process Spotify -ErrorAction SilentlyContinue | ForEach-Object { $_.Id })
  $wins = $AE::RootElement.FindAll($TS::Children, [System.Windows.Automation.Condition]::TrueCondition)
  $best = $null; $bestScore = 0; $fallback = $null
  foreach ($w in $wins) {
    if ($pids -notcontains $w.Current.ProcessId) { continue }
    $rect = $w.Current.BoundingRectangle
    $bs = $w.FindAll($TS::Descendants, $btn)
    foreach ($b in $bs) {
      $n = $b.Current.Name
      if (-not $n -or $n -notlike 'Play *' -or $n -like 'Play / Pause*') { continue }
      $seen += $n
      $nl = $n.ToLower(); $s = 0
      foreach ($t in $tokens) { if ($nl.Contains($t)) { $s++ } }
      if ($s -gt $bestScore) { $best = $b; $bestScore = $s }
      $br = $b.Current.BoundingRectangle
      if (-not $fallback -and $br.X -gt ($rect.X + $rect.Width * 0.25) -and $br.Y -gt ($rect.Y + 90)) { $fallback = $b }
    }
  }
  $target = $best
  if (-not $target -and ((Get-Date) - $start).TotalSeconds -gt 6) { $target = $fallback }
  if ($target) {
    try {
      $target.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
      Write-Output ('OK: ' + $target.Current.Name)
      exit 0
    } catch {}
  }
  Start-Sleep -Milliseconds 500
}
Write-Output ('NOTFOUND: ' + (($seen | Select-Object -Unique -First 12) -join ' | '))
exit 3
"#
    .replace("__Q__", &query.replace('\'', "''"));

    match Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
    {
        Ok(o) => {
            println!("🧭 Spotify UIA: {}", String::from_utf8_lossy(&o.stdout).trim());
            o.status.code() == Some(0)
        }
        Err(e) => {
            println!("⚠️ powershell error: {e}");
            false
        }
    }
}

// Launch Spotify with native UI Automation & native GSMTC controls (0% hotkeys)
pub(crate) fn launch_spotify(query: Option<&str>) {
    let q = query.unwrap_or("").trim();

    let spotify_open = is_spotify_window_open();

    let is_liked = q == "liked song"
        || q == "liked songs"
        || q == "my liked"
        || q == "my liked song"
        || q == "my liked songs"
        || q == "my like song"
        || q == "like song"
        || q == "amar liked"
        || q == "amar like"
        || q == "liked"
        || q == "liked s"
        || q.contains("liked");

    if is_liked {
        println!("🎵 Spotify: Opening Liked Songs & triggering green Play button natively");
        let _ = Command::new("explorer.exe")
            .arg("spotify:collection:tracks")
            .spawn();

        std::thread::spawn(move || {
            let initial_wait = if spotify_open { 800 } else { 2200 };
            std::thread::sleep(Duration::from_millis(initial_wait));
            let mut played = false;
            for attempt in 1..=6 {
                if trigger_spotify_play_button() {
                    println!("🎵 Spotify Liked Songs: Play button triggered on attempt {attempt}");
                    played = true;
                    break;
                }
                std::thread::sleep(Duration::from_millis(400));
            }
            if !played {
                println!("🎵 Spotify Liked Songs: triggering native GSMTC play");
                media_native::control_media(media_native::MediaAction::Play, Some(SP_SESSION));
            }
        });
        return;
    }

    if !q.is_empty() && q != "music" && q != "song" && q != "something" && q != "on" && q != "d" {
        println!("🎵 Spotify: searching and playing \"{}\"", q);
        let q_string = q.to_string();

        std::thread::spawn(move || {
            let enc = url_encode(&q_string);
            let _ = Command::new("explorer.exe")
                .arg(format!("spotify:search:{enc}"))
                .spawn();

            let mut hwnd_opt = None;
            for _ in 0..60 {
                std::thread::sleep(Duration::from_millis(100));
                if let Some(h) = find_spotify_hwnd() {
                    hwnd_opt = Some(h);
                    break;
                }
            }
            let Some(hwnd) = hwnd_opt else {
                println!("⚠️ Spotify window pai nai");
                return;
            };

            unsafe {
                if IsIconic(hwnd) != 0 {
                    ShowWindow(hwnd, SW_SHOWNOACTIVATE);
                }
                SetForegroundWindow(hwnd);
            }

            std::thread::sleep(Duration::from_millis(if spotify_open { 1500 } else { 3000 }));

            // 3. UIA diye search result-er Play button invoke (mouse lagena)
            let played = spotify_uia_play_search(&q_string);
            if !played {
                println!("❌ Spotify play hoy nai (upore NOTFOUND list dekho)");
            }
        });
        return;
    } else {
        println!("🎵 Spotify: Opening & resuming playback via native GSMTC");
        if !spotify_open {
            println!("🎵 Spotify fresh open, playing Liked Songs");
            launch_spotify(Some("liked songs"));
            return;
        }
        open_spotify_app();
        let mut resumed = false;
        for _ in 1..=6 {
            if media_native::control_media(media_native::MediaAction::Play, Some(SP_SESSION)) {
                resumed = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(400));
        }
        if !resumed {
            launch_spotify(Some("liked songs"));
        }
    }
}

/// Find the best Spotify HWND using process scan

#[allow(dead_code)]

pub(crate) fn find_spotify_hwnd() -> Option<HWND> {
    // First try the normal list (fastest path)

    let wins = list_windows();

    if let Some(w) = wins
        .iter()
        .find(|w| norm(&w.exe).contains("spotify") || norm(&w.title).contains("spotify"))
    {
        return Some(w.hwnd);
    }

    // Fallback: scan ALL Spotify PIDs and find any usable HWND

    // We use EnumWindows without the strict owner/style filters

    let mut pids: Vec<u32> = Vec::new();

    // Collect PIDs of all running Spotify processes

    // We read the process list from a snapshot

    unsafe {
        let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);

        if !snap.is_null() {
            let mut pe: PROCESSENTRY32W = std::mem::zeroed();

            pe.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;

            if Process32FirstW(snap, &mut pe) != 0 {
                loop {
                    let name = String::from_utf16_lossy(
                        &pe.szExeFile[..pe
                            .szExeFile
                            .iter()
                            .position(|&c| c == 0)
                            .unwrap_or(pe.szExeFile.len())],
                    )
                    .to_lowercase();

                    if name.contains("spotify") {
                        pids.push(pe.th32ProcessID);
                    }

                    if Process32NextW(snap, &mut pe) == 0 {
                        break;
                    }
                }
            }

            CloseHandle(snap);
        }
    }

    if pids.is_empty() {
        return None;
    }

    // Now EnumWindows to find any visible window belonging to these PIDs

    struct Ctx {
        pids: Vec<u32>,
        found: HWND,
    }

    let mut ctx = Ctx {
        pids,
        found: 0 as HWND,
    };

    unsafe extern "system" fn find_cb(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let ctx = &mut *(lparam as *mut Ctx);

        if !ctx.found.is_null() {
            return 0;
        } // stop once found

        if IsWindowVisible(hwnd) == 0 {
            return 1;
        }

        let mut pid = 0u32;

        GetWindowThreadProcessId(hwnd, &mut pid);

        if ctx.pids.contains(&pid) {
            let mut r: RECT = std::mem::zeroed();
            GetWindowRect(hwnd, &mut r);
            let w = r.right - r.left;
            let h = r.bottom - r.top;
            if (w > 300 && h > 300) || IsIconic(hwnd) != 0 {
                ctx.found = hwnd;
                return 0; // stop enum
            }
        }

        1
    }

    unsafe {
        EnumWindows(Some(find_cb), &mut ctx as *mut _ as LPARAM);
    }

    if !ctx.found.is_null() {
        Some(ctx.found)
    } else {
        None
    }
}
