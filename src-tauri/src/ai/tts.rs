//! Natural Siri / Google-grade Voice Output + Windows OneCore Male (`Ravi`/`George`) offline fallback.
//! Drives real-time voice amplitude bars cleanly on Inex Island until speech finishes or is canceled.

use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

static SPEAK_SEQ: AtomicU64 = AtomicU64::new(0);
static STOP_REQ: AtomicBool = AtomicBool::new(false);

pub fn stop() {
    STOP_REQ.store(true, Ordering::SeqCst);
    SPEAK_SEQ.fetch_add(1, Ordering::SeqCst);
    #[cfg(target_os = "windows")]
    {
        mci_cmd("stop nexus_tts");
        mci_cmd("close nexus_tts");
        unsafe {
            PlaySoundW(core::ptr::null(), 0, 0);
        }
    }
}

#[cfg(target_os = "windows")]
#[link(name = "winmm")]
extern "system" {
    fn mciSendStringW(cmd: *const u16, ret: *mut u16, cch_ret: u32, hwnd_cb: isize) -> u32;
    fn PlaySoundW(psz_sound: *const u8, hmod: isize, fdw_sound: u32) -> i32;
}

#[cfg(target_os = "windows")]
fn mci_cmd(cmd: &str) -> u32 {
    let wide: Vec<u16> = cmd.encode_utf16().chain(std::iter::once(0)).collect();
    unsafe { mciSendStringW(wide.as_ptr(), core::ptr::null_mut(), 0, 0) }
}

#[cfg(target_os = "windows")]
fn mci_query(cmd: &str) -> String {
    let wide: Vec<u16> = cmd.encode_utf16().chain(std::iter::once(0)).collect();
    let mut buf = [0u16; 128];
    let rc = unsafe { mciSendStringW(wide.as_ptr(), buf.as_mut_ptr(), buf.len() as u32, 0) };
    if rc != 0 {
        return String::new();
    }
    let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..end]).trim().to_string()
}

fn temp_mp3_path(seq: u64) -> PathBuf {
    if let Ok(local) = std::env::var("LOCALAPPDATA") {
        let dir = PathBuf::from(local).join("Nexus");
        let _ = fs::create_dir_all(&dir);
        return dir.join(format!("tts_{}.mp3", seq));
    }
    std::env::temp_dir().join(format!("nexus_tts_{}.mp3", seq))
}

fn cleanup_stale_tts_files() {
    let dir = std::env::var("LOCALAPPDATA")
        .map(|l| PathBuf::from(l).join("Nexus"))
        .unwrap_or_else(|_| std::env::temp_dir());
    if let Ok(entries) = fs::read_dir(dir) {
        for e in entries.flatten() {
            if let Some(name) = e.file_name().to_str() {
                if name.starts_with("tts_") && name.ends_with(".mp3") {
                    let _ = fs::remove_file(e.path());
                }
            }
        }
    }
}

fn percent_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 2);
    for b in s.as_bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(*b as char),
            b' ' => out.push_str("%20"),
            _ => out.push_str(&format!("%{:02X}", b)),
        }
    }
    out
}

pub fn detect_tts_lang(text: &str) -> &'static str {
    if text.chars().any(|c| ('\u{0980}'..='\u{09FF}').contains(&c)) {
        return "bn";
    }
    if text.chars().any(|c| ('\u{0900}'..='\u{097F}').contains(&c)) {
        return "hi";
    }
    let lower = format!(" {} ", text.to_ascii_lowercase());
    let hinglish_hits = [
        " hai ", " hoon ", " hu ", " main ", " mein ", " kya ", " aap ", " tum ", " mera ",
        " nahi ", " haan ", " yaar ", " bhai ", " badiya ", " theek ", " sakta ", " raha ",
        " chalo ", " kar ", " karo ", " mast ", " sab ", " dekho ", " dekh ", " aaj ",
    ]
    .iter()
    .filter(|w| lower.contains(*w))
    .count();
    if hinglish_hits >= 2 {
        "hi"
    } else {
        "en"
    }
}

