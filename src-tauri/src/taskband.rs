//! Wat apps aan "hun taakbalkknop" doorgeven: knipperen (aandacht vragen),
//! een voortgangsbalk (kopiëren, downloads) en een badge (ongelezen berichten).
//!
//! - **Knipperen** (FlashWindowEx) meldt Windows via de shell-hook
//!   (RegisterShellHookWindow): `HSHELL_FLASH` met het venster. Het knipperen
//!   stopt zodra het venster actief wordt of verdwijnt.
//! - **Voortgang en badges** (ITaskbarList3) zoekt shell32 in het proces van
//!   de app: het eerste `Shell_TrayWnd`, daarvan de eigenschap `TaskbandHWND`,
//!   en daarheen gaan berichten `WM_USER + n`. Is het systeemvak van Dockey
//!   aan (zie tray.rs), dan is dat eerste `Shell_TrayWnd` het onze en wijst de
//!   eigenschap naar het venster hier. Alles wat binnenkomt, gaat ook door naar
//!   de taakbalk van Windows, zodat die compleet blijft.

use crate::util::log;
use serde::Serialize;
use std::collections::HashMap;
use std::ffi::c_void;
use std::mem::size_of;
use std::sync::atomic::{AtomicIsize, Ordering};
use std::sync::mpsc::channel;
use std::sync::Mutex;
use std::time::Duration;
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{HANDLE, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::*;

// ITaskbarList3 → taakbalk (zoals de taakbalk ze ontvangt).
const TB_SET_PROGRESS_VALUE: u32 = WM_USER + 64; // lParam = 0..=65535
const TB_SET_PROGRESS_STATE: u32 = WM_USER + 65; // lParam = TBPFLAG
const TB_SET_OVERLAY_ICON: u32 = WM_USER + 79; // lParam = HICON (0 = weg)

// TBPFLAG
const TBPF_NOPROGRESS: u32 = 0;
const TBPF_INDETERMINATE: u32 = 1;
const TBPF_ERROR: u32 = 4;
const TBPF_PAUSED: u32 = 8;

// Shell-hook (winuser.h).
const HSHELL_WINDOWDESTROYED: usize = 2;
const HSHELL_WINDOWACTIVATED: usize = 4;
const HSHELL_RUDEAPPACTIVATED: usize = 0x8004;
const HSHELL_FLASH: usize = 0x8006;

/// Wat een venster aan zijn knop doorgaf.
#[derive(Serialize, Clone, PartialEq, Debug, Default)]
pub struct Badge {
    /// Vraagt aandacht (knippert).
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub flash: bool,
    /// "normal" | "paused" | "error" | "busy" (onbepaald); leeg = geen voortgang.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub progress: String,
    /// 0.0–1.0 (niet bij "busy").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<f32>,
    /// Badge als PNG (data-URL).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub overlay: Option<String>,
}

impl Badge {
    fn is_empty(&self) -> bool {
        !self.flash && self.progress.is_empty() && self.overlay.is_none()
    }
}

#[derive(Default)]
struct Entry {
    badge: Badge,
    state: u32,
    raw_value: u32,
    overlay_handle: isize,
}

static ENTRIES: Mutex<Option<HashMap<isize, Entry>>> = Mutex::new(None);
static HWND_SELF: AtomicIsize = AtomicIsize::new(0);
static SHELLHOOK: AtomicIsize = AtomicIsize::new(0);

fn with_entry(hwnd: isize, f: impl FnOnce(&mut Entry)) {
    let mut all = ENTRIES.lock().unwrap();
    let map = all.get_or_insert_with(HashMap::new);
    let e = map.entry(hwnd).or_default();
    f(e);
    if e.badge.is_empty() {
        map.remove(&hwnd);
    }
}

fn sync_progress(e: &mut Entry) {
    let (label, value) = match e.state {
        TBPF_NOPROGRESS => ("", None),
        s if s & TBPF_INDETERMINATE != 0 => ("busy", None),
        s if s & TBPF_ERROR != 0 => ("error", Some(e.raw_value)),
        s if s & TBPF_PAUSED != 0 => ("paused", Some(e.raw_value)),
        _ => ("normal", Some(e.raw_value)),
    };
    e.badge.progress = label.to_string();
    // Afgerond op hele procenten: anders zou elke kleine stap een update zijn.
    e.badge.value = value.map(|v| ((v.min(65535) as f32 / 65535.0) * 100.0).round() / 100.0);
}

/// Het venster waar ITaskbarList3-berichten heen moeten (voor tray.rs).
pub fn hwnd() -> Option<HWND> {
    let raw = HWND_SELF.load(Ordering::SeqCst);
    (raw != 0).then_some(HWND(raw as *mut c_void))
}

/// Wat elk venster nu aan zijn knop doorgeeft.
pub fn badges() -> HashMap<isize, Badge> {
    ENTRIES
        .lock()
        .unwrap()
        .as_ref()
        .map(|m| m.iter().map(|(h, e)| (*h, e.badge.clone())).collect())
        .unwrap_or_default()
}

/// Een venster is actief geworden: niet meer knipperen.
pub fn activated(hwnd: isize) {
    with_entry(hwnd, |e| e.badge.flash = false);
}

/// Vergeet vensters die niet meer bestaan.
pub fn retain(alive: &[isize]) {
    if let Some(map) = ENTRIES.lock().unwrap().as_mut() {
        map.retain(|h, _| alive.contains(h));
    }
}

/// De taakband van Windows zelf: de eigenschap `TaskbandHWND` van zijn
/// `Shell_TrayWnd`, of anders het knoppenvenster daarbinnen.
fn explorer_taskband() -> Option<HWND> {
    let tray = crate::tray::explorer_tray()?;
    unsafe {
        let prop = GetPropW(tray, w!("TaskbandHWND"));
        if !prop.0.is_null() {
            return Some(HWND(prop.0));
        }
        let rebar = FindWindowExW(Some(tray), None, w!("ReBarWindow32"), PCWSTR::null()).ok()?;
        let band = FindWindowExW(Some(rebar), None, w!("MSTaskSwWClass"), PCWSTR::null()).ok()?;
        (!band.0.is_null()).then_some(band)
    }
}

fn forward(msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    let Some(band) = explorer_taskband() else { return LRESULT(0) };
    let mut result = 0usize;
    unsafe {
        SendMessageTimeoutW(band, msg, wparam, lparam, SMTO_ABORTIFHUNG, 500, Some(&mut result));
    }
    LRESULT(result as isize)
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    let hook = SHELLHOOK.load(Ordering::SeqCst) as u32;
    if msg == hook && hook != 0 {
        let target = lparam.0;
        match wparam.0 {
            HSHELL_FLASH => with_entry(target, |e| e.badge.flash = true),
            HSHELL_WINDOWACTIVATED | HSHELL_RUDEAPPACTIVATED => activated(target),
            HSHELL_WINDOWDESTROYED => {
                if let Some(map) = ENTRIES.lock().unwrap().as_mut() {
                    map.remove(&target);
                }
            }
            _ => {}
        }
        return LRESULT(0);
    }
    match msg {
        TB_SET_PROGRESS_VALUE => {
            with_entry(wparam.0 as isize, |e| {
                e.raw_value = lparam.0 as u32;
                if e.state == TBPF_NOPROGRESS {
                    e.state = 2; // TBPF_NORMAL: een waarde zetten toont de balk
                }
                sync_progress(e);
            });
            forward(msg, wparam, lparam)
        }
        TB_SET_PROGRESS_STATE => {
            with_entry(wparam.0 as isize, |e| {
                e.state = lparam.0 as u32;
                if e.state == TBPF_NOPROGRESS {
                    e.raw_value = 0;
                }
                sync_progress(e);
            });
            forward(msg, wparam, lparam)
        }
        TB_SET_OVERLAY_ICON => {
            let handle = lparam.0;
            let same = ENTRIES
                .lock()
                .unwrap()
                .as_ref()
                .and_then(|m| m.get(&(wparam.0 as isize)))
                .is_some_and(|e| e.overlay_handle == handle && handle != 0);
            if !same {
                let url = if handle == 0 {
                    None
                } else {
                    crate::icons::hicon_data_url(HICON(handle as *mut c_void)).ok()
                };
                with_entry(wparam.0 as isize, |e| {
                    e.overlay_handle = if url.is_some() { handle } else { 0 };
                    e.badge.overlay = url;
                });
            }
            forward(msg, wparam, lparam)
        }
        m if (WM_USER..WM_USER + 200).contains(&m) => forward(msg, wparam, lparam),
        WM_CLOSE => {
            let _ = DestroyWindow(hwnd);
            LRESULT(0)
        }
        WM_DESTROY => {
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

fn run(ready: std::sync::mpsc::Sender<()>) {
    unsafe {
        let instance = GetModuleHandleW(PCWSTR::null()).map(|m| m.into()).ok();
        let class = WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(wndproc),
            hInstance: instance.unwrap_or_default(),
            lpszClassName: w!("DockeyTaskband"),
            ..Default::default()
        };
        RegisterClassExW(&class);
        let hwnd = match CreateWindowExW(
            WS_EX_TOOLWINDOW,
            w!("DockeyTaskband"),
            w!("Dockey taakknoppen"),
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
                log(&format!("taakknoppen: venster maken mislukt: {e}"));
                let _ = ready.send(());
                return;
            }
        };
        SHELLHOOK.store(RegisterWindowMessageW(w!("SHELLHOOK")) as isize, Ordering::SeqCst);
        if !RegisterShellHookWindow(hwnd).as_bool() {
            log("taakknoppen: shell-hook aanmelden mislukt (knipperen werkt niet)");
        }
        // Berichten van apps met een lager integriteitsniveau doorlaten.
        for m in [TB_SET_PROGRESS_VALUE, TB_SET_PROGRESS_STATE, TB_SET_OVERLAY_ICON] {
            let _ = ChangeWindowMessageFilterEx(hwnd, m, MSGFLT_ALLOW, None);
        }
        HWND_SELF.store(hwnd.0 as isize, Ordering::SeqCst);
        let _ = ready.send(());

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        let _ = DeregisterShellHookWindow(hwnd);
        HWND_SELF.store(0, Ordering::SeqCst);
    }
}

/// Start het venster en wacht (kort) tot het bestaat, zodat het systeemvak
/// er meteen naar kan wijzen.
pub fn start() {
    if HWND_SELF.load(Ordering::SeqCst) != 0 {
        return;
    }
    let (tx, rx) = channel();
    if std::thread::Builder::new().name("taakknoppen".into()).spawn(move || run(tx)).is_ok() {
        let _ = rx.recv_timeout(Duration::from_secs(2));
    }
}

/// Laat de vensters van apps opnieuw "TaskbarButtonCreated" ontvangen, zoals
/// na een herstart van Explorer: apps geven dan hun voortgang en badge
/// opnieuw door (nu ook aan ons).
pub fn announce(windows: &[isize]) {
    let msg = unsafe { RegisterWindowMessageW(w!("TaskbarButtonCreated")) };
    for &h in windows {
        unsafe {
            let _ = SendNotifyMessageW(HWND(h as *mut c_void), msg, WPARAM(0), LPARAM(0));
        }
    }
}

/// Zet op `tray` (ons `Shell_TrayWnd`) de verwijzing naar ons venster.
pub fn attach(tray: HWND) {
    if let Some(own) = hwnd() {
        unsafe {
            if let Err(e) = SetPropW(tray, w!("TaskbandHWND"), Some(HANDLE(own.0))) {
                log(&format!("taakknoppen: TaskbandHWND zetten mislukt: {e}"));
            }
        }
    }
}
