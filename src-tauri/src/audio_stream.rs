use std::net::{SocketAddr, UdpSocket};
use std::ptr::null_mut;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::Duration;

use windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume;
use windows::Win32::Media::Audio::{
    eConsole, eRender, IAudioCaptureClient, IAudioClient, IMMDeviceEnumerator, MMDeviceEnumerator,
    AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_LOOPBACK,
};
use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CLSCTX_ALL, COINIT_MULTITHREADED};

#[link(name = "winmm")]
extern "system" {
    fn timeBeginPeriod(uPeriod: u32) -> u32;
    fn timeEndPeriod(uPeriod: u32) -> u32;
}

static ACTIVE: AtomicBool = AtomicBool::new(false);
static WAS_MUTED: AtomicBool = AtomicBool::new(false);
const SILENT: u32 = 0x2; // AUDCLNT_BUFFERFLAGS_SILENT

unsafe fn set_pc_mute(mute: bool) -> bool {
    let run = || -> windows::core::Result<bool> {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        let en: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
        let dev = en.GetDefaultAudioEndpoint(eRender, eConsole)?;
        let vol: IAudioEndpointVolume = dev.Activate(CLSCTX_ALL, None)?;
        let prev = vol.GetMute()?.as_bool();
        vol.SetMute(mute, std::ptr::null())?;
        Ok(prev)
    };
    run().unwrap_or(false)
}

pub fn stop() {
    ACTIVE.store(false, Ordering::SeqCst);
    let prev = WAS_MUTED.load(Ordering::SeqCst);
    unsafe { set_pc_mute(prev); }
}

/// Returns the sample rate (always stereo, 16-bit).
pub fn start(dst: SocketAddr) -> Result<u32, String> {
    if ACTIVE.swap(true, Ordering::SeqCst) {
        stop();
        std::thread::sleep(Duration::from_millis(60));
        ACTIVE.store(true, Ordering::SeqCst);
    }
    // Mute PC speakers natively so sound plays ONLY on the phone
    let was = unsafe { set_pc_mute(true) };
    WAS_MUTED.store(was, Ordering::SeqCst);

    let (tx, rx) = mpsc::channel::<Result<u32, String>>();
    std::thread::spawn(move || {
        if let Err(e) = unsafe { run(dst, &tx) } {
            let _ = tx.send(Err(e.to_string()));
        }
        ACTIVE.store(false, Ordering::SeqCst);
        let prev = WAS_MUTED.load(Ordering::SeqCst);
        unsafe { set_pc_mute(prev); }
    });
    rx.recv_timeout(Duration::from_secs(3)).unwrap_or(Err("timeout".into()))
}

unsafe fn run(dst: SocketAddr, tx: &mpsc::Sender<Result<u32, String>>) -> windows::core::Result<()> {
    timeBeginPeriod(1);
    struct PeriodGuard;
    impl Drop for PeriodGuard {
        fn drop(&mut self) {
            unsafe { timeEndPeriod(1); }
        }
    }
    let _period_guard = PeriodGuard;

    let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
    let en: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
    let dev = en.GetDefaultAudioEndpoint(eRender, eConsole)?;
    let client: IAudioClient = dev.Activate(CLSCTX_ALL, None)?;
    let fmt = client.GetMixFormat()?;
    let (rate, ch, bits) = ((*fmt).nSamplesPerSec, (*fmt).nChannels as usize, (*fmt).wBitsPerSample);
    // Request minimum engine period (10ms = 100_000 100ns units) for ultra-low latency sync
    client.Initialize(AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_LOOPBACK, 100_000, 0, fmt, None)?;
    let cap: IAudioCaptureClient = client.GetService()?;

    let sock = UdpSocket::bind("0.0.0.0:0").map_err(|_| windows::core::Error::from_win32())?;
    sock.connect(dst).map_err(|_| windows::core::Error::from_win32())?; // phone closed port => send() errors => we stop
    client.Start()?;
    let _ = tx.send(Ok(rate));

    let mut pcm: Vec<u8> = Vec::with_capacity(4096);
    while ACTIVE.load(Ordering::Relaxed) {
        let mut drained = 0;
        while cap.GetNextPacketSize()? > 0 {
            drained += 1;
            let (mut data, mut frames, mut flags) = (null_mut::<u8>(), 0u32, 0u32);
            cap.GetBuffer(&mut data, &mut frames, &mut flags, None, None)?;
            pcm.clear();
            if flags & SILENT == 0 && !data.is_null() {
                for f in 0..frames as usize {
                    for c in 0..2usize.min(ch) {
                        let s = if bits == 32 {
                            let v = *(data as *const f32).add(f * ch + c);
                            (v.clamp(-1.0, 1.0) * 32767.0) as i16
                        } else {
                            *(data as *const i16).add(f * ch + c)
                        };
                        pcm.extend_from_slice(&s.to_le_bytes());
                    }
                    if ch == 1 { // mono -> duplicate
                        let l = [pcm[pcm.len() - 2], pcm[pcm.len() - 1]];
                        pcm.extend_from_slice(&l);
                    }
                }
            }
            cap.ReleaseBuffer(frames)?;
            for chunk in pcm.chunks(1280) {
                if sock.send(chunk).is_err() {
                    ACTIVE.store(false, Ordering::SeqCst);
                    break;
                }
            }
        }
        if drained == 0 {
            std::thread::sleep(Duration::from_millis(1));
        }
    }
    client.Stop()?;
    Ok(())
}
