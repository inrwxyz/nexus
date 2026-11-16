use std::ptr::null;
use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE, LUID};
use windows_sys::Win32::Security::{
    AdjustTokenPrivileges, LookupPrivilegeValueW, LUID_AND_ATTRIBUTES, SE_PRIVILEGE_ENABLED,
    TOKEN_ADJUST_PRIVILEGES, TOKEN_PRIVILEGES, TOKEN_QUERY,
};
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
};
use windows_sys::Win32::System::Power::SetSuspendState;
use windows_sys::Win32::System::Shutdown::{AbortSystemShutdownW, InitiateSystemShutdownExW};
use windows_sys::Win32::System::Threading::{
    GetCurrentProcess, OpenProcess, OpenProcessToken, TerminateProcess,
};

fn w(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Returns Some(result) if `l` (already lower-cased/normalised) is one of ours.
pub fn try_handle(l: &str) -> Option<bool> {
    Some(match l {
        "shutdown" | "shutdown pc" | "turn off pc" | "shut down pc" => shutdown(false),
        "restart" | "restart pc" | "reboot" | "reboot pc" => shutdown(true),
        "cancel shutdown" | "abort shutdown" | "stop shutdown" => unsafe {
            enable_shutdown_priv();
            AbortSystemShutdownW(null()) != 0
        },
        "sleep" | "sleep pc" | "sleep computer" => unsafe { SetSuspendState(0, 0, 0) != 0 },
        "show desktop" | "minimize all" => {
            crate::trackpad::press_combo(&[0x5B, 0x44]); // Win + D  (make press_combo pub, see 1.7)
            true
        }
        _ => return None,
    })
}

unsafe fn enable_shutdown_priv() {
    let mut tok = std::mem::zeroed();
    if OpenProcessToken(GetCurrentProcess(), TOKEN_ADJUST_PRIVILEGES | TOKEN_QUERY, &mut tok) == 0 {
        return;
    }
    let mut luid: LUID = std::mem::zeroed();
    if LookupPrivilegeValueW(null(), w("SeShutdownPrivilege").as_ptr(), &mut luid) != 0 {
        let tp = TOKEN_PRIVILEGES {
            PrivilegeCount: 1,
            Privileges: [LUID_AND_ATTRIBUTES { Luid: luid, Attributes: SE_PRIVILEGE_ENABLED }],
        };
        AdjustTokenPrivileges(tok, 0, &tp, 0, std::ptr::null_mut(), std::ptr::null_mut());
    }
    CloseHandle(tok);
}

fn shutdown(reboot: bool) -> bool {
    unsafe {
        enable_shutdown_priv();
        // 15s grace, same as before; "cancel shutdown" aborts it
        InitiateSystemShutdownExW(null(), null(), 15, 0, reboot as i32, 0) != 0
    }
}

fn for_each_process(mut f: impl FnMut(u32, &str)) {
    unsafe {
        let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snap == INVALID_HANDLE_VALUE {
            return;
        }
        let mut e: PROCESSENTRY32W = std::mem::zeroed();
        e.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        let mut ok = Process32FirstW(snap, &mut e);
        while ok != 0 {
            let len = e.szExeFile.iter().position(|&c| c == 0).unwrap_or(260);
            f(e.th32ProcessID, &String::from_utf16_lossy(&e.szExeFile[..len]).to_lowercase());
            ok = Process32NextW(snap, &mut e);
        }
        CloseHandle(snap);
    }
}

/// Native replacement for `taskkill /F /IM name.exe`
pub fn kill_by_name(exe: &str) -> bool {
    let target = {
        let t = exe.to_lowercase();
        if t.ends_with(".exe") { t } else { format!("{t}.exe") }
    };
    let mut hit = false;
    for_each_process(|pid, name| {
        if name == target {
            unsafe {
                let h = OpenProcess(0x0001 /* PROCESS_TERMINATE */, 0, pid);
                if !h.is_null() {
                    hit |= TerminateProcess(h, 1) != 0;
                    CloseHandle(h);
                }
            }
        }
    });
    hit
}
