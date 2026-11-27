//! App-iconen ophalen en als PNG (data-URL) teruggeven.
//!
//! Windows kent meerdere manieren om aan een icoon te komen, en niet elke
//! manier werkt voor elke app. We proberen ze op volgorde:
//! 1. de shell (IShellItemImageFactory): beste kwaliteit, werkt ook voor Store-apps;
//! 2. de systeemafbeeldingslijst, zoals Verkenner die gebruikt (48 px);
//! 3. het eerste icoon uit het exe-bestand zelf;
//! 4. het icoon van een open venster van de app, zoals de taakbalk dat doet.
//!
//! Alles draait op één eigen thread met een STA-apartment: shell-iconen werken
//! daar betrouwbaar, op de algemene threadpool niet altijd. Lukt niets, dan
//! komt de reden in het logbestand.

use crate::util::log;
use base64::Engine;
use std::ffi::c_void;
use std::mem::size_of;
use std::sync::mpsc::{channel, Sender};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;
use windows::core::HSTRING;
use windows::Win32::Foundation::{HWND, LPARAM, SIZE, WPARAM};
use windows::Win32::Graphics::Gdi::{
    DeleteObject, GetDC, GetDIBits, GetObjectW, ReleaseDC, BITMAP, BITMAPINFO, BITMAPINFOHEADER, DIB_RGB_COLORS,
    HBITMAP,
};
use windows::Win32::Storage::FileSystem::FILE_FLAGS_AND_ATTRIBUTES;
use windows::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};
use windows::Win32::UI::Controls::{IImageList, ILD_TRANSPARENT};
use windows::Win32::UI::Shell::{
    ExtractIconExW, IShellItemImageFactory, SHCreateItemFromParsingName, SHGetFileInfoW, SHGetImageList, SHFILEINFOW,
    SHGFI_SYSICONINDEX, SHIL_EXTRALARGE, SIIGBF_BIGGERSIZEOK, SIIGBF_ICONONLY,
};
use windows::Win32::UI::WindowsAndMessaging::{
    DestroyIcon, GetClassLongPtrW, GetIconInfo, SendMessageTimeoutW, GCLP_HICON, HICON, ICONINFO, ICON_BIG,
    ICON_SMALL2, SMTO_ABORTIFHUNG, WM_GETICON,
};

struct Job {
    key: String,
    hwnd: Option<isize>,
    size: i32,
    reply: Sender<Result<String, String>>,
}

static QUEUE: OnceLock<Mutex<Sender<Job>>> = OnceLock::new();

fn queue() -> Sender<Job> {
    QUEUE
        .get_or_init(|| {
            let (tx, rx) = channel::<Job>();
            std::thread::Builder::new()
                .name("iconen".into())
                .spawn(move || {
                    unsafe {
                        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
                    }
                    for job in rx {
                        let _ = job.reply.send(load(&job.key, job.hwnd, job.size));
                    }
                })
                .expect("icoonthread starten");
            Mutex::new(tx)
        })
        .lock()
        .unwrap()
        .clone()
}

/// `key` is een groepssleutel: exe-/lnk-pad, `aumid:<id>` of `hwnd:<handle>`.
/// `hwnd` is een open venster van de app, als dat er is (laatste redmiddel).
pub fn icon_data_url(key: &str, hwnd: Option<isize>, size: i32) -> Result<String, String> {
    let (tx, rx) = channel();
    queue()
        .send(Job { key: key.to_string(), hwnd, size, reply: tx })
        .map_err(|e| e.to_string())?;
    rx.recv_timeout(Duration::from_secs(10)).map_err(|_| "time-out".to_string())?
}

struct Image {
    w: u32,
    h: u32,
    rgba: Vec<u8>,
}

