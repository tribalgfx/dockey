//! Echte vervaging (acrylic) achter het glasthema.
//!
//! Windows vervaagt altijd een heel venster, terwijl het venster van de balk de
//! hele strook over de schermbreedte is (met een doorzichtige marge rond de
//! zwevende dock) en het paneelvenster een marge voor de schaduw heeft. Daarom
//! krijgt elk glasvlak een eigen, onzichtbaar achtergrondvenster precies op de
//! plek van dat vlak, direct eronder. Windows 11 rondt de hoeken af (8 px);
//! het glasthema gebruikt in de interface dezelfde straal.
//!
//! De vervaging zelf komt van SetWindowCompositionAttribute met
//! ACCENT_ENABLE_ACRYLICBLURBEHIND (wat ook taakbalk-tools gebruiken). De
//! officiële systeemachtergrond (DWMSBT_TRANSIENTWINDOW) valt terug op een vlakke
//! kleur zodra het venster niet actief is, en de balk is nooit actief.
//!
//! De interface meldt per venster waar het glasvlak staat (set_backdrop); deze
//! module zet het achtergrondvenster daar neer zolang het venster zichtbaar is.
//! Alles wat vensters maakt of verplaatst, draait op de hoofdthread.

use crate::util::log;
use serde::Deserialize;
use std::collections::HashMap;
use std::ffi::c_void;
use std::mem::size_of;
use std::sync::{Mutex, OnceLock};
use tauri::{AppHandle, Manager};
use windows::core::{s, w, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Dwm::{DwmSetWindowAttribute, DWMWINDOWATTRIBUTE};
use windows::Win32::Graphics::Gdi::{GetStockObject, BLACK_BRUSH, HBRUSH};
use windows::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress, LoadLibraryW};
use windows::Win32::UI::WindowsAndMessaging::*;

/// Plek van het glasvlak binnen zijn venster, in CSS-pixels.
#[derive(Deserialize, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

#[derive(Default)]
struct Surface {
    /// Het achtergrondvenster (0 = nog niet gemaakt).
    hwnd: isize,
    rect: Option<Rect>,
    round: bool,
    /// Laatst ingestelde hoekvoorkeur, om DWM niet elke keer aan te roepen.
    rounded: Option<bool>,
}

fn surfaces() -> &'static Mutex<HashMap<String, Surface>> {
    static S: OnceLock<Mutex<HashMap<String, Surface>>> = OnceLock::new();
    S.get_or_init(|| Mutex::new(HashMap::new()))
}

// DWM-attributen en -waarden (dwmapi.h).
const DWMWA_WINDOW_CORNER_PREFERENCE: i32 = 33;
const DWMWA_BORDER_COLOR: i32 = 34;
const DWMWCP_DONOTROUND: u32 = 1;
const DWMWCP_ROUND: u32 = 2;
const DWMWA_COLOR_NONE: u32 = 0xFFFF_FFFE;

// SetWindowCompositionAttribute (niet gedocumenteerd, user32).
const WCA_ACCENT_POLICY: u32 = 19;
const ACCENT_ENABLE_ACRYLICBLURBEHIND: u32 = 4;
/// Lichte tint onder de vervaging, als ABGR. De eigenlijke kleur van het glas
/// tekent de interface erbovenop; helemaal geen tint (alfa 0) geeft op sommige
/// Windows-versies een ondoorzichtig vlak.
const TINT: u32 = 0x10_62_48_3c;

#[repr(C)]
struct AccentPolicy {
    state: u32,
    flags: u32,
    color: u32,
    animation: u32,
}

#[repr(C)]
struct CompositionData {
    attrib: u32,
    data: *mut c_void,
    size: usize,
}

type SetWca = unsafe extern "system" fn(HWND, *mut CompositionData) -> i32;

fn set_wca() -> Option<SetWca> {
    static F: OnceLock<Option<usize>> = OnceLock::new();
    let f = *F.get_or_init(|| unsafe {
        let user32 = LoadLibraryW(w!("user32.dll")).ok()?;
        GetProcAddress(user32, s!("SetWindowCompositionAttribute")).map(|f| f as usize)
    });
    f.map(|f| unsafe { std::mem::transmute::<usize, SetWca>(f) })
}

/// Kan Windows dit? Vraagt Windows 11 (voor de ronde hoeken) en de
/// accent-functie van user32.
pub fn supported() -> bool {
    static OK: OnceLock<bool> = OnceLock::new();
    *OK.get_or_init(|| {
        let build = windows_registry::LOCAL_MACHINE
            .open(r"SOFTWARE\Microsoft\Windows NT\CurrentVersion")
            .and_then(|k| k.get_string("CurrentBuildNumber"))
            .ok()
            .and_then(|b| b.parse::<u32>().ok())
            .unwrap_or(0);
        build >= 22000 && set_wca().is_some()
    })
}

