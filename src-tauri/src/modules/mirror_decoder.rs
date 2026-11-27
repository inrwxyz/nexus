//! Pure-Rust Windows Media Foundation decoders for Nexus Screen Mirror.
//! Decodes H.264 Annex B video streams directly into cropped NV12 frames
//! and decodes AAC / Opus audio packets directly into 48 kHz S16LE stereo PCM,
//! with zero external DLLs or executables.

use windows::core::GUID;
use windows::Win32::Media::MediaFoundation::{
    IMFActivate, IMFMediaBuffer, IMFMediaType, IMFSample, IMFTransform, MFCreateMediaType,
    MFCreateMemoryBuffer, MFCreateSample, MFMediaType_Audio, MFMediaType_Video, MFShutdown,
    MFStartup, MFTEnumEx, MFT_CATEGORY_AUDIO_DECODER, MFT_CATEGORY_VIDEO_DECODER,
    MFT_ENUM_FLAG_LOCALMFT, MFT_ENUM_FLAG_SORTANDFILTER, MFT_ENUM_FLAG_SYNCMFT,
    MFT_MESSAGE_COMMAND_FLUSH, MFT_MESSAGE_NOTIFY_BEGIN_STREAMING,
    MFT_MESSAGE_NOTIFY_START_OF_STREAM, MFT_OUTPUT_DATA_BUFFER,
    MFT_OUTPUT_STREAM_PROVIDES_SAMPLES, MFT_REGISTER_TYPE_INFO, MFAudioFormat_AAC,
    MFAudioFormat_Float, MFAudioFormat_Opus, MFAudioFormat_PCM, MFVideoArea,
    MFVideoFormat_H264, MFVideoFormat_NV12, MF_E_NOTACCEPTING, MF_E_TRANSFORM_NEED_MORE_INPUT,
    MF_E_TRANSFORM_STREAM_CHANGE, MF_LOW_LATENCY, MF_MT_AAC_PAYLOAD_TYPE,
    MF_MT_AUDIO_AVG_BYTES_PER_SECOND, MF_MT_AUDIO_BITS_PER_SAMPLE, MF_MT_AUDIO_BLOCK_ALIGNMENT,
    MF_MT_AUDIO_NUM_CHANNELS, MF_MT_AUDIO_SAMPLES_PER_SECOND, MF_MT_DEFAULT_STRIDE,
    MF_MT_FRAME_SIZE, MF_MT_INTERLACE_MODE, MF_MT_MAJOR_TYPE, MF_MT_MINIMUM_DISPLAY_APERTURE,
    MF_MT_SUBTYPE, MF_MT_USER_DATA, MF_VERSION,
};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize, CLSCTX_INPROC_SERVER,
    COINIT_MULTITHREADED,
};

const MFSTARTUP_NOSOCKET: u32 = 1;
const CLSID_CMS_H264_DECODER_MFT: GUID = GUID::from_u128(0x62ce7e72_4c71_4d20_b15d_452831a87d9d);
const CLSID_CMS_AAC_DEC_MFT: GUID = GUID::from_u128(0x32d186a7_218f_4c75_8876_dd77273a8999);

/// RAII guard for COM + Media Foundation on the owning decoder thread.
pub struct MfSession {
    com_inited: bool,
    mf_started: bool,
}

impl MfSession {
    pub fn start() -> Result<Self, String> {
        let com_hr = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        // S_OK (0) or S_FALSE (1) means COM is active and paired with CoUninitialize;
        // RPC_E_CHANGED_MODE (0x80010106) means the thread already runs an apartment.
        let com_inited = com_hr.is_ok();
        unsafe { MFStartup(MF_VERSION, MFSTARTUP_NOSOCKET) }
            .map_err(|e| format!("MFStartup failed: {e}"))?;
        Ok(Self {
            com_inited,
            mf_started: true,
        })
    }
}

impl Drop for MfSession {
    fn drop(&mut self) {
        if self.mf_started {
            let _ = unsafe { MFShutdown() };
        }
        if self.com_inited {
            unsafe { CoUninitialize() };
        }
    }
}

