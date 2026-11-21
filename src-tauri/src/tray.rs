//! Systeemvak: de pictogrammen van apps (OneDrive, Steam, Discord, …) in Dockey.
//!
//! Apps geven hun pictogram aan "het systeemvak" met Shell_NotifyIcon. Windows
//! stuurt dat naar het eerste venster met de klasse `Shell_TrayWnd` dat het
//! vindt. Dockey maakt zo'n venster aan en houdt het bovenaan, zodat de
//! berichten bij Dockey aankomen. Alles wat binnenkomt, gaat ook door naar de
//! taakbalk van Windows: die blijft compleet, ook als Dockey stopt. Berichten
//! die niet over pictogrammen gaan (zoals AppBar-berichten, ook die van Dockey
//! zelf), geven we ongewijzigd door.
//!
//! Na het aanmelden vragen we alle apps met "TaskbarCreated" hun pictogram
//! opnieuw door te geven; bij het stoppen nogmaals, voor de zekerheid.

use crate::util::{from_wide, log};
use serde::Serialize;
use std::ffi::c_void;
use std::mem::size_of;
use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter};
use windows::core::{w, PCWSTR, PWSTR};
use windows::Win32::Foundation::{CloseHandle, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::DataExchange::COPYDATASTRUCT;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::Input::KeyboardAndMouse::GetDoubleClickTime;
use windows::Win32::UI::WindowsAndMessaging::*;

// Shell_NotifyIcon-berichten en -vlaggen (shellapi.h).
const NIM_ADD: u32 = 0;
const NIM_MODIFY: u32 = 1;
const NIM_DELETE: u32 = 2;
const NIM_SETVERSION: u32 = 4;
const NIF_MESSAGE: u32 = 0x01;
const NIF_ICON: u32 = 0x02;
const NIF_TIP: u32 = 0x04;
const NIF_STATE: u32 = 0x08;
const NIF_GUID: u32 = 0x20;
const NIS_HIDDEN: u32 = 0x01;
const NIN_SELECT: u32 = WM_USER;

/// Kenmerk waarmee shell32 een systeemvakbericht ondertekent.
const TRAY_SIGNATURE: u32 = 0x3475_3423;

#[derive(Clone)]
struct Icon {
    key: String,
    hwnd: u32,
    uid: u32,
    guid: Option<[u8; 16]>,
    callback: u32,
    version: u32,
    tip: String,
    png: String,
    hidden: bool,
    last_left: Option<Instant>,
}

#[derive(Serialize, Clone)]
pub struct TrayItem {
    pub key: String,
    pub tip: String,
    pub icon: String,
}

static APP: OnceLock<AppHandle> = OnceLock::new();
static ICONS: Mutex<Vec<Icon>> = Mutex::new(Vec::new());
static HWND_SELF: AtomicIsize = AtomicIsize::new(0);
static RUNNING: AtomicBool = AtomicBool::new(false);
static DIRTY: AtomicBool = AtomicBool::new(false);
/// Schermrechthoek van het systeemvak in de balk (voor apps die vragen waar hun pictogram staat).
static RECT: Mutex<Option<(i32, i32, i32, i32)>> = Mutex::new(None);
static TIMERS: Mutex<Timers> = Mutex::new(Timers { broadcast_at: None, last_own_broadcast: None, last_raise: None, ticks: 0 });

struct Timers {
    broadcast_at: Option<Instant>,
    last_own_broadcast: Option<Instant>,
    last_raise: Option<Instant>,
    ticks: u32,
}

fn own_hwnd() -> Option<HWND> {
    let raw = HWND_SELF.load(Ordering::SeqCst);
    (raw != 0).then_some(HWND(raw as *mut c_void))
}

fn pid_of(h: HWND) -> u32 {
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(h, Some(&mut pid)) };
    pid
}

/// De echte taakbalk van Windows (het `Shell_TrayWnd` dat niet van Dockey is).
pub fn explorer_tray() -> Option<HWND> {
    let own = std::process::id();
    let mut prev: Option<HWND> = None;
    unsafe {
        while let Ok(h) = FindWindowExW(None, prev, w!("Shell_TrayWnd"), PCWSTR::null()) {
            if h.0.is_null() {
                return None;
            }
            if pid_of(h) != own {
                return Some(h);
            }
            prev = Some(h);
        }
    }
    None
}

fn taskbar_created() -> u32 {
    static MSG: OnceLock<u32> = OnceLock::new();
    *MSG.get_or_init(|| unsafe { RegisterWindowMessageW(w!("TaskbarCreated")) })
}

