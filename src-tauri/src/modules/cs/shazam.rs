// cs/shazam.rs — Windows WASAPI Loopback audio capture (4s) + Shazam v1 landmark signature + SMTC fallback.

use base64::Engine;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::FromSample;
use serde::Serialize;
use std::f64::consts::PI;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use uuid::Uuid;
use windows::Media::Control::{
    GlobalSystemMediaTransportControlsSessionManager,
    GlobalSystemMediaTransportControlsSessionPlaybackStatus,
};
use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_MULTITHREADED};

#[derive(Debug, Clone, Serialize)]
pub struct MusicRecognitionResult {
    pub found: bool,
    pub title: String,
    pub artist: String,
    pub cover_url: Option<String>,
    pub search_url: String,
    pub source: String,
    pub message: String,
}

pub struct CapturedAudio {
    pub samples_16k: Vec<i16>,
    pub peak: f32,
}

fn build_loopback_stream<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    buf: Arc<Mutex<Vec<f32>>>,
    ch: usize,
    max_len: usize,
) -> Result<cpal::Stream, String>
where
    T: cpal::SizedSample,
    f32: FromSample<T>,
{
    device
        .build_input_stream(
            config,
            move |data: &[T], _| {
                let mut g = buf.lock().unwrap();
                if g.len() >= max_len {
                    return;
                }
                for frame in data.chunks(ch.max(1)) {
                    let sum: f32 = frame.iter().map(|&s| f32::from_sample_(s)).sum();
                    g.push(sum / frame.len().max(1) as f32);
                }
            },
            |err| eprintln!("WASAPI loopback error: {err}"),
            None,
        )
        .map_err(|e| format!("Failed to open WASAPI loopback stream: {e}"))
}

fn resample_to_16k(input: &[f32], src_sr: u32) -> (Vec<i16>, f32) {
    if input.is_empty() || src_sr == 0 {
        return (Vec::new(), 0.0);
    }
    let ratio = src_sr as f64 / 16000.0;
    let out_len = (input.len() as f64 / ratio) as usize;
    if out_len == 0 {
        return (Vec::new(), 0.0);
    }

    // Decimate with symmetric triangular weighting when src_sr > 16 kHz to suppress aliasing in Shazam bands,
    // then normalize quiet browser/Reels audio safely before 16-bit quantization.
    let half_span = if ratio > 1.25 { (ratio * 0.5).ceil() as isize } else { 0 };
    let mut tmp = Vec::with_capacity(out_len);
    let mut peak = 0.0f32;
    let last_idx = input.len() as isize - 1;

    for i in 0..out_len {
        let pos = i as f64 * ratio;
        let v = if half_span == 0 {
            let idx = (pos as usize).min(input.len() - 1);
            let next = (idx + 1).min(input.len() - 1);
            let frac = (pos - idx as f64) as f32;
            input[idx] * (1.0 - frac) + input[next] * frac
        } else {
            let center = pos.round() as isize;
            let mut acc = 0.0f64;
            let mut wsum = 0.0f64;
            let span_f = (half_span as f64) + 0.5;
            for d in -half_span..=half_span {
                let idx = (center + d).clamp(0, last_idx) as usize;
                let w = (1.0 - ((center + d) as f64 - pos).abs() / span_f).max(0.0);
                acc += input[idx] as f64 * w;
                wsum += w;
            }
            if wsum > 0.0 {
                (acc / wsum) as f32
            } else {
                input[center.clamp(0, last_idx) as usize]
            }
        };
        peak = peak.max(v.abs());
        tmp.push(v);
    }

    let gain = if peak > 0.0005 && peak < 0.45 {
        (0.55 / peak).min(12.0)
    } else {
        1.0
    };
    let out = tmp
        .into_iter()
        .map(|v| ((v * gain).clamp(-1.0, 1.0) * 32767.0) as i16)
        .collect();
    (out, peak)
}

