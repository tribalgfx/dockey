//! Open vensters volgen: welke apps draaien er, welke is actief, en acties
//! zoals activeren, minimaliseren en sluiten.

use crate::config::{Config, Edge};
use crate::util::{from_wide, take_co_string};
use serde::Serialize;
use std::collections::HashMap;
use std::ffi::c_void;
use std::mem::size_of;
use windows::core::{w, BOOL, GUID, HSTRING, PCWSTR, PWSTR};
use windows::Win32::Foundation::{CloseHandle, HWND, LPARAM, POINT, PROPERTYKEY, RECT, WPARAM};
use windows::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_CLOAKED, DWMWA_EXTENDED_FRAME_BOUNDS};
use windows::Win32::Graphics::Gdi::{
    EnumDisplayMonitors, GetMonitorInfoW, MonitorFromWindow, HDC, HMONITOR, MONITORINFO, MONITORINFOEXW,
    MONITOR_DEFAULTTONEAREST,
};
use windows::Win32::Storage::Packaging::Appx::GetApplicationUserModelId;
use windows::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::Shell::{
    IShellItem, IShellItem2, SHCreateItemFromParsingName, ShellExecuteW, SIGDN_NORMALDISPLAY,
};
use windows::Win32::UI::WindowsAndMessaging::*;

const MONITORINFOF_PRIMARY: u32 = 1;

/// System.FileDescription
const PKEY_FILE_DESCRIPTION: PROPERTYKEY =
    PROPERTYKEY { fmtid: GUID::from_u128(0x0CEF7D53_FA64_11D1_A203_0000F81FEDEE), pid: 3 };

#[derive(Serialize, Clone, PartialEq, Debug)]
pub struct WinEntry {
    pub hwnd: isize,
    pub title: String,
    pub active: bool,
    pub min: bool,
    /// Scherm waarop het venster (grotendeels) staat, bijv. `\\.\DISPLAY2`.
    pub monitor: String,
    /// Knipperen, voortgang en badge (zie taskband.rs).
    #[serde(flatten)]
    pub badge: crate::taskband::Badge,
}

#[derive(Serialize, Clone, PartialEq, Debug)]
pub struct Group {
    pub key: String,
    pub name: String,
    pub launch: String,
    pub pinned: bool,
    pub windows: Vec<WinEntry>,
}

#[derive(Serialize, Clone, PartialEq, Debug, Default)]
pub struct Snapshot {
    pub groups: Vec<Group>,
    /// Scherm waarop een schermvullend venster (game, video) actief is.
    pub fullscreen: Option<String>,
    /// Schermen waarop een venster tegen de balk aan staat (gemaximaliseerd of
    /// zo groot gesnapt).
    pub maximized: Vec<String>,
}

#[derive(Clone)]
struct Meta {
    key: String,
    launch: String,
}

pub fn hwnd(raw: isize) -> HWND {
    HWND(raw as *mut c_void)
}

unsafe extern "system" fn enum_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
    let list = &mut *(lparam.0 as *mut Vec<HWND>);
    list.push(hwnd);
    BOOL(1)
}

fn top_level_windows() -> Vec<HWND> {
    let mut list: Vec<HWND> = Vec::with_capacity(256);
    unsafe {
        let _ = EnumWindows(Some(enum_proc), LPARAM(&mut list as *mut Vec<HWND> as isize));
    }
    list
}

fn class_name(h: HWND) -> String {
    let mut buf = [0u16; 256];
    let n = unsafe { GetClassNameW(h, &mut buf) };
    String::from_utf16_lossy(&buf[..n.max(0) as usize])
}

fn window_title(h: HWND) -> String {
    let mut buf = [0u16; 512];
    let n = unsafe { GetWindowTextW(h, &mut buf) };
    String::from_utf16_lossy(&buf[..n.max(0) as usize])
}

fn pid_of(h: HWND) -> u32 {
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(h, Some(&mut pid)) };
    pid
}

fn is_cloaked(h: HWND) -> bool {
    let mut cloaked = 0u32;
    unsafe {
        let _ = DwmGetWindowAttribute(h, DWMWA_CLOAKED, &mut cloaked as *mut u32 as *mut c_void, size_of::<u32>() as u32);
    }
    cloaked != 0
}