fn load(key: &str, hwnd: Option<isize>, size: i32) -> Result<String, String> {
    let aumid = key.strip_prefix("aumid:");
    let path = (aumid.is_none() && !key.starts_with("hwnd:")).then_some(key);
    let mut errors: Vec<String> = Vec::new();
    let mut attempt = |name: &str, result: Result<Image, String>| match result {
        Ok(img) => Some(img),
        Err(e) => {
            errors.push(format!("{name}: {e}"));
            None
        }
    };

    let parsing = aumid.map(|a| format!("shell:AppsFolder\\{a}")).or(path.map(String::from));
    let mut found = parsing.and_then(|src| attempt("shell", unsafe { from_shell(&src, size) }));
    if let Some(p) = path {
        found = found
            .or_else(|| attempt("systeemlijst", unsafe { from_system_list(p) }))
            .or_else(|| attempt("exe", unsafe { from_exe(p) }));
    }
    if let Some(h) = hwnd {
        found = found.or_else(|| attempt("venster", unsafe { from_window(h) }));
    }

    match found {
        Some(img) => encode(img),
        None => {
            let msg = format!("geen icoon voor {key}: {}", errors.join("; "));
            log(&msg);
            Err(msg)
        }
    }
}

// ------------------------------------------------------------- bronnen --

unsafe fn from_shell(source: &str, size: i32) -> Result<Image, String> {
    let factory: IShellItemImageFactory =
        SHCreateItemFromParsingName(&HSTRING::from(source), None).map_err(|e| e.message())?;
    let hbmp = factory.GetImage(SIZE { cx: size, cy: size }, SIIGBF_ICONONLY | SIIGBF_BIGGERSIZEOK).map_err(|e| e.message())?;
    let pixels = bitmap_pixels(hbmp);
    let _ = DeleteObject(hbmp.into());
    let (w, h, px) = pixels.ok_or("bitmap onleesbaar")?;
    if px.iter().all(|&b| b == 0) {
        return Err("lege afbeelding".into());
    }
    // De shell levert voorvermenigvuldigde alpha.
    Ok(to_image(w, h, px, true))
}

unsafe fn from_system_list(path: &str) -> Result<Image, String> {
    let mut info = SHFILEINFOW::default();
    let ok = SHGetFileInfoW(
        &HSTRING::from(path),
        FILE_FLAGS_AND_ATTRIBUTES(0),
        Some(&mut info),
        size_of::<SHFILEINFOW>() as u32,
        SHGFI_SYSICONINDEX,
    );
    if ok == 0 {
        return Err("geen index".into());
    }
    let list: IImageList = SHGetImageList(SHIL_EXTRALARGE as i32).map_err(|e| e.message())?;
    let icon = list.GetIcon(info.iIcon, ILD_TRANSPARENT.0).map_err(|e| e.message())?;
    let img = icon_image(icon);
    let _ = DestroyIcon(icon);
    img
}

unsafe fn from_exe(path: &str) -> Result<Image, String> {
    let mut large = HICON::default();
    let n = ExtractIconExW(&HSTRING::from(path), 0, Some(&mut large), None, 1);
    if n == 0 || large.is_invalid() {
        return Err("geen icoon in bestand".into());
    }
    let img = icon_image(large);
    let _ = DestroyIcon(large);
    img
}

/// Het icoon dat een open venster zelf opgeeft. Dat icoon is van het venster:
/// niet vrijgeven.
unsafe fn from_window(raw: isize) -> Result<Image, String> {
    let hwnd = HWND(raw as *mut c_void);
    let ask = |kind: u32| {
        let mut result = 0usize;
        SendMessageTimeoutW(hwnd, WM_GETICON, WPARAM(kind as usize), LPARAM(0), SMTO_ABORTIFHUNG, 200, Some(&mut result));
        result
    };
    let mut handle = ask(ICON_BIG);
    if handle == 0 {
        handle = ask(ICON_SMALL2);
    }
    if handle == 0 {
        handle = GetClassLongPtrW(hwnd, GCLP_HICON);
    }
    if handle == 0 {
        return Err("venster heeft geen icoon".into());
    }
    icon_image(HICON(handle as *mut c_void))
}

// ----------------------------------------------------------- omzetten --