/// Captures ~6 seconds of internal desktop render audio via Windows WASAPI Loopback.
pub fn capture_system_audio_4s() -> Result<CapturedAudio, String> {
    let host = cpal::default_host();
    let device = host
        .default_output_device()
        .ok_or_else(|| "No default Windows playback device found".to_string())?;
    let default_cfg = device
        .default_output_config()
        .map_err(|e| format!("Failed to query output format: {e}"))?;

    let sample_rate = default_cfg.sample_rate().0;
    let channels = default_cfg.channels() as usize;
    let stream_cfg: cpal::StreamConfig = default_cfg.clone().into();
    let target_len = (sample_rate as usize) * 6;
    let max_samples = (sample_rate as usize) * 7;
    let buf = Arc::new(Mutex::new(Vec::with_capacity(max_samples)));

    let stream = match default_cfg.sample_format() {
        cpal::SampleFormat::F32 => {
            build_loopback_stream::<f32>(&device, &stream_cfg, buf.clone(), channels, max_samples)?
        }
        cpal::SampleFormat::I16 => {
            build_loopback_stream::<i16>(&device, &stream_cfg, buf.clone(), channels, max_samples)?
        }
        cpal::SampleFormat::U16 => {
            build_loopback_stream::<u16>(&device, &stream_cfg, buf.clone(), channels, max_samples)?
        }
        other => return Err(format!("Unsupported loopback sample format: {other:?}")),
    };

    stream
        .play()
        .map_err(|e| format!("Failed to start WASAPI loopback capture: {e}"))?;

    let start = Instant::now();
    while start.elapsed() < Duration::from_millis(6200) {
        if buf.lock().unwrap().len() >= target_len {
            break;
        }
        std::thread::sleep(Duration::from_millis(60));
    }
    drop(stream);

    let captured = buf.lock().unwrap().clone();
    let (samples_16k, peak) = resample_to_16k(&captured, sample_rate);
    Ok(CapturedAudio { samples_16k, peak })
}

// ─── Minimal Radix-2 Cooley-Tukey Real FFT (N = 2048) & Shazam v1 Signature ───────────

#[derive(Clone, Copy)]
struct Cpx {
    re: f64,
    im: f64,
}

fn fft_2048_mag_sq(windowed: &[f64; 2048]) -> [f64; 1025] {
    let mut a = [Cpx { re: 0.0, im: 0.0 }; 2048];
    for (i, &s) in windowed.iter().enumerate() {
        a[i].re = s;
    }

    // Bit-reversal permutation (11 bits for N = 2048)
    let mut j = 0usize;
    for i in 1..2048 {
        let mut bit = 1024usize;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j ^= bit;
        if i < j {
            a.swap(i, j);
        }
    }

    let mut len = 2usize;
    while len <= 2048 {
        let half = len / 2;
        let ang = -2.0 * PI / (len as f64);
        let wlen = Cpx {
            re: ang.cos(),
            im: ang.sin(),
        };
        let mut i = 0;
        while i < 2048 {
            let mut w = Cpx { re: 1.0, im: 0.0 };
            for k in 0..half {
                let u = a[i + k];
                let v0 = a[i + k + half];
                let v = Cpx {
                    re: v0.re * w.re - v0.im * w.im,
                    im: v0.re * w.im + v0.im * w.re,
                };
                a[i + k] = Cpx {
                    re: u.re + v.re,
                    im: u.im + v.im,
                };
                a[i + k + half] = Cpx {
                    re: u.re - v.re,
                    im: u.im - v.im,
                };
                w = Cpx {
                    re: w.re * wlen.re - w.im * wlen.im,
                    im: w.re * wlen.im + w.im * wlen.re,
                };
            }
            i += len;
        }
        len <<= 1;
    }

    let mut mag = [0.0f64; 1025];
    for (k, slot) in mag.iter_mut().enumerate() {
        let c = a[k];
        *slot = ((c.re * c.re + c.im * c.im) / ((1i64 << 17) as f64)).max(1e-10);
    }
    mag
}

struct FreqPeak {
    fft_pass: u32,
    peak_magnitude: u32,
    corrected_bin: u32,
}