/// Klassen die nooit een "app-venster" zijn (bureaublad, shell-onderdelen).
const SHELL_CLASSES: &[&str] = &[
    "Progman",
    "WorkerW",
    "Shell_TrayWnd",
    "Shell_SecondaryTrayWnd",
    "Windows.UI.Core.CoreWindow",
    "XamlExplorerHostIslandWindow",
    "MultitaskingViewFrame",
    "ForegroundStaging",
    "TopLevelWindowForOverflowXamlIsland",
];

/// Dezelfde vuistregels als Alt-Tab: zichtbaar, niet "cloaked", geen
/// hulpvenster, geen eigenaar (tenzij expliciet als app-venster gemarkeerd).
fn is_app_window(h: HWND, own_pid: u32) -> bool {
    unsafe {
        if !IsWindowVisible(h).as_bool() {
            return false;
        }
        let ex = GetWindowLongPtrW(h, GWL_EXSTYLE) as u32;
        let app = ex & WS_EX_APPWINDOW.0 != 0;
        if !app && (ex & WS_EX_TOOLWINDOW.0 != 0 || ex & WS_EX_NOACTIVATE.0 != 0) {
            return false;
        }
        if !app {
            if let Ok(owner) = GetWindow(h, GW_OWNER) {
                if !owner.0.is_null() {
                    return false;
                }
            }
        }
        if is_cloaked(h) || GetWindowTextLengthW(h) == 0 {
            return false;
        }
    }
    if pid_of(h) == own_pid {
        return false;
    }
    let class = class_name(h);
    !SHELL_CLASSES.contains(&class.as_str())
}

/// Bepaalt bij welke app een venster hoort. `None` als dat (nog) niet lukt.
fn identify(h: HWND) -> Option<Meta> {
    unsafe {
        let mut pid = pid_of(h);
        if class_name(h) == "ApplicationFrameWindow" {
            // UWP-apps draaien in een gedeeld kadervenster; het echte proces
            // zit achter het CoreWindow-kindvenster.
            let child = FindWindowExW(Some(h), None, w!("Windows.UI.Core.CoreWindow"), PCWSTR::null()).ok()?;
            if child.0.is_null() {
                return None;
            }
            pid = pid_of(child);
        }
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;

        let mut path_buf = [0u16; 1024];
        let mut path_len = path_buf.len() as u32;
        let path = QueryFullProcessImageNameW(process, PROCESS_NAME_WIN32, PWSTR(path_buf.as_mut_ptr()), &mut path_len)
            .ok()
            .map(|_| String::from_utf16_lossy(&path_buf[..path_len as usize]));

        let mut id_buf = [0u16; 256];
        let mut id_len = id_buf.len() as u32;
        let rc = GetApplicationUserModelId(process, &mut id_len, Some(PWSTR(id_buf.as_mut_ptr())));
        let _ = CloseHandle(process);

        if rc.0 == 0 {
            let aumid = from_wide(&id_buf);
            if !aumid.is_empty() {
                return Some(Meta { key: format!("aumid:{aumid}"), launch: format!("shell:AppsFolder\\{aumid}") });
            }
        }
        let path = path?;
        Some(Meta { key: path.to_lowercase(), launch: path })
    }
}

/// Leesbare naam voor een groep (bijv. "Google Chrome" i.p.v. chrome.exe).
pub fn display_name(key: &str) -> String {
    let fallback = || {
        let tail = key.rsplit(['\\', '!', ':']).next().unwrap_or(key);
        tail.trim_end_matches(".exe").to_string()
    };
    let found = unsafe {
        if let Some(aumid) = key.strip_prefix("aumid:") {
            let parsing = HSTRING::from(format!("shell:AppsFolder\\{aumid}"));
            SHCreateItemFromParsingName::<_, _, IShellItem>(&parsing, None)
                .ok()
                .and_then(|item| item.GetDisplayName(SIGDN_NORMALDISPLAY).ok())
                .and_then(|p| take_co_string(p))
        } else {
            SHCreateItemFromParsingName::<_, _, IShellItem2>(&HSTRING::from(key), None)
                .ok()
                .and_then(|item| item.GetString(&PKEY_FILE_DESCRIPTION).ok())
                .and_then(|p| take_co_string(p))
        }
    };
    match found {
        Some(name) if !name.trim().is_empty() => name.trim().to_string(),
        _ => fallback(),
    }
}