fn exe_of(hwnd: u32) -> String {
    unsafe {
        let pid = pid_of(HWND(hwnd as usize as *mut c_void));
        let Ok(process) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else { return String::new() };
        let mut buf = [0u16; 1024];
        let mut len = buf.len() as u32;
        let path = QueryFullProcessImageNameW(process, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len)
            .ok()
            .map(|_| String::from_utf16_lossy(&buf[..len as usize]).to_lowercase())
            .unwrap_or_default();
        let _ = CloseHandle(process);
        path
    }
}

// ------------------------------------------------------------- berichten --

fn u32_at(b: &[u8], off: usize) -> Option<u32> {
    b.get(off..off + 4).map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
}

fn text_at(b: &[u8], off: usize, chars: usize) -> String {
    let end = (off + chars * 2).min(b.len());
    if off >= end {
        return String::new();
    }
    let wide: Vec<u16> = b[off..end].chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
    from_wide(&wide)
}

/// Verwerkt een Shell_NotifyIcon-bericht. Indeling van de gegevens: kenmerk,
/// bericht en dan NOTIFYICONDATAW in de 32-bitsvorm (handles als 32 bit).
fn handle_notify(data: &[u8]) {
    let (Some(sig), Some(message)) = (u32_at(data, 0), u32_at(data, 4)) else { return };
    if sig != TRAY_SIGNATURE {
        return;
    }
    let n = &data[8..];
    let (Some(hwnd), Some(uid), Some(flags)) = (u32_at(n, 4), u32_at(n, 8), u32_at(n, 12)) else { return };
    let guid = if flags & NIF_GUID != 0 { n.get(936..952).map(|g| <[u8; 16]>::try_from(g).unwrap()) } else { None };

    let mut icons = ICONS.lock().unwrap();
    let pos = icons.iter().position(|i| match (guid, i.guid) {
        (Some(a), Some(b)) => a == b,
        _ => i.hwnd == hwnd && i.uid == uid,
    });

    match message {
        NIM_ADD | NIM_MODIFY => {
            let index = match pos {
                Some(i) => i,
                None if message == NIM_ADD => {
                    let exe = exe_of(hwnd);
                    let mut key = match guid {
                        Some(g) => format!("guid:{}", g.iter().map(|b| format!("{b:02x}")).collect::<String>()),
                        None => format!("{exe}#{uid}"),
                    };
                    if icons.iter().any(|i| i.key == key) {
                        key = format!("{key}@{hwnd}");
                    }
                    icons.push(Icon { key, hwnd, uid, guid, callback: 0, version: 0, tip: String::new(), png: String::new(), hidden: false, last_left: None });
                    icons.len() - 1
                }
                None => return, // wijziging van een pictogram dat we niet kennen: negeren, net als Windows
            };
            let icon = &mut icons[index];
            if flags & NIF_MESSAGE != 0 {
                icon.callback = u32_at(n, 16).unwrap_or(0);
            }
            if flags & NIF_ICON != 0 {
                let handle = u32_at(n, 20).unwrap_or(0);
                icon.png = if handle == 0 {
                    String::new()
                } else {
                    crate::icons::hicon_data_url(HICON(handle as usize as *mut c_void)).unwrap_or_default()
                };
            }
            if flags & NIF_TIP != 0 {
                icon.tip = text_at(n, 24, 128);
            }
            if flags & NIF_STATE != 0 {
                let (state, mask) = (u32_at(n, 280).unwrap_or(0), u32_at(n, 284).unwrap_or(0));
                if mask & NIS_HIDDEN != 0 {
                    icon.hidden = state & NIS_HIDDEN != 0;
                }
            }
        }
        NIM_DELETE => {
            if let Some(i) = pos {
                icons.remove(i);
            }
        }
        NIM_SETVERSION => {
            if let Some(i) = pos {
                icons[i].version = u32_at(n, 800).unwrap_or(0);
            }
        }
        _ => {}
    }
    DIRTY.store(true, Ordering::SeqCst);
}

/// Antwoord op "waar staat mijn pictogram?" (Shell_NotifyIconGetRect): het
/// systeemvak in de balk. Bericht 1 vraagt linksboven, 2 rechtsonder.
fn icon_rect(data: &[u8]) -> Option<LRESULT> {
    let which = u32_at(data, 4)?;
    let (l, t, r, b) = (*RECT.lock().unwrap())?;
    let (x, y) = match which {
        1 => (l, t),
        2 => (r, b),
        _ => return None,
    };
    Some(LRESULT(((y as i16 as u16 as u32) << 16 | (x as i16 as u16 as u32)) as isize))
}

