//! Native low-latency WASAPI audio output via `cpal` for Nexus Screen Mirror PCM streams.
//! Handles device sample-rate mismatches via linear resampling and enforces a
//! tight max-buffer latency cap with smooth crossfade trimming so audio never drifts.

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::SampleFormat;
use nexus_mirror_engine::protocol::stream::AudioPacket;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;

const SRC_RATE: f64 = 48_000.0;
const SRC_RATE_U32: usize = 48_000;

struct RingState {
    frames: VecDeque<(f32, f32)>,
    target_frames: usize,
    max_frames: usize,
    buffering: bool,
    phase: f64,
    last_sample: (f32, f32),
    prev_out: (f32, f32),
}

impl RingState {
    fn new(buffer_ms: u16) -> Self {
        let clamped_ms = (buffer_ms as usize).clamp(20, 250);
        let target_frames = (SRC_RATE_U32 * clamped_ms) / 1000;
        let max_frames = ((SRC_RATE_U32 * (clamped_ms + 35)) / 1000).max(target_frames + 960);
        Self {
            frames: VecDeque::with_capacity(max_frames + 2048),
            target_frames,
            max_frames,
            buffering: true,
            phase: 0.0,
            last_sample: (0.0, 0.0),
            prev_out: (0.0, 0.0),
        }
    }

    fn push_s16le(&mut self, bytes: &[u8]) {
        for chunk in bytes.as_chunks::<4>().0 {
            let l = i16::from_le_bytes([chunk[0], chunk[1]]) as f32 / 32768.0;
            let r = i16::from_le_bytes([chunk[2], chunk[3]]) as f32 / 32768.0;
            self.frames.push_back((l, r));
        }

        // Enforce strict upper latency bound by draining oldest frames smoothly.
        if self.frames.len() > self.max_frames {
            let excess = self.frames.len() - self.target_frames;
            if excess > 0 && self.frames.len() > excess + 16 {
                let tail = self.frames[excess - 1];
                self.frames.drain(..excess);
                for i in 0..16 {
                    let alpha = (i + 1) as f32 / 17.0;
                    if let Some(s) = self.frames.get_mut(i) {
                        s.0 = tail.0 * (1.0 - alpha) + s.0 * alpha;
                        s.1 = tail.1 * (1.0 - alpha) + s.1 * alpha;
                    }
                }
            }
        }

        if self.buffering && self.frames.len() >= self.target_frames {
            self.buffering = false;
        }
    }

    fn pull_next_stereo(&mut self, step: f64, muted: bool) -> (f32, f32) {
        if self.buffering {
            if self.frames.len() < self.target_frames.min(960) {
                return (0.0, 0.0);
            }
            self.buffering = false;
        }

        if self.frames.is_empty() {
            self.buffering = true;
            // Smooth micro-decay on underrun to avoid hard click.
            self.prev_out = (self.prev_out.0 * 0.85, self.prev_out.1 * 0.85);
            return if muted { (0.0, 0.0) } else { self.prev_out };
        }

        while self.phase >= 1.0 {
            if let Some(s) = self.frames.pop_front() {
                self.last_sample = s;
                self.phase -= 1.0;
            } else {
                self.phase = 0.0;
                break;
            }
        }

        let (l, r) = if let Some(&next) = self.frames.front() {
            let frac = self.phase as f32;
            (
                self.last_sample.0 + (next.0 - self.last_sample.0) * frac,
                self.last_sample.1 + (next.1 - self.last_sample.1) * frac,
            )
        } else {
            self.last_sample
        };

        self.phase += step;
        self.prev_out = (l, r);

        if muted {
            (0.0, 0.0)
        } else {
            (l, r)
        }
    }
}

/// Thread-safe handle to feed decoded S16LE 48kHz stereo PCM frames into WASAPI.
#[derive(Clone)]
pub struct PcmAudioSink {
    ring: Arc<Mutex<RingState>>,
    muted: Arc<AtomicBool>,
}

impl PcmAudioSink {
    pub fn push_s16le(&self, pcm_bytes: &[u8]) {
        if pcm_bytes.is_empty() {
            return;
        }
        if let Ok(mut guard) = self.ring.lock() {
            guard.push_s16le(pcm_bytes);
        }
    }

    pub fn set_muted(&self, muted: bool) {
        self.muted.store(muted, Ordering::Relaxed);
    }
}

pub struct NativeAudioPlayer {
    stop_flag: Arc<AtomicBool>,
    sink: PcmAudioSink,
    worker: Option<std::thread::JoinHandle<()>>,
}

impl NativeAudioPlayer {
    pub fn sink(&self) -> PcmAudioSink {
        self.sink.clone()
    }
}

impl Drop for NativeAudioPlayer {
    fn drop(&mut self) {
        self.stop_flag.store(true, Ordering::Relaxed);
        if let Some(h) = self.worker.take() {
            let _ = h.join();
        }
    }
}