#[derive(Debug, Clone)]
pub struct DecodedVideoFrame {
    pub width: u32,
    pub height: u32,
    pub crop_x: u32,
    pub crop_y: u32,
    pub coded_width: u32,
    pub coded_height: u32,
    pub stride: u32,
    pub nv12_data: Vec<u8>,
    pub pts_us: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct VideoOutputLayout {
    coded_width: u32,
    coded_height: u32,
    crop_x: u32,
    crop_y: u32,
    width: u32,
    height: u32,
    stride: u32,
    provides_samples: bool,
    buffer_size: u32,
}

pub struct H264MfDecoder {
    transform: IMFTransform,
    layout: VideoOutputLayout,
    pending_config: Option<Vec<u8>>,
    last_config: Option<Vec<u8>>,
}

impl H264MfDecoder {
    pub fn new(initial_width: u32, initial_height: u32) -> Result<Self, String> {
        let transform = activate_sync_mft(
            MFT_CATEGORY_VIDEO_DECODER,
            MFMediaType_Video,
            MFVideoFormat_H264,
            MFMediaType_Video,
            MFVideoFormat_NV12,
        )
        .or_else(|_| unsafe {
            CoCreateInstance(&CLSID_CMS_H264_DECODER_MFT, None, CLSCTX_INPROC_SERVER)
                .map_err(|e| format!("CoCreateInstance H264 MFT failed: {e}"))
        })?;

        if let Ok(attrs) = unsafe { transform.GetAttributes() } {
            let _ = unsafe { attrs.SetUINT32(&MF_LOW_LATENCY, 1) };
        }

        let in_type = unsafe { MFCreateMediaType() }
            .map_err(|e| format!("MFCreateMediaType input failed: {e}"))?;
        unsafe {
            in_type
                .SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)
                .map_err(|e| format!("Set major type failed: {e}"))?;
            in_type
                .SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_H264)
                .map_err(|e| format!("Set H264 subtype failed: {e}"))?;
            // MFVideoInterlace_Progressive = 2
            let _ = in_type.SetUINT32(&MF_MT_INTERLACE_MODE, 2);
            if initial_width > 0 && initial_height > 0 {
                let packed = ((initial_width as u64) << 32) | (initial_height as u64);
                let _ = in_type.SetUINT64(&MF_MT_FRAME_SIZE, packed);
            }
            transform
                .SetInputType(0, &in_type, 0)
                .map_err(|e| format!("SetInputType H264 failed: {e}"))?;
        }

        let layout = select_nv12_output(&transform, initial_width, initial_height)?;
        unsafe {
            let _ = transform.ProcessMessage(MFT_MESSAGE_NOTIFY_BEGIN_STREAMING, 0);
            let _ = transform.ProcessMessage(MFT_MESSAGE_NOTIFY_START_OF_STREAM, 0);
        }

