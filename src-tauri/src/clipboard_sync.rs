//! Windows clipboard writer for text, links, and PNG screenshots synced from Phone Link.
//! Uses WinRT `Clipboard::SetContentWithOptions` + `Flush()` on an STA thread so items appear
//! in `Win+V` Clipboard History and work with both Ctrl+V and right-click Paste across apps.

use std::io::Cursor;

use base64::Engine as _;
use windows::core::HSTRING;
use windows::ApplicationModel::DataTransfer::{
    Clipboard, ClipboardContentOptions, DataPackage, DataPackageOperation,
};
use windows::Storage::Streams::{
    DataWriter, InMemoryRandomAccessStream, RandomAccessStreamReference,
};
use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED};
use windows_sys::Win32::Foundation::{GlobalFree, HWND};
use windows_sys::Win32::Graphics::Gdi::{BITMAPINFOHEADER, BI_RGB};
use windows_sys::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData,
};
use windows_sys::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, PeekMessageW, TranslateMessage, MSG, PM_REMOVE,
};

const CF_DIB: u32 = 8;
const CF_UNICODETEXT: u32 = 13;
const GMEM_MOVEABLE: u32 = 0x0002;
const MAX_CLIPBOARD_RETRIES: u32 = 6;

#[derive(Debug, PartialEq, Eq)]
pub enum ClipboardPayload {
    Text(String),
    PngImage(Vec<u8>),
}

/// Parses either raw `image/png` bytes or JSON (`{"kind":"text","text":...}` / `{"kind":"image","data":...}`).
pub fn parse_payload(content_type: &str, body: &[u8]) -> Result<ClipboardPayload, String> {
    if content_type.to_ascii_lowercase().starts_with("image/png") {
        if body.len() < 8 || &body[..8] != b"\x89PNG\r\n\x1a\n" {
            return Err("Invalid PNG header".into());
        }
        return Ok(ClipboardPayload::PngImage(body.to_vec()));
    }

    #[derive(serde::Deserialize)]
    struct JsonClip {
        kind: Option<String>,
        text: Option<String>,
        data: Option<String>,
    }

    let req: JsonClip = serde_json::from_slice(body)
        .map_err(|_| "Malformed clipboard JSON payload".to_string())?;
    let kind = req
        .kind
        .as_deref()
        .unwrap_or(if req.data.is_some() { "image" } else { "text" });

    match kind {
        "text" | "link" => {
            let txt = req.text.unwrap_or_default();
            if txt.is_empty() {
                return Err("Clipboard text is empty".into());
            }
            Ok(ClipboardPayload::Text(txt))
        }
        "image" => {
            let b64 = req.data.unwrap_or_default();
            let raw = base64::engine::general_purpose::STANDARD
                .decode(b64.trim())
                .map_err(|e| format!("Invalid base64 image: {e}"))?;
            if raw.len() < 8 || &raw[..8] != b"\x89PNG\r\n\x1a\n" {
                return Err("Decoded image is not valid PNG".into());
            }
            Ok(ClipboardPayload::PngImage(raw))
        }
        other => Err(format!("Unsupported clipboard kind: {other}")),
    }
}

/// Decodes PNG bytes into a Windows `CF_DIB` buffer (40-byte `BITMAPINFOHEADER` + bottom-up 32bpp BGRA pixels).
pub fn png_to_cf_dib(png_bytes: &[u8]) -> Result<(u32, u32, Vec<u8>), String> {
    let mut decoder = png::Decoder::new(Cursor::new(png_bytes));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::ALPHA);
    let mut reader = decoder
        .read_info()
        .map_err(|e| format!("PNG header decode failed: {e}"))?;

    let buf_size = reader
        .output_buffer_size()
        .ok_or_else(|| "PNG output buffer size overflow".to_string())?;
    let mut rgba = vec![0u8; buf_size];
    let info = reader
        .next_frame(&mut rgba)
        .map_err(|e| format!("PNG frame decode failed: {e}"))?;

    let width = info.width;
    let height = info.height;
    if width == 0 || height == 0 {
        return Err("Zero-dimension PNG".into());
    }

    let stride = (width as usize) * 4;
    let expected_len = stride * (height as usize);
    let rgba_slice = match info.color_type {
        png::ColorType::Rgba => &rgba[..expected_len.min(rgba.len())],
        other => return Err(format!("Unexpected decoded PNG color type: {other:?}")),
    };
    if rgba_slice.len() < expected_len {
        return Err("Truncated PNG frame buffer".into());
    }

    let hdr = BITMAPINFOHEADER {
        biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
        biWidth: width as i32,
        biHeight: height as i32,
        biPlanes: 1,
        biBitCount: 32,
        biCompression: BI_RGB,
        biSizeImage: expected_len as u32,
        biXPelsPerMeter: 0,
        biYPelsPerMeter: 0,
        biClrUsed: 0,
        biClrImportant: 0,
    };

    let hdr_bytes: &[u8] = unsafe {
        std::slice::from_raw_parts(
            (&hdr as *const BITMAPINFOHEADER) as *const u8,
            std::mem::size_of::<BITMAPINFOHEADER>(),
        )
    };

    let mut dib = Vec::with_capacity(hdr_bytes.len() + expected_len);
    dib.extend_from_slice(hdr_bytes);

    for y in (0..height as usize).rev() {
        let row = &rgba_slice[y * stride..(y + 1) * stride];
        for px in row.as_chunks::<4>().0 {
            dib.push(px[2]);
            dib.push(px[1]);
            dib.push(px[0]);
            dib.push(px[3]);
        }
    }

    Ok((width, height, dib))
}