fn split_speech_chunks(text: &str) -> Vec<String> {
    let clean = text
        .replace(['*', '#', '`', '_'], "")
        .replace('\n', ". ");
    let mut out = Vec::new();
    let mut cur = String::new();

    for word in clean.split_whitespace() {
        if cur.len() + word.len() + 1 > 175 && !cur.is_empty() {
            out.push(cur.trim().to_string());
            cur.clear();
        }
        if !cur.is_empty() {
            cur.push(' ');
        }
        cur.push_str(word);
        if (word.ends_with('.') || word.ends_with('?') || word.ends_with('!') || word.ends_with('।'))
            && cur.len() >= 45
        {
            out.push(cur.trim().to_string());
            cur.clear();
        }
    }
    if !cur.trim().is_empty() {
        out.push(cur.trim().to_string());
    }
    out
}

fn fetch_google_tts_mp3(chunk: &str, lang: &str) -> Result<Vec<u8>, String> {
    let q = percent_encode(chunk);
    let url = format!(
        "https://translate.google.com/translate_tts?ie=UTF-8&client=tw-ob&tl={}&q={}",
        lang, q
    );
    let agent = ureq::AgentBuilder::new().timeout(Duration::from_secs(6)).build();
    let resp = agent
        .get(&url)
        .set("User-Agent", "Mozilla/5.0")
        .call()
        .map_err(|e| format!("Google TTS failed: {e}"))?;

    let mut bytes = Vec::with_capacity(16_384);
    std::io::Read::read_to_end(&mut resp.into_reader(), &mut bytes)
        .map_err(|e| format!("TTS stream read failed: {e}"))?;
    if bytes.len() < 128 {
        return Err("Empty TTS audio".to_string());
    }
    Ok(bytes)
}

#[cfg(target_os = "windows")]
fn synthesize_onecore_wav(text: &str, style: &str) -> Result<Vec<u8>, String> {
    use windows::core::HSTRING;
    use windows::Media::SpeechSynthesis::{SpeechSynthesizer, VoiceGender};
    use windows::Storage::Streams::DataReader;

    let synth = SpeechSynthesizer::new().map_err(|e| format!("SpeechSynthesizer init: {e}"))?;
    if let Ok(voices) = SpeechSynthesizer::AllVoices() {
        let count = voices.Size().unwrap_or(0);
        let mut chosen = None;
        for i in 0..count {
            if let Ok(v) = voices.GetAt(i) {
                let name = v.DisplayName().map(|s| s.to_string()).unwrap_or_default();
                let male = v.Gender() == Ok(VoiceGender::Male);
                if style == "male_george" && name.contains("George") {
                    chosen = Some(v);
                    break;
                }
                if style == "male_ravi" && name.contains("Ravi") {
                    chosen = Some(v);
                    break;
                }
                if male && chosen.is_none() {
                    chosen = Some(v);
                }
            }
        }
        if let Some(v) = chosen {
            let _ = synth.SetVoice(&v);
        }
    }

    let stream = synth
        .SynthesizeTextToStreamAsync(&HSTRING::from(text))
        .map_err(|e| format!("Synth async failed: {e}"))?
        .get()
        .map_err(|e| format!("Synth get failed: {e}"))?;

    let size = stream.Size().map_err(|e| e.to_string())? as u32;
    if size <= 44 {
        return Err("Empty synthesized WAV".to_string());
    }
    let input = stream.GetInputStreamAt(0).map_err(|e| e.to_string())?;
    let reader = DataReader::CreateDataReader(&input).map_err(|e| e.to_string())?;
    reader.LoadAsync(size).map_err(|e| e.to_string())?.get().map_err(|e| e.to_string())?;
    let mut buf = vec![0u8; size as usize];
    reader.ReadBytes(&mut buf).map_err(|e| e.to_string())?;
    Ok(buf)
}