        Ok(Self {
            transform,
            layout,
            pending_config: None,
            last_config: None,
        })
    }

    pub fn decode_packet(
        &mut self,
        data: &[u8],
        pts_us: u64,
        is_config: bool,
        is_key_frame: bool,
    ) -> Result<Vec<DecodedVideoFrame>, String> {
        if is_config {
            self.last_config = Some(data.to_vec());
            self.pending_config = Some(data.to_vec());
            return Ok(Vec::new());
        }
        if data.is_empty() {
            return Ok(Vec::new());
        }

        let combined: Vec<u8>;
        let payload = if let Some(cfg) = self.pending_config.take() {
            let mut v = Vec::with_capacity(cfg.len() + data.len());
            v.extend_from_slice(&cfg);
            v.extend_from_slice(data);
            combined = v;
            &combined[..]
        } else if is_key_frame && !contains_sps(data) {
            if let Some(cfg) = &self.last_config {
                let mut v = Vec::with_capacity(cfg.len() + data.len());
                v.extend_from_slice(cfg);
                v.extend_from_slice(data);
                combined = v;
                &combined[..]
            } else {
                data
            }
        } else {
            data
        };

        let sample = create_sample_from_bytes(payload, (pts_us as i64).saturating_mul(10))?;
        let mut frames = Vec::new();

        if let Err(err) = unsafe { self.transform.ProcessInput(0, &sample, 0) } {
            if err.code() == MF_E_NOTACCEPTING {
                self.drain_outputs(&mut frames)?;
                unsafe { self.transform.ProcessInput(0, &sample, 0) }
                    .map_err(|e| format!("H264 ProcessInput retry failed: {e}"))?;
            } else {
                return Err(format!("H264 ProcessInput failed: {err}"));
            }
        }

        self.drain_outputs(&mut frames)?;
        Ok(frames)
    }

    pub fn flush(&mut self) -> Result<Vec<DecodedVideoFrame>, String> {
        let mut frames = Vec::new();
        let _ = unsafe { self.transform.ProcessMessage(MFT_MESSAGE_COMMAND_FLUSH, 0) };
        self.drain_outputs(&mut frames)?;
        Ok(frames)
    }

    fn drain_outputs(&mut self, out: &mut Vec<DecodedVideoFrame>) -> Result<(), String> {
        loop {
            let out_sample = if self.layout.provides_samples {
                None
            } else {
                Some(create_empty_sample(self.layout.buffer_size.max(65536))?)
            };

            let mut out_buf = [MFT_OUTPUT_DATA_BUFFER {
                dwStreamID: 0,
                pSample: std::mem::ManuallyDrop::new(out_sample),
                dwStatus: 0,
                pEvents: std::mem::ManuallyDrop::new(None),
            }];
            let mut status: u32 = 0;

            let res = unsafe { self.transform.ProcessOutput(0, &mut out_buf, &mut status) };

            let extracted_sample = unsafe { std::mem::ManuallyDrop::take(&mut out_buf[0].pSample) };
            let _events = unsafe { std::mem::ManuallyDrop::take(&mut out_buf[0].pEvents) };

            match res {
                Ok(()) => {
                    if let Some(sample) = extracted_sample {
                        if let Some(frame) = extract_nv12_frame(&sample, self.layout)? {
                            out.push(frame);
                        }
                    }
                }
                Err(e) if e.code() == MF_E_TRANSFORM_STREAM_CHANGE => {
                    self.layout = select_nv12_output(
                        &self.transform,
                        self.layout.width,
                        self.layout.height,
                    )?;
                }
                Err(e) if e.code() == MF_E_TRANSFORM_NEED_MORE_INPUT => {
                    break;
                }
                Err(e) => {
                    return Err(format!("H264 ProcessOutput failed: {e}"));
                }
            }
        }
        Ok(())
    }
}

fn contains_sps(annex_b: &[u8]) -> bool {
    let mut i = 0usize;
    while i + 4 < annex_b.len() {
        if annex_b[i] == 0 && annex_b[i + 1] == 0 {
            if annex_b[i + 2] == 1 && i + 3 < annex_b.len() {
                if (annex_b[i + 3] & 0x1f) == 7 {
                    return true;
                }
                i += 3;
                continue;
            } else if annex_b[i + 2] == 0 && annex_b[i + 3] == 1 && i + 4 < annex_b.len() {
                if (annex_b[i + 4] & 0x1f) == 7 {
                    return true;
                }
                i += 4;
                continue;
            }
        }
        i += 1;
    }
    false
}

