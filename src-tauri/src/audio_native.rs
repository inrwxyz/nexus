use windows::core::Interface;
use windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume;
use windows::Win32::Media::Audio::{
    eConsole, eRender, IAudioSessionControl2, IAudioSessionManager2, IMMDevice,
    IMMDeviceEnumerator, ISimpleAudioVolume, MMDeviceEnumerator,
};
use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CLSCTX_ALL, COINIT_MULTITHREADED};
use windows_sys::Win32::Foundation::CloseHandle;
use windows_sys::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_QUERY_LIMITED_INFORMATION,
};

pub enum Change {
    Set(u32),
    Delta(i32),
    ToggleMute,
}

fn new_level(cur: f32, ch: &Change) -> f32 {
    match ch {
        Change::Set(p) => *p as f32 / 100.0,
        Change::Delta(d) => (cur + *d as f32 / 100.0).clamp(0.0, 1.0),
        Change::ToggleMute => cur,
    }
}

fn exe_stem(pid: u32) -> String {
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if h.is_null() {
            return String::new();
        }
        let mut buf = [0u16; 1024];
        let mut sz = buf.len() as u32;
        let mut out = String::new();
        if QueryFullProcessImageNameW(h, 0, buf.as_mut_ptr(), &mut sz) != 0 {
            let full = String::from_utf16_lossy(&buf[..sz as usize]);
            out = std::path::Path::new(&full)
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_string();
        }
        CloseHandle(h);
        out
    }
}

unsafe fn device() -> windows::core::Result<IMMDevice> {
    let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
    let en: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
    en.GetDefaultAudioEndpoint(eRender, eConsole)
}

pub fn master(ch: Change) -> bool {
    unsafe {
        let run = || -> windows::core::Result<()> {
            let vol: IAudioEndpointVolume = device()?.Activate(CLSCTX_ALL, None)?;
            match ch {
                Change::ToggleMute => {
                    let m = vol.GetMute()?.as_bool();
                    vol.SetMute(if m { false } else { true }, std::ptr::null())?;
                }
                _ => {
                    let cur = vol.GetMasterVolumeLevelScalar()?;
                    vol.SetMasterVolumeLevelScalar(new_level(cur, &ch), std::ptr::null())?;
                    vol.SetMute(false, std::ptr::null())?;
                }
            }
            Ok(())
        };
        match run() {
            Ok(_) => true,
            Err(e) => {
                println!("⚠️ master volume error: {e}");
                false
            }
        }
    }
}

pub fn app(targets: &[&str], ch: Change) -> bool {
    unsafe {
        let run = || -> windows::core::Result<bool> {
            let mgr: IAudioSessionManager2 = device()?.Activate(CLSCTX_ALL, None)?;
            let sessions = mgr.GetSessionEnumerator()?;
            let n = sessions.GetCount()?;
            let mut hit = false;
            for i in 0..n {
                let ctl = sessions.GetSession(i)?;
                let ctl2: IAudioSessionControl2 = ctl.cast()?;
                let pid = ctl2.GetProcessId()?;
                if pid == 0 {
                    continue;
                }
                let name = exe_stem(pid).to_lowercase();
                if !targets.iter().any(|t| name.contains(t)) {
                    continue;
                }
                let sav: ISimpleAudioVolume = ctl.cast()?;
                match ch {
                    Change::ToggleMute => {
                        let m = sav.GetMute()?.as_bool();
                        sav.SetMute(if m { false } else { true }, std::ptr::null())?;
                    }
                    _ => {
                        let cur = sav.GetMasterVolume()?;
                        sav.SetMasterVolume(new_level(cur, &ch), std::ptr::null())?;
                        sav.SetMute(false, std::ptr::null())?;
                    }
                }
                hit = true;
            }
            Ok(hit)
        };
        match run() {
            Ok(h) => h,
            Err(e) => {
                println!("⚠️ app volume error: {e}");
                false
            }
        }
    }
}