/// Builds a standard Shazam v1 landmark signature (`data:audio/vnd.shazam.sig;base64,...`)
/// matching Shazam's 16kHz / 2048-FFT / 128-hop landmark constellation & TLV format.
pub fn build_shazam_signature(samples_16k: &[i16]) -> String {
    let chunk_count = samples_16k.len() / 128;
    let processed_samples = (chunk_count * 128) as u32;

    let mut hann = [0.0f64; 2048];
    for (i, slot) in hann.iter_mut().enumerate() {
        *slot = 0.5 * (1.0 - (2.0 * PI * ((i + 1) as f64) / 2049.0).cos());
    }

    let mut ring_buf = [0i16; 2048];
    let mut ring_idx: usize = 0;

    let mut fft_outputs = vec![[0.0f64; 1025]; 256];
    let mut fft_idx: u8 = 0;

    let mut spread_outputs = vec![[0.0f64; 1025]; 256];
    let mut spread_idx: u8 = 0;
    let mut num_spread_done: u32 = 0;

    let mut bands: [Vec<FreqPeak>; 4] = [Vec::new(), Vec::new(), Vec::new(), Vec::new()];

    for chunk in samples_16k.chunks_exact(128) {
        // 1) Ring-buffer FFT every 128 new 16 kHz samples
        ring_buf[ring_idx..ring_idx + 128].copy_from_slice(chunk);
        ring_idx = (ring_idx + 128) & 2047;

        let mut windowed = [0.0f64; 2048];
        for (i, &w) in hann.iter().enumerate() {
            windowed[i] = ring_buf[(i + ring_idx) & 2047] as f64 * w;
        }
        fft_outputs[fft_idx as usize] = fft_2048_mag_sq(&windowed);
        fft_idx = fft_idx.wrapping_add(1);

        // 2) Peak spreading across neighboring frequency bins & recent time slots [1, 3, 6]
        let real_fft = &fft_outputs[fft_idx.wrapping_sub(1) as usize];
        let cur_spread = &mut spread_outputs[spread_idx as usize];
        cur_spread.copy_from_slice(real_fft);
        for pos in 0..=1022 {
            cur_spread[pos] = cur_spread[pos]
                .max(cur_spread[pos + 1])
                .max(cur_spread[pos + 2]);
        }
        let spread_copy = *cur_spread;
        for former in [1u8, 3, 6] {
            let slot = &mut spread_outputs[spread_idx.wrapping_sub(former) as usize];
            for pos in 0..=1024 {
                slot[pos] = slot[pos].max(spread_copy[pos]);
            }
        }
        spread_idx = spread_idx.wrapping_add(1);
        num_spread_done += 1;

        // 3) Peak recognition looking 46 hops back against dilated neighborhood
        if num_spread_done >= 46 {
            let fft_minus_46 = &fft_outputs[fft_idx.wrapping_sub(46) as usize];
            let fft_minus_49 = &spread_outputs[spread_idx.wrapping_sub(49) as usize];

            for bin_pos in 10..=1014usize {
                let cur_val = fft_minus_46[bin_pos];
                if cur_val < 1.0 / 64.0 || cur_val < fft_minus_49[bin_pos - 1] {
                    continue;
                }

                let mut max_freq_nbr = 0.0f64;
                for &nbr_off in &[-10isize, -7, -4, -3, 1, 2, 5, 8] {
                    let b = (bin_pos as isize + nbr_off) as usize;
                    max_freq_nbr = max_freq_nbr.max(fft_minus_49[b]);
                }
                if cur_val <= max_freq_nbr {
                    continue;
                }

                let mut max_time_nbr = max_freq_nbr;
                for &time_off in &[
                    -53isize, -45, 165, 172, 179, 186, 193, 200, 214, 221, 228, 235, 242, 249,
                ] {
                    let s_idx = ((spread_idx as isize + time_off) & 255) as usize;
                    max_time_nbr = max_time_nbr.max(spread_outputs[s_idx][bin_pos - 1]);
                }
                if cur_val <= max_time_nbr {
                    continue;
                }

                let fft_pass = num_spread_done - 46;
                let peak_mag = cur_val.ln().max(1.0 / 64.0) * 1477.3 + 6144.0;
                let peak_before = fft_minus_46[bin_pos - 1].ln().max(1.0 / 64.0) * 1477.3 + 6144.0;
                let peak_after = fft_minus_46[bin_pos + 1].ln().max(1.0 / 64.0) * 1477.3 + 6144.0;

                let var1 = peak_mag * 2.0 - peak_before - peak_after;
                if var1 <= 0.0 {
                    continue;
                }
                let var2 = (peak_after - peak_before) * 32.0 / var1;
                let corrected_bin = ((bin_pos as i32 * 64) + (var2 as i32)).clamp(0, 65535) as u32;
                let freq_hz = corrected_bin as f64 * (16000.0 / 2.0 / 1024.0 / 64.0);

                let band_id = match freq_hz as i32 {
                    250..=519 => 0,
                    520..=1449 => 1,
                    1450..=3499 => 2,
                    3500..=5500 => 3,
                    _ => continue,
                };

                bands[band_id].push(FreqPeak {
                    fft_pass,
                    peak_magnitude: peak_mag.clamp(0.0, 65535.0) as u32,
                    corrected_bin,
                });
            }
        }
    }

    // Encode TLV bands cleanly
    let mut contents = Vec::new();
    for (i, band) in bands.iter().enumerate() {
        if band.is_empty() {
            continue;
        }
        let mut band_buf = Vec::new();
        let mut cur_pass = 0u32;
        for pk in band {
            if pk.fft_pass - cur_pass >= 255 {
                band_buf.push(0xFF);
                band_buf.extend_from_slice(&pk.fft_pass.to_le_bytes());
                cur_pass = pk.fft_pass;
            }
            band_buf.push((pk.fft_pass - cur_pass) as u8);
            band_buf.extend_from_slice(&(pk.peak_magnitude as u16).to_le_bytes());
            band_buf.extend_from_slice(&(pk.corrected_bin as u16).to_le_bytes());
            cur_pass = pk.fft_pass;
        }

        let unpadded_len = band_buf.len() as u32;
        while !band_buf.len().is_multiple_of(4) {
            band_buf.push(0);
        }
        let section_id: u32 = 0x60030040 + i as u32;
        contents.extend_from_slice(&section_id.to_le_bytes());
        contents.extend_from_slice(&unpadded_len.to_le_bytes());
        contents.extend_from_slice(&band_buf);
    }

    // Total message size after the 48-byte RawSignatureHeader = 8 (0x40000000 + size) + contents.len()
    let size_minus_header = (contents.len() + 8) as u32;
    let num_samples_field = processed_samples + (16000.0 * 0.24) as u32;

    let mut out = Vec::with_capacity(48 + 8 + contents.len());
    out.extend_from_slice(&0xCAFE2580u32.to_le_bytes()); // magic1
    out.extend_from_slice(&0u32.to_le_bytes());          // crc32 placeholder
    out.extend_from_slice(&size_minus_header.to_le_bytes());
    out.extend_from_slice(&0x94119C00u32.to_le_bytes()); // magic2
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&(3u32 << 27).to_le_bytes());  // sample rate ID 3 = 16000 Hz
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&num_samples_field.to_le_bytes());
    out.extend_from_slice(&((15u32 << 19) + 0x40000).to_le_bytes());

    out.extend_from_slice(&0x40000000u32.to_le_bytes());
    out.extend_from_slice(&size_minus_header.to_le_bytes());
    out.extend_from_slice(&contents);

    let mut hasher = crc32fast::Hasher::new();
    hasher.update(&out[8..]);
    let crc = hasher.finalize();
    out[4..8].copy_from_slice(&crc.to_le_bytes());

    let encoded = base64::engine::general_purpose::STANDARD.encode(&out);
    format!("data:audio/vnd.shazam.sig;base64,{encoded}")
}