#[cfg(target_os = "windows")]
fn parse_wav_pcm_info(wav: &[u8]) -> Option<(u32, u16, u16, usize, usize)> {
    if wav.len() < 44 || &wav[0..4] != b"RIFF" || &wav[8..12] != b"WAVE" {
        return None;
    }
    let mut pos = 12usize;
    let mut sample_rate = 22050u32;
    let mut channels = 1u16;
    let mut bits = 16u16;
    while pos + 8 <= wav.len() {
        let id = &wav[pos..pos + 4];
        let sz = u32::from_le_bytes([wav[pos + 4], wav[pos + 5], wav[pos + 6], wav[pos + 7]]) as usize;
        let body = pos + 8;
        if id == b"fmt " && body + 16 <= wav.len() {
            channels = u16::from_le_bytes([wav[body + 2], wav[body + 3]]);
            sample_rate = u32::from_le_bytes([wav[body + 4], wav[body + 5], wav[body + 6], wav[body + 7]]);
            bits = u16::from_le_bytes([wav[body + 14], wav[body + 15]]);
        } else if id == b"data" && body <= wav.len() {
            let end = (body + sz).min(wav.len());
            return Some((sample_rate.max(8000), channels.max(1), bits.max(8), body, end));
        }
        pos = body.saturating_add(sz + (sz & 1));
    }
    None
}

#[cfg(target_os = "windows")]
fn play_wav_with_levels<F: FnMut(f32)>(
    wav: &[u8],
    my_seq: u64,
    started: &mut bool,
    on_start: &mut impl FnMut(),
    on_level: &mut F,
) {
    const SND_MEMORY: u32 = 0x0004;
    const SND_ASYNC: u32 = 0x0001;

    let Some((sr, ch, bits, data_start, data_end)) = parse_wav_pcm_info(wav) else {
        return;
    };
    let bytes_per_sec = (sr as usize) * (ch as usize) * (bits as usize / 8);
    if bytes_per_sec == 0 || data_end <= data_start {
        return;
    }
    let total_ms = ((data_end - data_start) * 1000 / bytes_per_sec).max(150) as u64;

    if !*started {
        *started = true;
        on_start();
    }
    unsafe {
        PlaySoundW(wav.as_ptr(), 0, SND_MEMORY | SND_ASYNC);
    }

    let t0 = Instant::now();
    loop {
        if STOP_REQ.load(Ordering::SeqCst) || SPEAK_SEQ.load(Ordering::SeqCst) != my_seq {
            unsafe {
                PlaySoundW(core::ptr::null(), 0, 0);
            }
            break;
        }
        let el_ms = t0.elapsed().as_millis() as u64;
        if el_ms >= total_ms {
            break;
        }
        if bits == 16 {
            let byte_off = data_start + (((el_ms as usize * bytes_per_sec) / 1000) & !1);
            let win_bytes = (bytes_per_sec / 20).max(256) & !1;
            let end = (byte_off + win_bytes).min(data_end);
            let mut sum_sq = 0.0f64;
            let mut cnt = 0usize;
            if byte_off + 2 <= end {
                for pair in wav[byte_off..end].as_chunks::<2>().0 {
                    let s = i16::from_le_bytes([pair[0], pair[1]]) as f64 / 32768.0;
                    sum_sq += s * s;
                    cnt += 1;
                }
            }
            let rms = if cnt > 0 { (sum_sq / cnt as f64).sqrt() as f32 } else { 0.0 };
            on_level((rms * 4.2).clamp(0.08, 1.0));
        } else {
            on_level(0.45);
        }
        std::thread::sleep(Duration::from_millis(65));
    }
    unsafe {
        PlaySoundW(core::ptr::null(), 0, 0);
    }
}