fn info_of(mon: HMONITOR) -> Option<MONITORINFOEXW> {
    unsafe {
        let mut mi = MONITORINFOEXW::default();
        mi.monitorInfo.cbSize = size_of::<MONITORINFOEXW>() as u32;
        GetMonitorInfoW(mon, &mut mi as *mut MONITORINFOEXW as *mut MONITORINFO).as_bool().then_some(mi)
    }
}

fn monitor_info(h: HWND) -> Option<MONITORINFOEXW> {
    info_of(unsafe { MonitorFromWindow(h, MONITOR_DEFAULTTONEAREST) })
}

fn device_name(mi: &MONITORINFOEXW) -> String {
    from_wide(&mi.szDevice)
}

/// Een scherm: naam (zoals `\\.\DISPLAY1`), rechthoek en werkgebied.
#[derive(Clone, PartialEq, Debug)]
pub struct Screen {
    pub name: String,
    pub primary: bool,
    pub rect: RECT,
    pub work: RECT,
}

unsafe extern "system" fn monitor_proc(mon: HMONITOR, _: HDC, _: *mut RECT, lparam: LPARAM) -> BOOL {
    let list = &mut *(lparam.0 as *mut Vec<Screen>);
    if let Some(mi) = info_of(mon) {
        list.push(Screen {
            name: device_name(&mi),
            primary: mi.monitorInfo.dwFlags & MONITORINFOF_PRIMARY != 0,
            rect: mi.monitorInfo.rcMonitor,
            work: mi.monitorInfo.rcWork,
        });
    }
    BOOL(1)
}

/// Alle schermen, het primaire eerst.
pub fn screens() -> Vec<Screen> {
    let mut list: Vec<Screen> = Vec::new();
    unsafe {
        let _ = EnumDisplayMonitors(None, None, Some(monitor_proc), LPARAM(&mut list as *mut Vec<Screen> as isize));
    }
    list.sort_by_key(|s| !s.primary);
    list
}

/// Vult dit venster zijn hele scherm (game, video)? Dan dat scherm.
fn fullscreen_on(h: HWND) -> Option<String> {
    if SHELL_CLASSES.contains(&class_name(h).as_str()) {
        return None;
    }
    let mi = monitor_info(h)?;
    let mut r = RECT::default();
    if unsafe { GetWindowRect(h, &mut r) }.is_err() {
        return None;
    }
    let m = mi.monitorInfo.rcMonitor;
    (r.left <= m.left && r.top <= m.top && r.right >= m.right && r.bottom >= m.bottom).then(|| device_name(&mi))
}

/// Staat dit venster op zijn scherm direct tegen de balk aan? Geldt voor
/// gemaximaliseerde vensters, maar ook voor vensters die tegen de rand van de
/// balk gesnapt zijn en die rand over de volle lengte raken.
fn touches_bar(h: HWND, mi: &MONITORINFOEXW, edge: Edge) -> bool {
    if unsafe { IsZoomed(h).as_bool() } {
        return true;
    }
    // De zichtbare rand, zonder de onzichtbare sleeprand van Windows 10/11.
    let mut r = RECT::default();
    let visible = unsafe {
        DwmGetWindowAttribute(h, DWMWA_EXTENDED_FRAME_BOUNDS, &mut r as *mut RECT as *mut c_void, size_of::<RECT>() as u32)
    };
    if visible.is_err() && unsafe { GetWindowRect(h, &mut r) }.is_err() {
        return false;
    }
    const SLACK: i32 = 4;
    let w = mi.monitorInfo.rcWork;
    let full_width = r.left <= w.left + SLACK && r.right >= w.right - SLACK;
    let full_height = r.top <= w.top + SLACK && r.bottom >= w.bottom - SLACK;
    match edge {
        Edge::Top => full_width && r.top <= w.top + SLACK,
        Edge::Bottom => full_width && r.bottom >= w.bottom - SLACK,
        Edge::Left => full_height && r.left <= w.left + SLACK,
        Edge::Right => full_height && r.right >= w.right - SLACK,
    }
}

