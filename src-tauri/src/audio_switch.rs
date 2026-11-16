//! Native playback device list + default-device switch (IPolicyConfig). No PowerShell.
#![allow(non_snake_case)]
use std::ffi::c_void;
use windows::core::{interface, IUnknown, IUnknown_Vtbl, GUID, HRESULT, PCWSTR, PWSTR};
use windows::Win32::Devices::FunctionDiscovery::PKEY_Device_FriendlyName;
use windows::Win32::Media::Audio::{
    eConsole, eRender, IMMDeviceEnumerator, MMDeviceEnumerator, DEVICE_STATE_ACTIVE,
};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoTaskMemFree, CLSCTX_ALL, COINIT_MULTITHREADED, STGM_READ,
};
use windows::Win32::System::Com::StructuredStorage::PropVariantToStringAlloc;

// Undocumented but stable since Win10 (same one used by SoundSwitch / AudioDeviceCmdlets)
#[interface("f8679f50-850a-41cf-9c72-430f290290c8")]
unsafe trait IPolicyConfig: IUnknown {
    unsafe fn GetMixFormat(&self, a: PCWSTR, b: *mut *mut c_void) -> HRESULT;
    unsafe fn GetDeviceFormat(&self, a: PCWSTR, b: i32, c: *mut *mut c_void) -> HRESULT;
    unsafe fn ResetDeviceFormat(&self, a: PCWSTR) -> HRESULT;
    unsafe fn SetDeviceFormat(&self, a: PCWSTR, b: *mut c_void, c: *mut c_void) -> HRESULT;
    unsafe fn GetProcessingPeriod(&self, a: PCWSTR, b: i32, c: *mut i64, d: *mut i64) -> HRESULT;
    unsafe fn SetProcessingPeriod(&self, a: PCWSTR, b: *mut i64) -> HRESULT;
    unsafe fn GetShareMode(&self, a: PCWSTR, b: *mut c_void) -> HRESULT;
    unsafe fn SetShareMode(&self, a: PCWSTR, b: *mut c_void) -> HRESULT;
    unsafe fn GetPropertyValue(&self, a: PCWSTR, b: i32, c: *const c_void, d: *mut c_void) -> HRESULT;
    unsafe fn SetPropertyValue(&self, a: PCWSTR, b: i32, c: *const c_void, d: *mut c_void) -> HRESULT;
    unsafe fn SetDefaultEndpoint(&self, id: PCWSTR, role: i32) -> HRESULT;
    unsafe fn SetEndpointVisibility(&self, a: PCWSTR, b: i32) -> HRESULT;
}
const CLSID_POLICY_CONFIG: GUID = GUID::from_u128(0x870af99c_171d_4f9e_af0d_e63df40c2bc9);

pub struct AudioDev {
    pub id: String,
    pub name: String,
    pub kind: &'static str, // "headphones" | "speakers"
    pub is_default: bool,
}

unsafe fn take(p: PWSTR) -> String {
    let s = p.to_string().unwrap_or_default();
    CoTaskMemFree(Some(p.0 as *const c_void));
    s
}

fn kind_of(name: &str) -> &'static str {
    let n = name.to_lowercase();
    if ["headphone", "headset", "earphone", "earbud", "buds", "airpods"].iter().any(|k| n.contains(k)) {
        "headphones"
    } else {
        "speakers"
    }
}

pub fn list() -> Vec<AudioDev> {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        let run = || -> windows::core::Result<Vec<AudioDev>> {
            let en: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
            let def = en
                .GetDefaultAudioEndpoint(eRender, eConsole)
                .ok()
                .and_then(|d| d.GetId().ok())
                .map(|p| take(p))
                .unwrap_or_default();
            let col = en.EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE)?;
            let mut out = Vec::new();
            for i in 0..col.GetCount()? {
                let d = col.Item(i)?;
                let id = take(d.GetId()?);
                let store = d.OpenPropertyStore(STGM_READ)?;
                let pv = store.GetValue(&PKEY_Device_FriendlyName)?;
                let name = take(PropVariantToStringAlloc(&pv)?);
                out.push(AudioDev { kind: kind_of(&name), is_default: id == def, id, name });
            }
            Ok(out)
        };
        run().unwrap_or_default()
    }
}

/// `target` = exact device id OR "headphones" / "speakers".
pub fn switch(target: &str) -> bool {
    let devs = list();
    let id = devs
        .iter()
        .find(|d| d.id == target)
        .or_else(|| devs.iter().find(|d| d.kind == target))
        .map(|d| d.id.clone());
    let Some(id) = id else { return false };
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        let pc: IPolicyConfig = match CoCreateInstance(&CLSID_POLICY_CONFIG, None, CLSCTX_ALL) {
            Ok(p) => p,
            Err(_) => return false,
        };
        let w: Vec<u16> = id.encode_utf16().chain(std::iter::once(0)).collect();
        // role 0 = console, 1 = multimedia, 2 = communications
        (0..3).all(|role| pc.SetDefaultEndpoint(PCWSTR(w.as_ptr()), role).is_ok())
    }
}
