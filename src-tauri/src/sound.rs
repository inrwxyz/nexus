use std::sync::OnceLock;
use windows_sys::Win32::Media::Audio::{PlaySoundW, SND_ASYNC, SND_MEMORY, SND_NODEFAULT};

static WAKE_CHIME_WAV: OnceLock<Vec<u8>> = OnceLock::new();

/// Synthesize a subtle, warm, lightweight ambient assistant chime in-memory (RIFF WAVE, 44.1kHz 16-bit Mono).
/// Notes: C5 (523.25 Hz) -> G5 (783.99 Hz) with a smooth raised-cosine attack and fast natural decay.
/// Gentle gain (~0.28) ensures it is soft, pleasant, and non-intrusive ("halka").
pub fn synthesize_nexus_chime() -> Vec<u8> {
    let sample_rate: u32 = 44100;
    let duration_secs: f32 = 0.22;
    let total_samples = (sample_rate as f32 * duration_secs) as usize;
    let mut pcm: Vec<f32> = vec![0.0; total_samples];

    // Lightweight two-tone ambient droplet (C5 -> G5)
    let notes: [(f32, f32, f32, f32); 2] = [
        (523.25, 0.000, 0.45, 22.0), // C5: soft initial tap
        (783.99, 0.040, 0.65, 17.0), // G5: warm ascending harmonic cue
    ];

    let attack_len = (0.010 * sample_rate as f32) as usize;

    for (freq, delay, vol, decay_rate) in notes {
        let start = (delay * sample_rate as f32) as usize;
        for i in start..total_samples {
            let offset = i - start;
            let t = offset as f32 / sample_rate as f32;
            let attack = if offset < attack_len {
                0.5 * (1.0 - (std::f32::consts::PI * (offset as f32 / attack_len as f32)).cos())
            } else {
                1.0
            };
            let decay = (-t * decay_rate).exp();
            let sig = (2.0 * std::f32::consts::PI * freq * t).sin();
            pcm[i] += sig * vol * attack * decay;
        }
    }

    // Peak normalize and apply soft, gentle gain (0.28)
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
        let scaled = (sample / max_val) * 0.28;
        let clamped = (scaled * 32767.0).clamp(-32768.0, 32767.0) as i16;
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
