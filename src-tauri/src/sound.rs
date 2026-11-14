use std::sync::OnceLock;
use windows_sys::Win32::Media::Audio::{PlaySoundW, SND_ASYNC, SND_MEMORY, SND_NODEFAULT};

static WAKE_CHIME_WAV: OnceLock<Vec<u8>> = OnceLock::new();

/// Synthesize a smooth, high-fidelity ambient assistant chime in-memory (RIFF WAVE, 44.1kHz 16-bit Mono).
/// Notes: D5 (587.33 Hz) -> A5 (880.00 Hz) with a delicate harmonic sparkle and zero click/pop.
pub fn synthesize_nexus_chime() -> Vec<u8> {
    let sample_rate: u32 = 44100;
    let duration_secs: f32 = 0.38;
    let total_samples = (sample_rate as f32 * duration_secs) as usize;
    let mut pcm: Vec<f32> = vec![0.0; total_samples];

    // Modern ascending two-tone glass chime (D5 -> A5)
    let notes: [(f32, f32, f32); 2] = [
        (587.33, 0.000, 0.32), // D5: warm, pleasant introductory tone
        (880.00, 0.080, 0.42), // A5: bright, clear ascending confirmation
    ];

    for (freq, delay, vol) in notes {
        let start = (delay * sample_rate as f32) as usize;
        for i in start..total_samples {
            let t = (i - start) as f32 / sample_rate as f32;
            let attack = (t / 0.006).min(1.0);
            let decay = (-t * 7.5).exp();
            // Fundamental sine wave + soft 2nd harmonic (18%) for warm acoustic bell character
            let sig = (2.0 * std::f32::consts::PI * freq * t).sin()
                + 0.18 * (4.0 * std::f32::consts::PI * freq * t).sin();
            pcm[i] += sig * vol * attack * decay;
        }
    }

    // Peak normalize to prevent any digital distortion
    let mut max_val = 0.0f32;
    for &sample in &pcm {
        let abs = sample.abs();
        if abs > max_val {
            max_val = abs;
        }
    }
    if max_val == 0.0 {
        max_val = 1.0;
    }

    let mut raw_pcm = Vec::with_capacity(total_samples * 2);
    for sample in pcm {
        let normalized = (sample / max_val) * 0.85;
        let clamped = (normalized * 32767.0).clamp(-32768.0, 32767.0) as i16;
        raw_pcm.extend_from_slice(&clamped.to_le_bytes());
    }

    let data_size = raw_pcm.len() as u32;
    let file_size = 36 + data_size;

    let mut wav = Vec::with_capacity(44 + raw_pcm.len());
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&file_size.to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    wav.extend_from_slice(&16u32.to_le_bytes()); // Subchunk1Size (16 for PCM)
    wav.extend_from_slice(&1u16.to_le_bytes());  // AudioFormat (1 = PCM)
    wav.extend_from_slice(&1u16.to_le_bytes());  // NumChannels (1 = Mono)
    wav.extend_from_slice(&sample_rate.to_le_bytes());
    wav.extend_from_slice(&(sample_rate * 2).to_le_bytes()); // ByteRate
    wav.extend_from_slice(&2u16.to_le_bytes());  // BlockAlign (16-bit mono = 2)
    wav.extend_from_slice(&16u16.to_le_bytes()); // BitsPerSample
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&data_size.to_le_bytes());
    wav.extend_from_slice(&raw_pcm);

    wav
}

/// Plays the unique Nexus wake sound asynchronously without blocking.
/// Uses Win32 PlaySoundW with SND_MEMORY | SND_ASYNC | SND_NODEFAULT.
pub fn play_wake_chime() {
    let wav = WAKE_CHIME_WAV.get_or_init(synthesize_nexus_chime);
    unsafe {
        PlaySoundW(
            wav.as_ptr() as *const u16,
            std::ptr::null_mut(),
            SND_MEMORY | SND_ASYNC | SND_NODEFAULT,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_chime_wav_header() {
        let wav = synthesize_nexus_chime();
        assert!(wav.len() > 44);
        assert_eq!(&wav[0..4], b"RIFF");
        assert_eq!(&wav[8..12], b"WAVE");
        assert_eq!(&wav[12..16], b"fmt ");
        assert_eq!(&wav[36..40], b"data");
    }
}
