// safety.rs — App discovery & URL validation safety rules.
use super::*;

/// Never launched by voice (names compared after norm(), lowercase, exact match).
const NEVER_LAUNCH: &[&str] = &[
    "file explorer",
    "windows explorer",
    "explorer",
    "this pc",
];

/// System tools: blocked when ANY of these appears as whole word(s) in the app name.
const BLOCKED_NAME_PARTS: &[&str] = &[
    "registry", "regedit", "settings", "control panel", "cmd", "command prompt", "powershell",
    "powershell ise", "ise", "terminal", "task manager", "services", "event viewer", "device manager",
    "computer management", "disk management", "system configuration", "msconfig",
    "windows tools", "administrative tools", "group policy", "resource monitor",
    "performance monitor", "task scheduler", "local security policy", "component services",
    "odbc", "iscsi", "system information", "windows memory diagnostic", "memory diagnostics",
    "developer command prompt", "developer powershell", "native tools", "cross tools",
    "application verifier", "gpuview", "performance analyzer", "performance recorder",
    "documentation", "release notes", "sample", "faq", "compiler", "uninstall",
    "firewall", "windows defender", "recoverydrive", "print management", "dfrgui", "steps recorder",
    "character map", "run", "livecaptions", "narrator", "magnify", "voiceaccess", "on-screen keyboard",
    "windows media player legacy",
];

/// Start-Menu folders that only hold Windows system tools.
const BLOCKED_PATH_PARTS: &[&str] = &[
    "\\windows system\\", "\\windows tools\\", "\\windows administrative tools\\",
    "\\administrative tools\\", "\\windows powershell\\", "\\system tools\\",
    "\\maintenance\\", "\\startup\\", "\\accessibility\\", "\\accessories\\",
    "\\windows kits\\", "\\visual studio\\",
];

/// Shortcut / launch targets that are never allowed.
const BLOCKED_TARGET_PARTS: &[&str] = &[
    "ms-settings:", "regedit", "cmd.exe", "powershell", "pwsh", "wt.exe", "taskmgr", "mmc.exe",
    ".msc", ".reg", ".bat", ".cmd", ".ps1", ".vbs", "control.exe", "conhost.exe", "bash.exe",
    "powershell_ise",
];

/// true if this app (name + launch id/path) may be launched by voice.
pub(crate) fn app_allowed(name: &str, id: &str) -> bool {
    let norm_name = norm(name);
    let n = format!(" {} ", norm_name);
    if NEVER_LAUNCH.iter().any(|b| norm_name == norm(b)) {
        return false;
    }
    for b in BLOCKED_NAME_PARTS {
        let b_norm = norm(b);
        if norm_name == b_norm || n.contains(&format!(" {} ", b_norm)) || norm_name.contains(&b_norm) {
            return false;
        }
    }
    let low = id.to_lowercase();
    if BLOCKED_PATH_PARTS.iter().any(|b| low.contains(b)) {
        return false;
    }
    !BLOCKED_TARGET_PARTS.iter().any(|b| low.contains(b))
}

/// Only plain web links may be opened through open_browser()
pub(crate) fn url_allowed(url: &str) -> bool {
    let u = url.trim().to_lowercase();
    u.starts_with("https://") || u.starts_with("http://")
}