#[cfg(target_os = "windows")]
fn play_mp3_with_levels<F: FnMut(f32)>(
    mp3: &[u8],
    my_seq: u64,
    started: &mut bool,
    on_start: &mut impl FnMut(),
    on_level: &mut F,
    on_lease_refresh: &mut impl FnMut(),
) -> bool {
    cleanup_stale_tts_files();
    let path = temp_mp3_path(my_seq);
    if fs::write(&path, mp3).is_err() {
        return false;
    }

    mci_cmd("stop nexus_tts");
    mci_cmd("close nexus_tts");

    let open_cmd = format!("open \"{}\" type mpegvideo alias nexus_tts", path.to_string_lossy());
    if mci_cmd(&open_cmd) != 0 {
        let _ = fs::remove_file(&path);
        return false;
    }

    if !*started {
        *started = true;
        on_start();
    }
    if mci_cmd("play nexus_tts") != 0 {
        mci_cmd("close nexus_tts");
        let _ = fs::remove_file(&path);
        return false;
    }

    let t0 = Instant::now();
    let mut last_lease = Instant::now();
    loop {
        if STOP_REQ.load(Ordering::SeqCst) || SPEAK_SEQ.load(Ordering::SeqCst) != my_seq {
            break;
        }
        let mode = mci_query("status nexus_tts mode");
        if !mode.eq_ignore_ascii_case("playing") {
            break;
        }
        if last_lease.elapsed() >= Duration::from_secs(6) {
            on_lease_refresh();
            last_lease = Instant::now();
        }
        let t = t0.elapsed().as_secs_f32();
        let wave = 0.45 + 0.35 * (t * 9.5).sin().abs() * (0.65 + 0.35 * (t * 4.2).cos());
        on_level(wave.clamp(0.15, 0.96));
        std::thread::sleep(Duration::from_millis(70));
    }

    mci_cmd("stop nexus_tts");
    mci_cmd("close nexus_tts");
    let _ = fs::remove_file(&path);
    true
}

/// Speaks text naturally using `voice_style`:
/// - `"google_natural"` (default): Google Neural Voice (`en`/`hi`/`bn`) with Windows OneCore Male fallback.
/// - `"male_ravi"` / `"male_george"`: Direct Windows OneCore Male voice with real PCM RMS bar sync.
///
/// Drives `on_start`, `on_level`, and `on_lease_refresh` cleanly until finished or stopped.
pub fn speak_blocking(
    text: &str,
    voice_style: &str,
    mut on_start: impl FnMut(),
    mut on_level: impl FnMut(f32),
    mut on_lease_refresh: impl FnMut(),
) {
    let clean = text.trim();
    if clean.is_empty() {
        return;
    }
    STOP_REQ.store(false, Ordering::SeqCst);
    let my_seq = SPEAK_SEQ.fetch_add(1, Ordering::SeqCst) + 1;
    let chunks = split_speech_chunks(&clean.chars().take(420).collect::<String>());
    let lang = detect_tts_lang(clean);
    let mut started = false;

    #[cfg(target_os = "windows")]
    for chunk in chunks {
        if STOP_REQ.load(Ordering::SeqCst) || SPEAK_SEQ.load(Ordering::SeqCst) != my_seq {
            break;
        }
        let prefer_male = voice_style == "male_ravi" || voice_style == "male_george";
        if !prefer_male {
            if let Ok(mp3) = fetch_google_tts_mp3(&chunk, lang) {
                if play_mp3_with_levels(&mp3, my_seq, &mut started, &mut on_start, &mut on_level, &mut on_lease_refresh) {
                    continue;
                }
            }
        }
        if let Ok(wav) = synthesize_onecore_wav(&chunk, voice_style) {
            play_wav_with_levels(&wav, my_seq, &mut started, &mut on_start, &mut on_level);
        }
    }

    #[cfg(not(target_os = "windows"))]
    let _ = (chunks, lang, voice_style, &mut on_start, &mut on_level, &mut on_lease_refresh, &mut started);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_english_hinglish_hindi_and_bengali_for_tts() {
        assert_eq!(detect_tts_lang("Main ekdum badiya hoon yaar, kya haal hai?"), "hi");
        assert_eq!(detect_tts_lang("मैं बिल्कुल ठीक हूँ!"), "hi");
        assert_eq!(detect_tts_lang("আমি একদম ভালো আছি!"), "bn");
        assert_eq!(detect_tts_lang("Chrome is now open and ready."), "en");
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn onecore_male_voice_synthesizes_clean_riff_wav() {
        let wav = synthesize_onecore_wav("Hello there", "male_ravi");
        assert!(wav.is_ok(), "OneCore SpeechSynthesizer failed: {:?}", wav.err());
        let bytes = wav.unwrap();
        assert!(parse_wav_pcm_info(&bytes).is_some());
    }
}