fn pump_sta_messages() {
    unsafe {
        let mut msg: MSG = std::mem::zeroed();
        while PeekMessageW(&mut msg, std::ptr::null_mut(), 0, 0, PM_REMOVE) != 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

fn run_on_sta<F>(job: F) -> Result<(), String>
where
    F: FnOnce() -> Result<(), String> + Send + 'static,
{
    std::thread::spawn(move || {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        }
        let res = job();
        pump_sta_messages();
        unsafe {
            CoUninitialize();
        }
        res
    })
    .join()
    .map_err(|_| "Clipboard STA thread panicked".to_string())?
}

fn winrt_set_text(text: &str) -> Result<(), String> {
    let owned = text.to_string();
    run_on_sta(move || {
        let pkg = DataPackage::new().map_err(|e| format!("DataPackage::new: {e}"))?;
        pkg.SetRequestedOperation(DataPackageOperation::Copy)
            .map_err(|e| format!("SetRequestedOperation: {e}"))?;
        pkg.SetText(&HSTRING::from(owned.as_str()))
            .map_err(|e| format!("SetText: {e}"))?;

        let opts =
            ClipboardContentOptions::new().map_err(|e| format!("ClipboardContentOptions: {e}"))?;
        let _ = opts.SetIsAllowedInHistory(true);
        let _ = opts.SetIsRoamable(false);

        Clipboard::SetContentWithOptions(&pkg, &opts)
            .map_err(|e| format!("SetContentWithOptions: {e}"))?;
        Clipboard::Flush().map_err(|e| format!("Clipboard::Flush: {e}"))?;
        Ok(())
    })
}

fn winrt_set_png(png_bytes: &[u8]) -> Result<(), String> {
    let owned = png_bytes.to_vec();
    run_on_sta(move || {
        let stream = InMemoryRandomAccessStream::new()
            .map_err(|e| format!("InMemoryRandomAccessStream: {e}"))?;
        let writer =
            DataWriter::CreateDataWriter(&stream).map_err(|e| format!("CreateDataWriter: {e}"))?;
        writer
            .WriteBytes(&owned)
            .map_err(|e| format!("WriteBytes: {e}"))?;
        writer
            .StoreAsync()
            .map_err(|e| format!("StoreAsync: {e}"))?
            .get()
            .map_err(|e| format!("StoreAsync.get: {e}"))?;
        writer
            .FlushAsync()
            .map_err(|e| format!("FlushAsync: {e}"))?
            .get()
            .map_err(|e| format!("FlushAsync.get: {e}"))?;
        let _ = writer.DetachStream();
        stream.Seek(0).map_err(|e| format!("Seek(0): {e}"))?;

        let stream_ref = RandomAccessStreamReference::CreateFromStream(&stream)
            .map_err(|e| format!("CreateFromStream: {e}"))?;

        let pkg = DataPackage::new().map_err(|e| format!("DataPackage::new: {e}"))?;
        pkg.SetRequestedOperation(DataPackageOperation::Copy)
            .map_err(|e| format!("SetRequestedOperation: {e}"))?;
        pkg.SetBitmap(&stream_ref)
            .map_err(|e| format!("SetBitmap: {e}"))?;

        let opts =
            ClipboardContentOptions::new().map_err(|e| format!("ClipboardContentOptions: {e}"))?;
        let _ = opts.SetIsAllowedInHistory(true);
        let _ = opts.SetIsRoamable(false);

        Clipboard::SetContentWithOptions(&pkg, &opts)
            .map_err(|e| format!("SetContentWithOptions: {e}"))?;
        Clipboard::Flush().map_err(|e| format!("Clipboard::Flush: {e}"))?;
        Ok(())
    })
}

fn write_clipboard_bytes_win32(format: u32, bytes: &[u8]) -> Result<(), String> {
    unsafe {
        let mut opened = false;
        for _ in 0..MAX_CLIPBOARD_RETRIES {
            if OpenClipboard(std::ptr::null_mut() as HWND) != 0 {
                opened = true;
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(12));
        }
        if !opened {
            return Err("Could not open Windows clipboard".into());
        }

        EmptyClipboard();
        let h_mem = GlobalAlloc(GMEM_MOVEABLE, bytes.len());
        if h_mem.is_null() {
            CloseClipboard();
            return Err("GlobalAlloc failed".into());
        }

        let ptr = GlobalLock(h_mem) as *mut u8;
        if ptr.is_null() {
            GlobalFree(h_mem);
            CloseClipboard();
            return Err("GlobalLock failed".into());
        }

        std::ptr::copy_nonoverlapping(bytes.as_ptr(), ptr, bytes.len());
        GlobalUnlock(h_mem);

        let set_ok = !SetClipboardData(format, h_mem).is_null();
        if !set_ok {
            GlobalFree(h_mem);
        }
        CloseClipboard();

        if set_ok {
            Ok(())
        } else {
            Err("SetClipboardData failed".into())
        }
    }
}

pub fn set_clipboard_text(text: &str) -> Result<(), String> {
    if winrt_set_text(text).is_ok() {
        return Ok(());
    }
    let utf16: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
    let bytes = unsafe {
        std::slice::from_raw_parts(utf16.as_ptr() as *const u8, utf16.len() * 2)
    };
    write_clipboard_bytes_win32(CF_UNICODETEXT, bytes)
}

pub fn set_clipboard_png(png_bytes: &[u8]) -> Result<(), String> {
    if winrt_set_png(png_bytes).is_ok() {
        return Ok(());
    }
    let (_, _, dib) = png_to_cf_dib(png_bytes)?;
    write_clipboard_bytes_win32(CF_DIB, &dib)
}

pub fn apply_payload(payload: &ClipboardPayload) -> Result<&'static str, String> {
    match payload {
        ClipboardPayload::Text(txt) => {
            set_clipboard_text(txt)?;
            Ok("text")
        }
        ClipboardPayload::PngImage(png) => {
            set_clipboard_png(png)?;
            Ok("image")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_2x2_png() -> Vec<u8> {
        let mut out = Vec::new();
        {
            let mut enc = png::Encoder::new(&mut out, 2, 2);
            enc.set_color(png::ColorType::Rgba);
            enc.set_depth(png::BitDepth::Eight);
            let mut writer = enc.write_header().unwrap();
            let data = [
                255, 0, 0, 255, 0, 255, 0, 255, // row 0 (top)
                0, 0, 255, 255, 255, 255, 255, 255, // row 1 (bottom)
            ];
            writer.write_image_data(&data).unwrap();
        }
        out
    }

    #[test]
    fn parses_text_link_and_image_payloads() {
        let p_txt = parse_payload("application/json", br#"{"kind":"text","text":"hello world"}"#).unwrap();
        assert_eq!(p_txt, ClipboardPayload::Text("hello world".into()));

        let p_link = parse_payload("application/json", br#"{"kind":"link","text":"https://github.com/inrwxyz"}"#).unwrap();
        assert_eq!(p_link, ClipboardPayload::Text("https://github.com/inrwxyz".into()));

        let sample = sample_2x2_png();
        let b64 = base64::engine::general_purpose::STANDARD.encode(&sample);
        let json_img = serde_json::json!({ "kind": "image", "data": b64 }).to_string();
        assert_eq!(
            parse_payload("application/json", json_img.as_bytes()).unwrap(),
            ClipboardPayload::PngImage(sample.clone())
        );

        assert_eq!(
            parse_payload("image/png", &sample).unwrap(),
            ClipboardPayload::PngImage(sample)
        );
        assert!(parse_payload("application/json", br#"{"kind":"text","text":""}"#).is_err());
        assert!(parse_payload("image/png", b"not-a-png").is_err());
    }

    #[test]
    fn converts_png_to_bottom_up_bgra_dib() {
        let sample = sample_2x2_png();
        let (w, h, dib) = png_to_cf_dib(&sample).unwrap();
        assert_eq!((w, h), (2, 2));
        let hdr_size = std::mem::size_of::<BITMAPINFOHEADER>();
        assert_eq!(hdr_size, 40);
        assert_eq!(dib.len(), 40 + 16);
        let pixels = &dib[40..];
        assert_eq!(&pixels[0..4], &[255, 0, 0, 255]);
        assert_eq!(&pixels[4..8], &[255, 255, 255, 255]);
        assert_eq!(&pixels[8..12], &[0, 0, 255, 255]);
        assert_eq!(&pixels[12..16], &[0, 255, 0, 255]);
    }
}