fn select_nv12_output(
    transform: &IMFTransform,
    fallback_w: u32,
    fallback_h: u32,
) -> Result<VideoOutputLayout, String> {
    let mut chosen: Option<IMFMediaType> = None;
    for idx in 0..32u32 {
        match unsafe { transform.GetOutputAvailableType(0, idx) } {
            Ok(mt) => {
                if let Ok(sub) = unsafe { mt.GetGUID(&MF_MT_SUBTYPE) } {
                    if sub == MFVideoFormat_NV12 {
                        chosen = Some(mt);
                        break;
                    }
                }
            }
            Err(_) => break,
        }
    }

    let mt = chosen.ok_or_else(|| "MF NV12 output media type not exposed by H264 MFT".to_string())?;
    unsafe {
        transform
            .SetOutputType(0, &mt, 0)
            .map_err(|e| format!("SetOutputType NV12 failed: {e}"))?;
    }

    let cur = unsafe { transform.GetOutputCurrentType(0) }.unwrap_or(mt);
    let packed = unsafe { cur.GetUINT64(&MF_MT_FRAME_SIZE) }.unwrap_or(
        ((fallback_w.max(64) as u64) << 32) | (fallback_h.max(64) as u64),
    );
    let coded_width = ((packed >> 32) as u32).max(16);
    let coded_height = ((packed & 0xffff_ffff) as u32).max(16);

    let (crop_x, crop_y, width, height) = read_display_aperture(&cur)
        .unwrap_or((0, 0, coded_width, coded_height));

    let raw_stride = unsafe { cur.GetUINT32(&MF_MT_DEFAULT_STRIDE) }.unwrap_or(coded_width);
    let stride = (raw_stride as i32).unsigned_abs().max(coded_width);

    let stream_info = unsafe { transform.GetOutputStreamInfo(0) }
        .map_err(|e| format!("GetOutputStreamInfo failed: {e}"))?;
    let provides_samples = (stream_info.dwFlags & MFT_OUTPUT_STREAM_PROVIDES_SAMPLES.0 as u32) != 0;
    let min_nv12 = stride.saturating_mul(coded_height).saturating_mul(3) / 2;
    let buffer_size = stream_info.cbSize.max(min_nv12);

    Ok(VideoOutputLayout {
        coded_width,
        coded_height,
        crop_x,
        crop_y,
        width: width.min(coded_width.saturating_sub(crop_x)).max(1),
        height: height.min(coded_height.saturating_sub(crop_y)).max(1),
        stride,
        provides_samples,
        buffer_size,
    })
}

fn read_display_aperture(mt: &IMFMediaType) -> Option<(u32, u32, u32, u32)> {
    let mut area = MFVideoArea::default();
    let area_bytes = unsafe {
        std::slice::from_raw_parts_mut(
            (&mut area as *mut MFVideoArea) as *mut u8,
            std::mem::size_of::<MFVideoArea>(),
        )
    };
    unsafe { mt.GetBlob(&MF_MT_MINIMUM_DISPLAY_APERTURE, area_bytes, None).ok()? };
    if area.Area.cx > 0 && area.Area.cy > 0 {
        Some((
            area.OffsetX.value.max(0) as u32,
            area.OffsetY.value.max(0) as u32,
            area.Area.cx as u32,
            area.Area.cy as u32,
        ))
    } else {
        None
    }
}