fn url_encode_q(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 3);
    for &b in s.as_bytes() {
        if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) {
            out.push(b as char);
        } else if b == b' ' {
            out.push('+');
        } else {
            out.push_str(&format!("%{:02X}", b));
        }
    }
    out
}

/// Filters out generic website / browser / app names (e.g. "Instagram", "Chrome", "TikTok")
/// that Chrome reports via SMTC when playing a feed video without actual song metadata.
pub fn is_generic_smtc_title(title: &str, artist: &str) -> bool {
    let t = title.trim().to_lowercase();
    let a = artist.trim().to_lowercase();
    if t.is_empty() {
        return true;
    }

    const GENERIC_NAMES: &[&str] = &[
        "instagram",
        "instagram reel",
        "instagram reels",
        "instagram video",
        "reels",
        "reel",
        "tiktok",
        "facebook",
        "facebook watch",
        "watch",
        "x",
        "twitter",
        "reddit",
        "twitch",
        "youtube",
        "youtube shorts",
        "shorts",
        "google chrome",
        "chrome",
        "microsoft edge",
        "edge",
        "brave",
        "firefox",
        "opera",
        "vlc media player",
        "media player",
        "video",
        "audio",
        "unknown",
    ];

    let app_like_artist = a.is_empty()
        || a == t
        || matches!(
            a.as_str(),
            "instagram" | "tiktok" | "facebook" | "google chrome" | "chrome" | "microsoft edge" | "brave" | "firefox"
        );

    if app_like_artist
        && (GENERIC_NAMES.contains(&t.as_str())
            || t.starts_with("instagram")
            || t.starts_with("tiktok")
            || t.contains("on instagram")
            || t.ends_with("- instagram")
            || t.ends_with("| instagram"))
    {
        return true;
    }

    false
}