/// Starts a shared WASAPI playback thread and returns a `PcmAudioSink` for any codec decoder.
pub fn start_pcm_player(buffer_ms: u16, muted: Arc<AtomicBool>) -> Option<NativeAudioPlayer> {
    let host = cpal::default_host();
    let device = host.default_output_device()?;
    let supported = device.default_output_config().ok()?;
    let config = supported.config();
    let sample_format = supported.sample_format();
    let device_rate = config.sample_rate.0.max(8000) as f64;
    let step = SRC_RATE / device_rate;
    let channels = config.channels as usize;

    let ring = Arc::new(Mutex::new(RingState::new(buffer_ms)));
    let sink = PcmAudioSink {
        ring: ring.clone(),
        muted: muted.clone(),
    };
    let stop_flag = Arc::new(AtomicBool::new(false));
    let stop_worker = stop_flag.clone();
    let ring_cb = ring.clone();
    let muted_cb = muted.clone();

    let (ready_tx, ready_rx) = std::sync::mpsc::channel::<bool>();

    let worker = std::thread::Builder::new()
        .name("nexus-audio-wasapi".to_string())
        .spawn(move || {
            let err_cb = |err: cpal::StreamError| {
                eprintln!("[nexus-audio] WASAPI stream error: {err}");
            };

            let stream_res = match sample_format {
                SampleFormat::F32 => device.build_output_stream(
                    &config,
                    move |out: &mut [f32], _| {
                        let is_muted = muted_cb.load(Ordering::Relaxed);
                        if let Ok(mut guard) = ring_cb.lock() {
                            for frame in out.chunks_mut(channels) {
                                let (l, r) = guard.pull_next_stereo(step, is_muted);
                                write_stereo_f32(frame, l, r);
                            }
                        } else {
                            out.fill(0.0);
                        }
                    },
                    err_cb,
                    None,
                ),
                SampleFormat::I16 => device.build_output_stream(
                    &config,
                    move |out: &mut [i16], _| {
                        let is_muted = muted_cb.load(Ordering::Relaxed);
                        if let Ok(mut guard) = ring_cb.lock() {
                            for frame in out.chunks_mut(channels) {
                                let (l, r) = guard.pull_next_stereo(step, is_muted);
                                write_stereo_i16(frame, l, r);
                            }
                        } else {
                            out.fill(0);
                        }
                    },
                    err_cb,
                    None,
                ),
                SampleFormat::U16 => device.build_output_stream(
                    &config,
                    move |out: &mut [u16], _| {
                        let is_muted = muted_cb.load(Ordering::Relaxed);
                        if let Ok(mut guard) = ring_cb.lock() {
                            for frame in out.chunks_mut(channels) {
                                let (l, r) = guard.pull_next_stereo(step, is_muted);
                                write_stereo_u16(frame, l, r);
                            }
                        } else {
                            out.fill(32768);
                        }
                    },
                    err_cb,
                    None,
                ),
                _ => {
                    let _ = ready_tx.send(false);
                    return;
                }
            };

            let Ok(stream) = stream_res else {
                let _ = ready_tx.send(false);
                return;
            };
            if stream.play().is_err() {
                let _ = ready_tx.send(false);
                return;
            }
            let _ = ready_tx.send(true);

            while !stop_worker.load(Ordering::Relaxed) {
                std::thread::sleep(std::time::Duration::from_millis(25));
            }
        })
        .ok()?;

    if !ready_rx.recv().unwrap_or(false) {
        stop_flag.store(true, Ordering::Relaxed);
        let _ = worker.join();
        return None;
    }

    Some(NativeAudioPlayer {
        stop_flag,
        sink,
        worker: Some(worker),
    })
}

/// Convenience wrapper for uncompressed S16LE `raw` `AudioPacket` streams.
pub fn start_raw_player(
    mut rx: mpsc::Receiver<AudioPacket>,
    buffer_ms: u16,
    muted: Arc<AtomicBool>,
) -> Option<NativeAudioPlayer> {
    let player = start_pcm_player(buffer_ms, muted)?;
    let sink = player.sink();
    std::thread::Builder::new()
        .name("nexus-audio-raw-feeder".to_string())
        .spawn(move || {
            while let Some(pkt) = rx.blocking_recv() {
                if !pkt.is_config && !pkt.data.is_empty() {
                    sink.push_s16le(&pkt.data);
                }
            }
        })
        .ok();
    Some(player)
}

#[inline]
fn write_stereo_f32(frame: &mut [f32], l: f32, r: f32) {
    if frame.is_empty() {
        return;
    }
    if frame.len() == 1 {
        frame[0] = (l + r) * 0.5;
    } else {
        frame[0] = l;
        frame[1] = r;
        for ch in &mut frame[2..] {
            *ch = 0.0;
        }
    }
}

#[inline]
fn write_stereo_i16(frame: &mut [i16], l: f32, r: f32) {
    let li = (l.clamp(-1.0, 1.0) * 32767.0) as i16;
    let ri = (r.clamp(-1.0, 1.0) * 32767.0) as i16;
    if frame.is_empty() {
        return;
    }
    if frame.len() == 1 {
        frame[0] = (((l + r) * 0.5).clamp(-1.0, 1.0) * 32767.0) as i16;
    } else {
        frame[0] = li;
        frame[1] = ri;
        for ch in &mut frame[2..] {
            *ch = 0;
        }
    }
}

#[inline]
fn write_stereo_u16(frame: &mut [u16], l: f32, r: f32) {
    let lu = ((l.clamp(-1.0, 1.0) * 32767.0) as i32 + 32768) as u16;
    let ru = ((r.clamp(-1.0, 1.0) * 32767.0) as i32 + 32768) as u16;
    if frame.is_empty() {
        return;
    }
    if frame.len() == 1 {
        frame[0] = ((((l + r) * 0.5).clamp(-1.0, 1.0) * 32767.0) as i32 + 32768) as u16;
    } else {
        frame[0] = lu;
        frame[1] = ru;
        for ch in &mut frame[2..] {
            *ch = 32768;
        }
    }
}