fn extract_nv12_frame(
    sample: &IMFSample,
    layout: VideoOutputLayout,
) -> Result<Option<DecodedVideoFrame>, String> {
    let buf: IMFMediaBuffer = unsafe { sample.ConvertToContiguousBuffer() }
        .map_err(|e| format!("ConvertToContiguousBuffer failed: {e}"))?;

    let pts_hns = unsafe { sample.GetSampleTime() }.unwrap_or(0);
    let pts_us = (pts_hns.max(0) as u64) / 10;

    let mut ptr: *mut u8 = std::ptr::null_mut();
    let mut max_len: u32 = 0;
    let mut cur_len: u32 = 0;
    unsafe {
        buf.Lock(&mut ptr, Some(&mut max_len), Some(&mut cur_len))
            .map_err(|e| format!("NV12 buffer Lock failed: {e}"))?;
    }

    let expected = (layout.stride as usize)
        .saturating_mul(layout.coded_height as usize)
        .saturating_mul(3)
        / 2;
    let copy_len = (cur_len as usize).min(max_len as usize);
    let data = if ptr.is_null() || copy_len < expected {
        Vec::new()
    } else {
        unsafe { std::slice::from_raw_parts(ptr, expected) }.to_vec()
    };

    unsafe {
        let _ = buf.Unlock();
    }

    if data.is_empty() {
        return Ok(None);
    }

    Ok(Some(DecodedVideoFrame {
        width: layout.width,
        height: layout.height,
        crop_x: layout.crop_x,
        crop_y: layout.crop_y,
        coded_width: layout.coded_width,
        coded_height: layout.coded_height,
        stride: layout.stride,
        nv12_data: data,
        pts_us,
    }))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioCodecKind {
    Aac,
    Opus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PcmSampleKind {
    I16,
    F32,
}

pub struct AudioMfDecoder {
    kind: AudioCodecKind,
    transform: IMFTransform,
    configured: bool,
    out_kind: PcmSampleKind,
    provides_samples: bool,
    buffer_size: u32,
}

impl AudioMfDecoder {
    pub fn new(kind: AudioCodecKind) -> Result<Self, String> {
        let in_sub = match kind {
            AudioCodecKind::Aac => MFAudioFormat_AAC,
            AudioCodecKind::Opus => MFAudioFormat_Opus,
        };
        let transform = activate_sync_mft(
            MFT_CATEGORY_AUDIO_DECODER,
            MFMediaType_Audio,
            in_sub,
            MFMediaType_Audio,
            MFAudioFormat_PCM,
        )
        .or_else(|_| {
            activate_sync_mft(
                MFT_CATEGORY_AUDIO_DECODER,
                MFMediaType_Audio,
                in_sub,
                MFMediaType_Audio,
                MFAudioFormat_Float,
            )
        })
        .or_else(|e| {
            if kind == AudioCodecKind::Aac {
                unsafe { CoCreateInstance(&CLSID_CMS_AAC_DEC_MFT, None, CLSCTX_INPROC_SERVER) }
                    .map_err(|ce| format!("AAC MFT unavailable ({e}; {ce})"))
            } else {
                Err(e)
            }
        })?;

        Ok(Self {
            kind,
            transform,
            configured: false,
            out_kind: PcmSampleKind::I16,
            provides_samples: false,
            buffer_size: 32768,
        })
    }

    pub fn is_available(kind: AudioCodecKind) -> bool {
        let Ok(_mf) = MfSession::start() else {
            return false;
        };
        Self::new(kind).is_ok()
    }

    pub fn decode_packet(
        &mut self,
        data: &[u8],
        pts_us: u64,
        is_config: bool,
    ) -> Result<Vec<u8>, String> {
        if is_config {
            self.configure(data)?;
            return Ok(Vec::new());
        }
        if !self.configured {
            // Default AAC LC 48kHz stereo AudioSpecificConfig [0x11, 0x90] if config skipped.
            let default_cfg: &[u8] = match self.kind {
                AudioCodecKind::Aac => &[0x11, 0x90],
                AudioCodecKind::Opus => &[],
            };
            self.configure(default_cfg)?;
        }
        if data.is_empty() {
            return Ok(Vec::new());
        }

        let sample = create_sample_from_bytes(data, (pts_us as i64).saturating_mul(10))?;
        let mut pcm_out = Vec::new();

        if let Err(err) = unsafe { self.transform.ProcessInput(0, &sample, 0) } {
            if err.code() == MF_E_NOTACCEPTING {
                self.drain_audio(&mut pcm_out)?;
                unsafe { self.transform.ProcessInput(0, &sample, 0) }
                    .map_err(|e| format!("Audio ProcessInput retry failed: {e}"))?;
            } else {
                return Err(format!("Audio ProcessInput failed: {err}"));
            }
        }

        self.drain_audio(&mut pcm_out)?;
        Ok(pcm_out)
    }

    fn configure(&mut self, config: &[u8]) -> Result<(), String> {
        let in_type = unsafe { MFCreateMediaType() }
            .map_err(|e| format!("Audio MFCreateMediaType failed: {e}"))?;

        unsafe {
            in_type
                .SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Audio)
                .map_err(|e| format!("Audio major type failed: {e}"))?;
            match self.kind {
                AudioCodecKind::Aac => {
                    in_type
                        .SetGUID(&MF_MT_SUBTYPE, &MFAudioFormat_AAC)
                        .map_err(|e| format!("AAC subtype failed: {e}"))?;
                    let _ = in_type.SetUINT32(&MF_MT_AUDIO_SAMPLES_PER_SECOND, 48_000);
                    let _ = in_type.SetUINT32(&MF_MT_AUDIO_NUM_CHANNELS, 2);
                    let _ = in_type.SetUINT32(&MF_MT_AUDIO_BITS_PER_SAMPLE, 16);
                    // Raw AAC payload type = 0, prefixed by 12B HEAACWAVEINFO header.
                    let _ = in_type.SetUINT32(&MF_MT_AAC_PAYLOAD_TYPE, 0);
                    let mut user_data = vec![0u8; 12 + config.len()];
                    user_data[2] = 0xFE; // wAudioProfileLevelIndication unspecified
                    user_data[12..].copy_from_slice(config);
                    let _ = in_type.SetBlob(&MF_MT_USER_DATA, &user_data);
                }
                AudioCodecKind::Opus => {
                    in_type
                        .SetGUID(&MF_MT_SUBTYPE, &MFAudioFormat_Opus)
                        .map_err(|e| format!("Opus subtype failed: {e}"))?;
                    let _ = in_type.SetUINT32(&MF_MT_AUDIO_SAMPLES_PER_SECOND, 48_000);
                    let _ = in_type.SetUINT32(&MF_MT_AUDIO_NUM_CHANNELS, 2);
                    if !config.is_empty() {
                        let _ = in_type.SetBlob(&MF_MT_USER_DATA, config);
                    }
                }
            }

            self.transform
                .SetInputType(0, &in_type, 0)
                .map_err(|e| format!("Audio SetInputType failed: {e}"))?;
        }

        self.select_pcm_output()?;
        unsafe {
            let _ = self
                .transform
                .ProcessMessage(MFT_MESSAGE_NOTIFY_BEGIN_STREAMING, 0);
            let _ = self
                .transform
                .ProcessMessage(MFT_MESSAGE_NOTIFY_START_OF_STREAM, 0);
        }
        self.configured = true;
        Ok(())
    }

    fn select_pcm_output(&mut self) -> Result<(), String> {
        let mut best: Option<(IMFMediaType, PcmSampleKind)> = None;
        for idx in 0..32u32 {
            let Ok(mt) = (unsafe { self.transform.GetOutputAvailableType(0, idx) }) else {
                break;
            };
            let Ok(sub) = (unsafe { mt.GetGUID(&MF_MT_SUBTYPE) }) else {
                continue;
            };
            let sr = unsafe { mt.GetUINT32(&MF_MT_AUDIO_SAMPLES_PER_SECOND) }.unwrap_or(0);
            let ch = unsafe { mt.GetUINT32(&MF_MT_AUDIO_NUM_CHANNELS) }.unwrap_or(0);
            let bps = unsafe { mt.GetUINT32(&MF_MT_AUDIO_BITS_PER_SAMPLE) }.unwrap_or(0);
            if sub == MFAudioFormat_PCM && sr == 48_000 && ch == 2 && bps == 16 {
                best = Some((mt, PcmSampleKind::I16));
                break;
            }
            if best.is_none() && (sub == MFAudioFormat_PCM || sub == MFAudioFormat_Float) {
                let kind = if sub == MFAudioFormat_Float {
                    PcmSampleKind::F32
                } else {
                    PcmSampleKind::I16
                };
                best = Some((mt, kind));
            }
        }

        let (mt, kind) = if let Some(found) = best {
            found
        } else {
            let mt = unsafe { MFCreateMediaType() }
                .map_err(|e| format!("Audio out MFCreateMediaType failed: {e}"))?;
            unsafe {
                let _ = mt.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Audio);
                let _ = mt.SetGUID(&MF_MT_SUBTYPE, &MFAudioFormat_PCM);
                let _ = mt.SetUINT32(&MF_MT_AUDIO_SAMPLES_PER_SECOND, 48_000);
                let _ = mt.SetUINT32(&MF_MT_AUDIO_NUM_CHANNELS, 2);
                let _ = mt.SetUINT32(&MF_MT_AUDIO_BITS_PER_SAMPLE, 16);
                let _ = mt.SetUINT32(&MF_MT_AUDIO_BLOCK_ALIGNMENT, 4);
                let _ = mt.SetUINT32(&MF_MT_AUDIO_AVG_BYTES_PER_SECOND, 192_000);
            }
            (mt, PcmSampleKind::I16)
        };

        unsafe {
            self.transform
                .SetOutputType(0, &mt, 0)
                .map_err(|e| format!("Audio SetOutputType failed: {e}"))?;
        }
        self.out_kind = kind;
        let info = unsafe { self.transform.GetOutputStreamInfo(0) }.unwrap_or_default();
        self.provides_samples = (info.dwFlags & MFT_OUTPUT_STREAM_PROVIDES_SAMPLES.0 as u32) != 0;
        self.buffer_size = info.cbSize.max(32768);
        Ok(())
    }

    fn drain_audio(&mut self, out_s16le: &mut Vec<u8>) -> Result<(), String> {
        loop {
            let out_sample = if self.provides_samples {
                None
            } else {
                Some(create_empty_sample(self.buffer_size)?)
            };
            let mut out_buf = [MFT_OUTPUT_DATA_BUFFER {
                dwStreamID: 0,
                pSample: std::mem::ManuallyDrop::new(out_sample),
                dwStatus: 0,
                pEvents: std::mem::ManuallyDrop::new(None),
            }];
            let mut status: u32 = 0;

            let res = unsafe { self.transform.ProcessOutput(0, &mut out_buf, &mut status) };
            let extracted = unsafe { std::mem::ManuallyDrop::take(&mut out_buf[0].pSample) };
            let _events = unsafe { std::mem::ManuallyDrop::take(&mut out_buf[0].pEvents) };

            match res {
                Ok(()) => {
                    if let Some(sample) = extracted {
                        extract_pcm_bytes(&sample, self.out_kind, out_s16le)?;
                    }
                }
                Err(e) if e.code() == MF_E_TRANSFORM_STREAM_CHANGE => {
                    self.select_pcm_output()?;
                }
                Err(e) if e.code() == MF_E_TRANSFORM_NEED_MORE_INPUT => {
                    break;
                }
                Err(e) => {
                    return Err(format!("Audio ProcessOutput failed: {e}"));
                }
            }
        }
        Ok(())
    }
}