/// Checks Windows Active Media Session (SMTC) for currently playing audio metadata as a fallback.
/// Only returns real track titles (never generic site labels like "Instagram" or "Chrome").
pub fn query_active_smtc_track() -> Option<(String, String)> {
    let init_ok = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED).is_ok() };
    let res = (|| {
        let mgr = GlobalSystemMediaTransportControlsSessionManager::RequestAsync()
            .ok()?
            .get()
            .ok()?;
        let sessions = mgr.GetSessions().ok()?;
        let count = sessions.Size().ok()?;
        for i in 0..count {
            let sess = sessions.GetAt(i).ok()?;
            let playing = sess
                .GetPlaybackInfo()
                .ok()
                .and_then(|p| p.PlaybackStatus().ok())
                == Some(GlobalSystemMediaTransportControlsSessionPlaybackStatus::Playing);
            if !playing {
                continue;
            }
            let props = sess.TryGetMediaPropertiesAsync().ok()?.get().ok()?;
            let raw_title = props.Title().ok()?.to_string_lossy().trim().to_string();
            let raw_artist = props
                .Artist()
                .ok()
                .map(|a| a.to_string_lossy().trim().to_string())
                .unwrap_or_default();
            if !raw_title.is_empty() {
                let title = raw_title
                    .trim_end_matches(" - YouTube")
                    .trim_end_matches(" | YouTube")
                    .trim_end_matches(" - Google Chrome")
                    .trim_end_matches(" - Instagram")
                    .trim_end_matches(" | Instagram")
                    .trim()
                    .to_string();
                if is_generic_smtc_title(&title, &raw_artist) {
                    continue;
                }
                return Some((title, raw_artist));
            }
        }
        None
    })();
    if init_ok {
        unsafe { CoUninitialize() };
    }
    res
}