/// Geeft een WM_COPYDATA ongewijzigd door aan de taakbalk van Windows.
fn forward(wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    let Some(explorer) = explorer_tray() else { return LRESULT(0) };
    let mut result = 0usize;
    unsafe {
        SendMessageTimeoutW(explorer, WM_COPYDATA, wparam, lparam, SMTO_ABORTIFHUNG, 1000, Some(&mut result));
    }
    LRESULT(result as isize)
}

fn broadcast_taskbar_created() {
    TIMERS.lock().unwrap().last_own_broadcast = Some(Instant::now());
    unsafe {
        let _ = SendNotifyMessageW(HWND_BROADCAST, taskbar_created(), WPARAM(0), LPARAM(0));
    }
}

/// Elke 250 ms: wijzigingen doorgeven; elke 2 s opruimen en controleren of
/// berichten nog bij Dockey aankomen.
fn tick(hwnd: HWND) {
    let mut timers = TIMERS.lock().unwrap();
    timers.ticks = timers.ticks.wrapping_add(1);
    let check = timers.ticks % 8 == 0;
    let due = timers.broadcast_at.is_some_and(|at| Instant::now() >= at);
    if due {
        timers.broadcast_at = None;
    }
    drop(timers);
    if due {
        broadcast_taskbar_created();
    }

    if check {
        // Pictogrammen van apps die gestopt zijn zonder ze weg te halen.
        let mut icons = ICONS.lock().unwrap();
        let before = icons.len();
        icons.retain(|i| unsafe { IsWindow(Some(HWND(i.hwnd as usize as *mut c_void))).as_bool() });
        if icons.len() != before {
            DIRTY.store(true, Ordering::SeqCst);
        }
        drop(icons);

        // Vindt Windows nog óns venster als eerste? Anders weer bovenaan zetten
        // en apps vragen hun pictogram opnieuw te sturen (hooguit eens per 30 s).
        let first = unsafe { FindWindowW(w!("Shell_TrayWnd"), PCWSTR::null()) }.ok();
        if first != Some(hwnd) {
            unsafe {
                let _ = SetWindowPos(hwnd, Some(HWND_TOPMOST), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
            }
            let mut timers = TIMERS.lock().unwrap();
            let recent = timers.last_raise.is_some_and(|t| t.elapsed() < Duration::from_secs(30));
            if !recent {
                timers.last_raise = Some(Instant::now());
                timers.broadcast_at = Some(Instant::now() + Duration::from_millis(500));
            }
        }
    }

    if DIRTY.swap(false, Ordering::SeqCst) {
        if let Some(app) = APP.get() {
            let _ = app.emit("tray", items());
        }
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_COPYDATA => {
            let cds = &*(lparam.0 as *const COPYDATASTRUCT);
            let data: &[u8] = if cds.lpData.is_null() || cds.cbData == 0 {
                &[]
            } else {
                std::slice::from_raw_parts(cds.lpData as *const u8, cds.cbData as usize)
            };
            match cds.dwData {
                1 => {
                    handle_notify(data);
                    forward(wparam, lparam); // de taakbalk van Windows blijft compleet
                    LRESULT(1)
                }
                3 => icon_rect(data).unwrap_or_else(|| forward(wparam, lparam)),
                _ => forward(wparam, lparam), // o.a. AppBar-berichten
            }
        }
        WM_TIMER => {
            tick(hwnd);
            LRESULT(0)
        }
        WM_CLOSE => {
            let _ = DestroyWindow(hwnd);
            LRESULT(0)
        }
        WM_DESTROY => {
            PostQuitMessage(0);
            LRESULT(0)
        }
        m if m == taskbar_created() => {
            // Explorer is (her)gestart en heeft een nieuwe taakbalk: weer
            // bovenaan gaan staan en de apps opnieuw laten aanmelden. Onze eigen
            // oproep komt hier ook langs; die negeren we.
            let mut timers = TIMERS.lock().unwrap();
            let own = timers.last_own_broadcast.is_some_and(|t| t.elapsed() < Duration::from_secs(3));
            if !own {
                let _ = SetWindowPos(hwnd, Some(HWND_TOPMOST), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
                timers.broadcast_at = Some(Instant::now() + Duration::from_millis(1500));
            }
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

fn run() {
    unsafe {
        let instance = GetModuleHandleW(PCWSTR::null()).map(|m| m.into()).ok();
        let class = WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(wndproc),
            hInstance: instance.unwrap_or_default(),
            lpszClassName: w!("Shell_TrayWnd"),
            ..Default::default()
        };
        RegisterClassExW(&class);
        let hwnd = match CreateWindowExW(
            WS_EX_TOOLWINDOW | WS_EX_TOPMOST,
            w!("Shell_TrayWnd"),
            w!("Dockey systeemvak"),
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
                log(&format!("systeemvak: venster maken mislukt: {e}"));
                RUNNING.store(false, Ordering::SeqCst);
                return;
            }
        };
        HWND_SELF.store(hwnd.0 as isize, Ordering::SeqCst);
        let _ = SetWindowPos(hwnd, Some(HWND_TOPMOST), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
        SetTimer(Some(hwnd), 1, 250, None);
        broadcast_taskbar_created();

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }

        HWND_SELF.store(0, Ordering::SeqCst);
        ICONS.lock().unwrap().clear();
        RUNNING.store(false, Ordering::SeqCst);
        // Alles staat ook al op de taakbalk van Windows; dit is voor de zekerheid.
        broadcast_taskbar_created();
        if let Some(app) = APP.get() {
            let _ = app.emit("tray", Vec::<TrayItem>::new());
        }
    }
}

// ------------------------------------------------------------- openbaar --

pub fn init(app: AppHandle) {
    let _ = APP.set(app);
}

/// Systeemvak aan- of uitzetten (de instelling "Systeemvak" bij de widgets).
pub fn set_enabled(on: bool) {
    if on {
        if !RUNNING.swap(true, Ordering::SeqCst) {
            std::thread::Builder::new().name("systeemvak".into()).spawn(run).ok();
        }
    } else {
        stop();
    }
}

/// Stopt het systeemvak en wacht tot het venster weg is (bij afsluiten).
pub fn stop() {
    if let Some(h) = own_hwnd() {
        let mut result = 0usize;
        unsafe {
            SendMessageTimeoutW(h, WM_CLOSE, WPARAM(0), LPARAM(0), SMTO_ABORTIFHUNG, 2000, Some(&mut result));
        }
        for _ in 0..20 {
            if !RUNNING.load(Ordering::SeqCst) {
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}

pub fn items() -> Vec<TrayItem> {
    ICONS
        .lock()
        .unwrap()
        .iter()
        .filter(|i| !i.hidden && !i.png.is_empty())
        .map(|i| TrayItem { key: i.key.clone(), tip: i.tip.clone(), icon: i.png.clone() })
        .collect()
}

pub fn set_rect(rect: Option<(i32, i32, i32, i32)>) {
    *RECT.lock().unwrap() = rect;
}

/// Een klik op een pictogram doorgeven aan de app, zoals de taakbalk dat doet.
/// `x`, `y` = waar geklikt is, in schermpixels (voor menu's van de app).
pub fn click(key: &str, right: bool, x: i32, y: i32) {
    let icon = {
        let mut icons = ICONS.lock().unwrap();
        let Some(icon) = icons.iter_mut().find(|i| i.key == key) else { return };
        let double = !right
            && icon.last_left.is_some_and(|t| t.elapsed() < Duration::from_millis(unsafe { GetDoubleClickTime() } as u64));
        icon.last_left = if right || double { None } else { Some(Instant::now()) };
        let mut copy = icon.clone();
        copy.last_left = double.then(Instant::now); // "dubbel" doorgeven via de kopie
        copy
    };
    let double = icon.last_left.is_some();
    let target = HWND(icon.hwnd as usize as *mut c_void);
    let mouse = ((y as i16 as u16 as usize) << 16) | (x as i16 as u16 as usize);
    let send = |event: u32| unsafe {
        let (wparam, lparam) = if icon.version >= 4 {
            (mouse, ((icon.uid as u16 as usize) << 16) | event as u16 as usize)
        } else {
            (icon.uid as usize, event as usize)
        };
        let _ = SendNotifyMessageW(target, icon.callback, WPARAM(wparam), LPARAM(lparam as isize));
    };
    unsafe {
        // De app mag zijn menu of venster naar voren halen.
        let _ = AllowSetForegroundWindow(pid_of(target));
    }
    if right {
        send(WM_RBUTTONDOWN);
        send(WM_RBUTTONUP);
        if icon.version >= 3 {
            send(WM_CONTEXTMENU);
        }
    } else if double {
        send(WM_LBUTTONDBLCLK);
        send(WM_LBUTTONUP);
    } else {
        send(WM_LBUTTONDOWN);
        send(WM_LBUTTONUP);
        if icon.version >= 3 {
            send(NIN_SELECT);
        }
    }
}