fn extract_pcm_bytes(
    sample: &IMFSample,
    kind: PcmSampleKind,
    out_s16le: &mut Vec<u8>,
) -> Result<(), String> {
    let buf = unsafe { sample.ConvertToContiguousBuffer() }
        .map_err(|e| format!("Audio ConvertToContiguousBuffer failed: {e}"))?;
    let mut ptr: *mut u8 = std::ptr::null_mut();
    let mut max_len: u32 = 0;
    let mut cur_len: u32 = 0;
    unsafe {
        buf.Lock(&mut ptr, Some(&mut max_len), Some(&mut cur_len))
            .map_err(|e| format!("Audio Lock failed: {e}"))?;
    }
    if !ptr.is_null() && cur_len > 0 {
        let bytes = unsafe { std::slice::from_raw_parts(ptr, cur_len as usize) };
        match kind {
            PcmSampleKind::I16 => out_s16le.extend_from_slice(bytes),
            PcmSampleKind::F32 => {
                for chunk in bytes.as_chunks::<4>().0 {
                    let f = f32::from_le_bytes(*chunk);
                    let s = (f.clamp(-1.0, 1.0) * 32767.0) as i16;
                    out_s16le.extend_from_slice(&s.to_le_bytes());
                }
            }
        }
    }
    unsafe {
        let _ = buf.Unlock();
    }
    Ok(())
}