/// Pixels van een bitmap als BGRA, bovenste rij eerst.
unsafe fn bitmap_pixels(hbmp: HBITMAP) -> Option<(i32, i32, Vec<u8>)> {
    let mut bm = BITMAP::default();
    if GetObjectW(hbmp.into(), size_of::<BITMAP>() as i32, Some(&mut bm as *mut BITMAP as *mut c_void)) == 0 {
        return None;
    }
    // Iconen komen ook van andere processen (systeemvak, badges). Een absurd
    // groot exemplaar weigeren: dan kan `w * h * 4` niet overlopen en schrijft
    // GetDIBits nooit buiten de buffer.
    const MAX_SIDE: i32 = 2048;
    let (w, h) = (bm.bmWidth, bm.bmHeight.checked_abs()?);
    if !(1..=MAX_SIDE).contains(&w) || !(1..=MAX_SIDE).contains(&h) {
        return None;
    }
    let mut info = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: w,
            biHeight: -h, // negatief = bovenste rij eerst
            biPlanes: 1,
            biBitCount: 32,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut px = vec![0u8; w as usize * h as usize * 4];
    let dc = GetDC(None);
    let rows = GetDIBits(dc, hbmp, 0, h as u32, Some(px.as_mut_ptr() as *mut c_void), &mut info, DIB_RGB_COLORS);
    ReleaseDC(None, dc);
    (rows != 0).then_some((w, h, px))
}

unsafe fn icon_image(icon: HICON) -> Result<Image, String> {
    let mut info = ICONINFO::default();
    GetIconInfo(icon, &mut info).map_err(|e| e.message())?;
    let (color, mask) = (info.hbmColor, info.hbmMask);
    let result = if color.is_invalid() {
        Err("monochroom icoon".to_string())
    } else {
        match bitmap_pixels(color) {
            None => Err("bitmap onleesbaar".to_string()),
            Some((w, h, mut px)) => {
                // Oude iconen hebben geen alpha maar een los masker (wit = doorzichtig).
                if px.chunks_exact(4).all(|p| p[3] == 0) {
                    if let Some((mw, mh, mpx)) = bitmap_pixels(mask) {
                        if mw == w && mh >= h {
                            for (p, m) in px.chunks_exact_mut(4).zip(mpx.chunks_exact(4)) {
                                p[3] = if m[0] | m[1] | m[2] != 0 { 0 } else { 255 };
                            }
                        }
                    }
                }
                // Iconen hebben gewone (niet voorvermenigvuldigde) alpha.
                Ok(to_image(w, h, px, false))
            }
        }
    };
    if !color.is_invalid() {
        let _ = DeleteObject(color.into());
    }
    if !mask.is_invalid() {
        let _ = DeleteObject(mask.into());
    }
    result
}

/// BGRA → RGBA; haalt eventueel de voorvermenigvuldiging eraf.
fn to_image(w: i32, h: i32, mut px: Vec<u8>, premultiplied: bool) -> Image {
    let has_alpha = px.chunks_exact(4).any(|p| p[3] != 0);
    for p in px.chunks_exact_mut(4) {
        p.swap(0, 2);
        if !has_alpha {
            p[3] = 255;
        } else if premultiplied && p[3] != 0 && p[3] != 255 {
            let a = p[3] as u32;
            for c in &mut p[..3] {
                *c = ((*c as u32 * 255 + a / 2) / a).min(255) as u8;
            }
        }
    }
    Image { w: w as u32, h: h as u32, rgba: px }
}

/// Een icoon-handle (bijv. van een systeemvakpictogram) als PNG-data-URL.
pub fn hicon_data_url(icon: HICON) -> Result<String, String> {
    encode(unsafe { icon_image(icon) }?)
}

fn encode(img: Image) -> Result<String, String> {
    let mut out = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut out, img.w, img.h);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        let mut writer = enc.write_header().map_err(|e| e.to_string())?;
        writer.write_image_data(&img.rgba).map_err(|e| e.to_string())?;
    }
    Ok(format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(out)))
}