/// Onthoudt waar het glasvlak van een venster staat (None = geen vervaging).
/// Daarna `apply` aanroepen, op de hoofdthread.
pub fn store(label: &str, rect: Option<Rect>, round: bool) {
    let mut all = surfaces().lock().unwrap();
    let s = all.entry(label.to_string()).or_default();
    s.rect = rect;
    s.round = round;
}

/// Verbergt het achtergrondvenster van een venster (kan vanaf elke thread).
pub fn hide(label: &str) {
    // Eerst de vergrendeling loslaten: vanaf een andere thread wacht ShowWindow
    // op de hoofdthread, en die kan net in `apply` op deze vergrendeling wachten.
    let hwnd = surfaces().lock().unwrap().get(label).map(|s| s.hwnd).unwrap_or(0);
    if hwnd != 0 {
        unsafe {
            let _ = ShowWindow(HWND(hwnd as *mut c_void), SW_HIDE);
        }
    }
}

/// Zet het achtergrondvenster op de plek van het glasvlak, direct onder het
/// venster zelf; of verbergt het als het venster verborgen is of geen
/// vervaging wil. Moet op de hoofdthread draaien.
pub fn apply(app: &AppHandle, label: &str) {
    let Some(win) = app.get_webview_window(label) else { return };
    let Ok(owner) = win.hwnd() else { return };
    let visible = unsafe { IsWindowVisible(owner).as_bool() };
    let (Ok(origin), Ok(scale)) = (win.inner_position(), win.scale_factor()) else { return };

    let mut all = surfaces().lock().unwrap();
    let s = all.entry(label.to_string()).or_default();
    let Some(rect) = s.rect.filter(|_| visible && supported()) else {
        if s.hwnd != 0 {
            unsafe {
                let _ = ShowWindow(HWND(s.hwnd as *mut c_void), SW_HIDE);
            }
        }
        return;
    };
    if s.hwnd == 0 {
        match create() {
            Some(h) => s.hwnd = h.0 as isize,
            None => return,
        }
    }
    let h = HWND(s.hwnd as *mut c_void);
    if s.rounded != Some(s.round) {
        s.rounded = Some(s.round);
        set_attr(h, DWMWA_WINDOW_CORNER_PREFERENCE, if s.round { DWMWCP_ROUND } else { DWMWCP_DONOTROUND });
    }
    let x = origin.x + (rect.x * scale).round() as i32;
    let y = origin.y + (rect.y * scale).round() as i32;
    let w = (rect.width * scale).round() as i32;
    let hgt = (rect.height * scale).round() as i32;
    unsafe {
        // Direct onder het venster zelf in de stapel; beide staan "altijd bovenop".
        let _ = SetWindowPos(h, Some(owner), x, y, w, hgt, SWP_NOACTIVATE | SWP_SHOWWINDOW);
    }
}

fn set_attr(h: HWND, attr: i32, value: u32) {
    unsafe {
        let _ = DwmSetWindowAttribute(h, DWMWINDOWATTRIBUTE(attr), &value as *const u32 as *const c_void, 4);
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_NCHITTEST => LRESULT(-1), // HTTRANSPARENT: klikken gaan naar het venster erboven
        WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

fn create() -> Option<HWND> {
    unsafe {
        let instance = GetModuleHandleW(PCWSTR::null()).map(|m| m.into()).ok();
        static CLASS: OnceLock<()> = OnceLock::new();
        CLASS.get_or_init(|| {
            let class = WNDCLASSEXW {
                cbSize: size_of::<WNDCLASSEXW>() as u32,
                lpfnWndProc: Some(wndproc),
                hInstance: instance.unwrap_or_default(),
                // Zwart = niets tekenen; de vervaging komt van Windows.
                hbrBackground: HBRUSH(GetStockObject(BLACK_BRUSH).0),
                lpszClassName: w!("DockeyAcrylic"),
                ..Default::default()
            };
            RegisterClassExW(&class);
        });
        let h = match CreateWindowExW(
            WS_EX_TOOLWINDOW | WS_EX_TOPMOST | WS_EX_NOACTIVATE | WS_EX_TRANSPARENT,
            w!("DockeyAcrylic"),
            w!("Dockey vervaging"),
            WS_POPUP,
            0,
            0,
            0,
            0,
            None,
            None,
            instance,
            None,
        ) {
            Ok(h) => h,
            Err(e) => {
                log(&format!("vervaging: venster maken mislukt: {e}"));
                return None;
            }
        };
        set_attr(h, DWMWA_BORDER_COLOR, DWMWA_COLOR_NONE); // de rand tekent de interface zelf
        let Some(set) = set_wca() else { return Some(h) };
        let mut policy = AccentPolicy { state: ACCENT_ENABLE_ACRYLICBLURBEHIND, flags: 0, color: TINT, animation: 0 };
        let mut data = CompositionData {
            attrib: WCA_ACCENT_POLICY,
            data: &mut policy as *mut AccentPolicy as *mut c_void,
            size: size_of::<AccentPolicy>(),
        };
        if set(h, &mut data) == 0 {
            log("vervaging: SetWindowCompositionAttribute mislukt");
        }
        Some(h)
    }
}