pub struct Tracker {
    own_pid: u32,
    metas: HashMap<isize, Meta>,
    names: HashMap<String, String>,
    /// Vensters in volgorde van eerste verschijning (stabiele volgorde in de balk).
    seen: Vec<isize>,
    last_active: isize,
    fullscreen_ticks: u8,
    fullscreen_on: Option<String>,
}

impl Tracker {
    pub fn new() -> Self {
        Self {
            own_pid: std::process::id(),
            metas: HashMap::new(),
            names: HashMap::new(),
            seen: Vec::new(),
            last_active: 0,
            fullscreen_ticks: 0,
            fullscreen_on: None,
        }
    }

    fn name_for(&mut self, key: &str) -> String {
        if let Some(n) = self.names.get(key) {
            return n.clone();
        }
        let n = display_name(key);
        self.names.insert(key.to_string(), n.clone());
        n
    }

    pub fn tick(&mut self, cfg: &Config) -> Snapshot {
        let current: Vec<isize> = top_level_windows()
            .into_iter()
            .filter(|h| is_app_window(*h, self.own_pid))
            .map(|h| h.0 as isize)
            .collect();

        self.seen.retain(|h| current.contains(h));
        self.metas.retain(|h, _| current.contains(h));
        for h in &current {
            if !self.seen.contains(h) {
                self.seen.push(*h);
            }
        }

        // Actief venster. Heeft onze eigen balk de focus (er is net op geklikt),
        // dan blijft het vorige venster "actief": zo kan een klik op de actieve
        // app die minimaliseren.
        let fg = unsafe { GetForegroundWindow() };
        let mut fullscreen_now = None;
        if !fg.0.is_null() && pid_of(fg) != self.own_pid {
            if self.last_active != fg.0 as isize {
                // Net actief geworden: dit venster hoeft geen aandacht meer te vragen.
                crate::taskband::activated(fg.0 as isize);
            }
            self.last_active = fg.0 as isize;
            fullscreen_now = fullscreen_on(fg);
        } else if !fg.0.is_null() && self.fullscreen_ticks >= 2 {
            // Onze balk heeft focus; schermvullend blijft zoals het was.
            fullscreen_now = self.fullscreen_on.clone();
        }
        self.fullscreen_ticks = match &fullscreen_now {
            Some(m) if self.fullscreen_on.as_ref() == Some(m) => self.fullscreen_ticks.saturating_add(1).min(3),
            Some(_) => 1,
            None => 0,
        };
        self.fullscreen_on = fullscreen_now;
        crate::taskband::retain(&current);
        let badges = crate::taskband::badges();

        let mut groups: Vec<Group> = cfg
            .pinned
            .iter()
            .map(|p| Group { key: p.key.clone(), name: p.name.clone(), launch: p.launch.clone(), pinned: true, windows: vec![] })
            .collect();

        for raw in self.seen.clone() {
            let h = hwnd(raw);
            let meta = match self.metas.get(&raw) {
                Some(m) => m.clone(),
                None => match identify(h) {
                    Some(m) => {
                        self.metas.insert(raw, m.clone());
                        m
                    }
                    // Nog niet te herleiden (bijv. UWP-app die net start):
                    // tijdelijk onder een eigen sleutel, volgende ronde opnieuw.
                    None => Meta { key: format!("hwnd:{raw}"), launch: String::new() },
                },
            };
            let min = unsafe { IsIconic(h).as_bool() };
            let entry = WinEntry {
                hwnd: raw,
                title: window_title(h),
                active: raw == self.last_active && !min,
                min,
                monitor: monitor_info(h).map(|mi| device_name(&mi)).unwrap_or_default(),
                badge: badges.get(&raw).cloned().unwrap_or_default(),
            };
            match groups.iter_mut().find(|g| g.key == meta.key) {
                Some(g) => g.windows.push(entry),
                None => {
                    let name = if meta.launch.is_empty() { entry.title.clone() } else { self.name_for(&meta.key) };
                    groups.push(Group { key: meta.key, name, launch: meta.launch, pinned: false, windows: vec![entry] });
                }
            }
        }

        let mut maximized: Vec<String> = Vec::new();
        for &raw in &current {
            let h = hwnd(raw);
            if unsafe { IsIconic(h).as_bool() } {
                continue;
            }
            let Some(mi) = monitor_info(h) else { continue };
            let name = device_name(&mi);
            if !maximized.contains(&name) && touches_bar(h, &mi, cfg.edge()) {
                maximized.push(name);
            }
        }
        maximized.sort();

        let fullscreen = if self.fullscreen_ticks >= 2 { self.fullscreen_on.clone() } else { None };
        Snapshot { groups, fullscreen, maximized }
    }
}