pub async fn query_shazam(uri: String, sample_ms: u64) -> Result<MusicRecognitionResult, String> {
    let u1 = Uuid::new_v4().to_string().to_uppercase();
    let u2 = Uuid::new_v4().to_string();
    let url = format!(
        "https://amp.shazam.com/discovery/v5/en/US/android/-/tag/{u1}/{u2}?sync=true&webv3=true&sampling=true&connected=&shazamapiversion=v3&sharehub=true&video=v3"
    );
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u32)
        .unwrap_or(0);

    let body = serde_json::json!({
        "geolocation": { "altitude": 300, "latitude": 45, "longitude": 2 },
        "signature": { "samplems": sample_ms, "timestamp": ts, "uri": uri },
        "timestamp": ts,
        "timezone": "Europe/Paris"
    });

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .map_err(|e| format!("HTTP client error: {e}"))?;

    let resp = client
        .post(&url)
        .header(
            "User-Agent",
            "Dalvik/2.1.0 (Linux; U; Android 11; Pixel 5 Build/RQ3A.210805.001.A1)",
        )
        .header("Content-Language", "en_US")
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("Shazam API error: {e}"))?;

    let json: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("Shazam response parse error: {e}"))?;

    if let Some(track) = json.get("track") {
        let title = track
            .get("title")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim()
            .to_string();
        let artist = track
            .get("subtitle")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim()
            .to_string();
        let cover_url = track
            .get("images")
            .and_then(|im| im.get("coverart").or_else(|| im.get("background")))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        if !title.is_empty() {
            let query = if artist.is_empty() {
                title.clone()
            } else {
                format!("{artist} {title}")
            };
            let display = if artist.is_empty() {
                title.clone()
            } else {
                format!("{artist} - {title}")
            };
            let google_url = format!(
                "https://www.google.com/search?q={}",
                url_encode_q(&query)
            );
            return Ok(MusicRecognitionResult {
                found: true,
                title,
                artist,
                cover_url,
                search_url: google_url,
                source: "shazam".to_string(),
                message: format!("Recognized via Shazam: {display}"),
            });
        }
    }

    Ok(MusicRecognitionResult {
        found: false,
        title: String::new(),
        artist: String::new(),
        cover_url: None,
        search_url: String::new(),
        source: "shazam".to_string(),
        message: "No Shazam match".to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_valid_shazam_signature_header() {
        // 5 seconds of changing 3-note chords at 16 kHz (emits landmarks across bands + 0xFF jumps)
        let chords: &[(f64, f64, f64)] = &[
            (330.0, 660.0, 1760.0),
            (440.0, 880.0, 2200.0),
            (523.25, 1046.5, 2637.0),
            (587.33, 1174.66, 2936.65),
            (659.25, 1318.5, 3951.0),
            (740.0, 1480.0, 4435.0),
        ];
        let mut samples = Vec::with_capacity(80000);
        for i in 0..80000 {
            let t = i as f64 / 16000.0;
            let note = i / 6400; // switch chord every 0.4s
            let (f1, f2, f3) = chords[note % chords.len()];
            // Envelope pulse per chord so each transient produces sharp temporal peaks
            let pos = (i % 6400) as f64 / 6400.0;
            let env = if pos < 0.25 { 1.0 } else { 0.2 };
            let v = ((2.0 * PI * f1 * t).sin()
                + 0.75 * (2.0 * PI * f2 * t).sin()
                + 0.55 * (2.0 * PI * f3 * t).sin())
                * (12000.0 * env);
            samples.push(v as i16);
        }
        let sig = build_shazam_signature(&samples);
        assert!(sig.starts_with("data:audio/vnd.shazam.sig;base64,"));
        let b64 = sig.strip_prefix("data:audio/vnd.shazam.sig;base64,").unwrap();
        let raw = base64::engine::general_purpose::STANDARD.decode(b64).unwrap();
        assert!(raw.len() > 56);
        assert_eq!(u32::from_le_bytes(raw[0..4].try_into().unwrap()), 0xCAFE2580);
        let size_minus_hdr = u32::from_le_bytes(raw[8..12].try_into().unwrap()) as usize;
        assert_eq!(size_minus_hdr, raw.len() - 48);
        assert_eq!(u32::from_le_bytes(raw[48..52].try_into().unwrap()), 0x40000000);
        assert_eq!(
            u32::from_le_bytes(raw[52..56].try_into().unwrap()) as usize,
            raw.len() - 48
        );
        let stored_crc = u32::from_le_bytes(raw[4..8].try_into().unwrap());
        let mut h = crc32fast::Hasher::new();
        h.update(&raw[8..]);
        assert_eq!(stored_crc, h.finalize());

        // Walk every TLV band & decode every FrequencyPeak cleanly (verifying 0xFF jumps + padding)
        let mut pos = 56usize;
        let mut total_peaks = 0usize;
        while pos < raw.len() {
            let band_id = u32::from_le_bytes(raw[pos..pos + 4].try_into().unwrap());
            let band_size = u32::from_le_bytes(raw[pos + 4..pos + 8].try_into().unwrap()) as usize;
            assert!((0x60030040..=0x60030043).contains(&band_id));
            pos += 8;
            let mut bpos = pos;
            let bend = pos + band_size;
            while bpos < bend {
                let d = raw[bpos];
                bpos += 1;
                if d == 0xFF {
                    bpos += 4;
                } else {
                    bpos += 4; // mag u16 + bin u16
                    total_peaks += 1;
                }
            }
            assert_eq!(bpos, bend);
            let pad = (4 - (band_size % 4)) % 4;
            pos = bend + pad;
        }
        assert_eq!(pos, raw.len());
        assert!(total_peaks > 5, "Expected landmarks from multi-tone signal, got {total_peaks}");
    }

    #[test]
    fn filters_generic_browser_and_instagram_smtc_titles() {
        assert!(is_generic_smtc_title("Instagram", ""));
        assert!(is_generic_smtc_title("Instagram Reels", ""));
        assert!(is_generic_smtc_title("Reel by user - Instagram", "Instagram"));
        assert!(is_generic_smtc_title("TikTok", ""));
        assert!(is_generic_smtc_title("Google Chrome", ""));
        assert!(!is_generic_smtc_title("Kesariya", "Arijit Singh"));
        assert!(!is_generic_smtc_title("Lofi Hip Hop Radio - Beats to Relax", ""));
    }
}