fn activate_sync_mft(
    category: GUID,
    in_major: GUID,
    in_sub: GUID,
    out_major: GUID,
    out_sub: GUID,
) -> Result<IMFTransform, String> {
    let in_info = MFT_REGISTER_TYPE_INFO {
        guidMajorType: in_major,
        guidSubtype: in_sub,
    };
    let out_info = MFT_REGISTER_TYPE_INFO {
        guidMajorType: out_major,
        guidSubtype: out_sub,
    };
    let mut pp_activates: *mut Option<IMFActivate> = std::ptr::null_mut();
    let mut count: u32 = 0;

    unsafe {
        MFTEnumEx(
            category,
            MFT_ENUM_FLAG_SYNCMFT | MFT_ENUM_FLAG_LOCALMFT | MFT_ENUM_FLAG_SORTANDFILTER,
            Some(&in_info),
            Some(&out_info),
            &mut pp_activates,
            &mut count,
        )
    }
    .map_err(|e| format!("MFTEnumEx failed: {e}"))?;

    if pp_activates.is_null() || count == 0 {
        if !pp_activates.is_null() {
            unsafe { CoTaskMemFree(Some(pp_activates.cast())) };
        }
        return Err("No matching synchronous MFT registered on system".to_string());
    }

    let mut chosen: Option<IMFTransform> = None;
    let slice = unsafe { std::slice::from_raw_parts_mut(pp_activates, count as usize) };
    for slot in slice.iter_mut() {
        if chosen.is_none() {
            if let Some(act) = slot.as_ref() {
                if let Ok(mft) = unsafe { act.ActivateObject::<IMFTransform>() } {
                    chosen = Some(mft);
                }
            }
        }
        drop(slot.take());
    }
    unsafe { CoTaskMemFree(Some(pp_activates.cast())) };

    chosen.ok_or_else(|| "Matched MFT activation failed".to_string())
}

