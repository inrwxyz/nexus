// cs/browser.rs — Reliable Windows URL launcher using STA-initialized ShellExecuteW
// with direct Registry default-browser fallback (zero cmd.exe `&` or URL truncation bugs).

use std::process::Command;
use windows_sys::Win32::System::Com::{
    CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE,
};
use windows_sys::Win32::System::Registry::{
    RegCloseKey, RegOpenKeyExW, RegQueryValueExW, HKEY, HKEY_CLASSES_ROOT, HKEY_CURRENT_USER,
    KEY_READ,
};
use windows_sys::Win32::UI::Shell::ShellExecuteW;
use windows_sys::Win32::UI::WindowsAndMessaging::{AllowSetForegroundWindow, SW_SHOWNORMAL};

fn reg_read_sz(root: HKEY, subkey: &str, value_name: Option<&str>) -> Option<String> {
    let sub_w: Vec<u16> = subkey.encode_utf16().chain(std::iter::once(0)).collect();
    let val_w: Option<Vec<u16>> =
        value_name.map(|v| v.encode_utf16().chain(std::iter::once(0)).collect());

    unsafe {
        let mut hkey: HKEY = std::ptr::null_mut();
        if RegOpenKeyExW(root, sub_w.as_ptr(), 0, KEY_READ, &mut hkey) != 0 {
            return None;
        }
        let val_ptr = val_w.as_ref().map(|v| v.as_ptr()).unwrap_or(std::ptr::null());
        let mut bytes: u32 = 0;
        let rc = RegQueryValueExW(
            hkey,
            val_ptr,
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            &mut bytes,
        );
        if rc != 0 || bytes < 2 {
            RegCloseKey(hkey);
            return None;
        }
        let mut buf = vec![0u16; (bytes as usize + 1) / 2];
        let rc2 = RegQueryValueExW(
            hkey,
            val_ptr,
            std::ptr::null(),
            std::ptr::null_mut(),
            buf.as_mut_ptr() as *mut u8,
            &mut bytes,
        );
        RegCloseKey(hkey);
        if rc2 != 0 {
            return None;
        }
        let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        Some(String::from_utf16_lossy(&buf[..len]))
    }
}

fn spawn_registered_https_browser(url: &str) -> Result<(), String> {
    let prog_id = reg_read_sz(
        HKEY_CURRENT_USER,
        r"Software\Microsoft\Windows\Shell\Associations\UrlAssociations\https\UserChoice",
        Some("ProgId"),
    )
    .ok_or_else(|| "No registered HTTPS ProgId".to_string())?;

    let cmd = reg_read_sz(
        HKEY_CLASSES_ROOT,
        &format!(r"{prog_id}\shell\open\command"),
        None,
    )
    .ok_or_else(|| format!("No open command for {prog_id}"))?;

    let trimmed = cmd.trim();
    let exe = if let Some(rest) = trimmed.strip_prefix('"') {
        rest.split('"')
            .next()
            .map(|s| s.to_string())
            .ok_or_else(|| "Malformed quoted browser path".to_string())?
    } else {
        trimmed
            .split_whitespace()
            .next()
            .map(|s| s.to_string())
            .ok_or_else(|| "Malformed browser path".to_string())?
    };

    let mut command = Command::new(&exe);
    if cmd.contains("--single-argument") {
        command.arg("--single-argument");
    }
    command
        .arg(url)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("Failed to spawn {exe}: {e}"))
}

pub fn open_url(url: &str) -> Result<(), String> {
    let trimmed = url.trim().to_string();
    if trimmed.is_empty() {
        return Err("Empty URL".to_string());
    }
    if !(trimmed.starts_with("https://") || trimmed.starts_with("http://")) {
        return Err("Refused non-HTTP URL".to_string());
    }
    open_target_raw(&trimmed)
}

fn open_target_raw(target: &str) -> Result<(), String> {
    let trimmed = target.trim().to_string();

    // Run ShellExecuteW on an explicit STA COM thread as required by Windows Shell documentation.
    std::thread::spawn(move || unsafe {
        let hr = CoInitializeEx(
            std::ptr::null(),
            (COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE) as u32,
        );
        let _ = AllowSetForegroundWindow(u32::MAX);

        let op: Vec<u16> = "open\0".encode_utf16().collect();
        let file: Vec<u16> = trimmed.encode_utf16().chain(std::iter::once(0)).collect();

        let hinst = ShellExecuteW(
            std::ptr::null_mut(),
            op.as_ptr(),
            file.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            SW_SHOWNORMAL,
        );

        if hr == 0 || hr == 1 {
            CoUninitialize();
        }

        if (hinst as isize) > 32 {
            return Ok(());
        }

        if spawn_registered_https_browser(&trimmed).is_ok() {
            return Ok(());
        }

        Command::new("cmd")
            .args(["/C", "start", "", &trimmed])
            .spawn()
            .map(|_| ())
            .map_err(|e| {
                format!(
                    "Failed to launch browser (ShellExecuteW={}, fallback={})",
                    hinst as isize, e
                )
            })
    })
    .join()
    .map_err(|_| "Browser launcher thread panicked".to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_empty_and_unsafe_urls() {
        assert!(open_url("").is_err());
        assert!(open_url("   ").is_err());
        assert!(open_url("javascript:alert(1)").is_err());
        assert!(open_url("file:///C:/Windows/win.ini").is_err());
    }
}