/// De app-vensters die nu open zijn (zoals in de balk).
pub fn app_windows() -> Vec<isize> {
    let own = std::process::id();
    top_level_windows().into_iter().filter(|h| is_app_window(*h, own)).map(|h| h.0 as isize).collect()
}

// ---------------------------------------------------------------- acties --

pub fn focus(raw: isize) {
    let h = hwnd(raw);
    unsafe {
        if IsIconic(h).as_bool() {
            let _ = ShowWindow(h, SW_RESTORE);
        }
        let _ = SetForegroundWindow(h);
        if GetForegroundWindow() != h {
            // Windows staat naar voren halen alleen toe na eigen invoer; een
            // lege muisinvoer telt als invoer (de balk zelf pakt geen focus).
            crate::keys::nudge_input();
            let _ = SetForegroundWindow(h);
        }
        if GetForegroundWindow() != h {
            SwitchToThisWindow(h, true);
        }
    }
}

pub fn minimize(raw: isize) {
    unsafe {
        let _ = PostMessageW(Some(hwnd(raw)), WM_SYSCOMMAND, WPARAM(SC_MINIMIZE as usize), LPARAM(0));
    }
}

pub fn close(raw: isize) {
    unsafe {
        let _ = PostMessageW(Some(hwnd(raw)), WM_CLOSE, WPARAM(0), LPARAM(0));
    }
}

/// Opent een bestand, snelkoppeling, shell-pad of URI via de Windows-shell.
pub fn open(target: &str) {
    if target.is_empty() {
        return;
    }
    unsafe {
        ShellExecuteW(None, w!("open"), &HSTRING::from(target), PCWSTR::null(), PCWSTR::null(), SW_SHOWNORMAL);
    }
}

/// Zorgt dat een venster niet in Alt-Tab verschijnt.
pub fn make_tool_window(h: HWND) {
    unsafe {
        let ex = GetWindowLongPtrW(h, GWL_EXSTYLE);
        let new = (ex | WS_EX_TOOLWINDOW.0 as isize) & !(WS_EX_APPWINDOW.0 as isize);
        SetWindowLongPtrW(h, GWL_EXSTYLE, new);
    }
}

/// Klikken op dit venster maakt het niet actief (zoals bij de taakbalk).
pub fn make_no_activate(h: HWND) {
    unsafe {
        let ex = GetWindowLongPtrW(h, GWL_EXSTYLE);
        SetWindowLongPtrW(h, GWL_EXSTYLE, ex | WS_EX_NOACTIVATE.0 as isize);
    }
}

/// Toont of verbergt een venster zonder de focus te verleggen.
pub fn set_shown(h: HWND, shown: bool) {
    unsafe {
        let _ = ShowWindow(h, if shown { SW_SHOWNOACTIVATE } else { SW_HIDE });
    }
}

/// Staat de muisaanwijzer boven dit venster?
pub fn cursor_inside(h: HWND) -> bool {
    let mut p = POINT::default();
    let mut r = RECT::default();
    unsafe {
        GetCursorPos(&mut p).is_ok()
            && GetWindowRect(h, &mut r).is_ok()
            && p.x >= r.left
            && p.x < r.right
            && p.y >= r.top
            && p.y < r.bottom
    }
}

pub fn place(h: HWND, x: i32, y: i32, w: i32, height: i32) {
    unsafe {
        let _ = SetWindowPos(h, Some(HWND_TOPMOST), x, y, w, height, SWP_NOACTIVATE);
    }
}