fn create_sample_from_bytes(data: &[u8], hns_time: i64) -> Result<IMFSample, String> {
    let sample = unsafe { MFCreateSample() }.map_err(|e| format!("MFCreateSample: {e}"))?;
    let buf = unsafe { MFCreateMemoryBuffer(data.len() as u32) }
        .map_err(|e| format!("MFCreateMemoryBuffer: {e}"))?;

    let mut ptr: *mut u8 = std::ptr::null_mut();
    unsafe {
        buf.Lock(&mut ptr, None, None)
            .map_err(|e| format!("Input buffer Lock: {e}"))?;
        std::ptr::copy_nonoverlapping(data.as_ptr(), ptr, data.len());
        let _ = buf.Unlock();
        let _ = buf.SetCurrentLength(data.len() as u32);
        sample
            .AddBuffer(&buf)
            .map_err(|e| format!("AddBuffer: {e}"))?;
        let _ = sample.SetSampleTime(hns_time);
    }
    Ok(sample)
}

fn create_empty_sample(size: u32) -> Result<IMFSample, String> {
    let sample = unsafe { MFCreateSample() }.map_err(|e| format!("MFCreateSample: {e}"))?;
    let buf =
        unsafe { MFCreateMemoryBuffer(size) }.map_err(|e| format!("MFCreateMemoryBuffer: {e}"))?;
    unsafe {
        sample
            .AddBuffer(&buf)
            .map_err(|e| format!("AddBuffer: {e}"))?;
    }
    Ok(sample)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mf_h264_and_audio_decoders_probe_cleanly() {
        let _mf = MfSession::start().expect("Media Foundation should initialize");
        let mut h264 = H264MfDecoder::new(64, 64).expect("Windows H264 MFT should initialize");

        // Minimal Baseline 64x64 SPS + PPS (4x4 macroblocks) in Annex B
        let sps_pps: [u8; 27] = [
            0x00, 0x00, 0x00, 0x01, 0x67, 0x42, 0xC0, 0x0A, 0xD9, 0x04, 0x10, 0xA0, 0x00, 0x00,
            0x03, 0x00, 0x20, 0x00, 0x00, 0x07, 0x81, 0xE2, 0x44, 0x00, 0x00, 0x00, 0x01,
        ];
        // Wait: append actual PPS NAL (68 ce 38 80) after last start code:
        let mut cfg = Vec::from(sps_pps);
        cfg.extend_from_slice(&[0x68, 0xCE, 0x38, 0x80]);

        let res = h264.decode_packet(&cfg, 0, true, false);
        assert!(res.is_ok());

        assert!(
            AudioMfDecoder::is_available(AudioCodecKind::Aac),
            "Windows AAC MFT should be available"
        );
        let mut aac = AudioMfDecoder::new(AudioCodecKind::Aac).unwrap();
        let aac_cfg = aac.decode_packet(&[0x11, 0x90], 0, true);
        assert!(aac_cfg.is_ok(), "AAC AudioSpecificConfig configure failed: {aac_cfg:?}");

        if AudioMfDecoder::is_available(AudioCodecKind::Opus) {
            let mut opus = AudioMfDecoder::new(AudioCodecKind::Opus).unwrap();
            // Standard 19-byte RFC 7845 OpusHead (48kHz, 2ch, pre-skip=312)
            let opus_head: [u8; 19] = [
                b'O', b'p', b'u', b's', b'H', b'e', b'a', b'd', 1, 2, 0x38, 0x01, 0x80, 0xBB, 0,
                0, 0, 0, 0,
            ];
            let opus_cfg = opus.decode_packet(&opus_head, 0, true);
            assert!(opus_cfg.is_ok(), "Opus OpusHead configure failed: {opus_cfg:?}");
        }
    }
}
