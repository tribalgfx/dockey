//! De echte Windows-taakbalk verbergen en weer terugzetten.
//!
//! We wijzigen niets blijvends aan Windows: de taakbalk wordt op "automatisch
//! verbergen" gezet (zodat hij geen schermruimte meer reserveert) en eventueel
//! helemaal onzichtbaar gemaakt. Bij afsluiten zetten we alles terug. De
//! oorspronkelijke stand staat ook op schijf, zodat een crash te herstellen is.

use crate::config::data_dir;
use std::mem::size_of;
use std::sync::Mutex;
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM};
use windows::Win32::UI::Shell::{SHAppBarMessage, ABM_GETSTATE, ABM_SETSTATE, ABS_AUTOHIDE, APPBARDATA};
use windows::Win32::UI::WindowsAndMessaging::{
    FindWindowExW, IsWindowVisible, ShowWindow, SW_HIDE, SW_SHOWNA,
};

/// Oorspronkelijke ABM_GETSTATE-waarde van vóór onze eerste wijziging.
static ORIGINAL: Mutex<Option<u32>> = Mutex::new(None);

fn state_file() -> std::path::PathBuf {
    data_dir().join("taakbalk-origineel.txt")
}

fn tray_windows() -> Vec<HWND> {
    let mut out = Vec::new();
    unsafe {
        // Niet FindWindow: dat kan het systeemvak-venster van Dockey zelf vinden.
        if let Some(h) = crate::tray::explorer_tray() {
            out.push(h);
        }
        // Taakbalken op extra schermen.
        let mut prev: Option<HWND> = None;
        while let Ok(h) = FindWindowExW(None, prev, w!("Shell_SecondaryTrayWnd"), PCWSTR::null()) {
            if h.0.is_null() {
                break;
            }
            out.push(h);
            prev = Some(h);
        }
    }
    out
}

fn appbar_data() -> Option<APPBARDATA> {
    let tray = crate::tray::explorer_tray()?;
    Some(APPBARDATA { cbSize: size_of::<APPBARDATA>() as u32, hWnd: tray, ..Default::default() })
}

fn get_state() -> Option<u32> {
    let mut abd = appbar_data()?;
    Some(crate::tray::bypass(|| unsafe { SHAppBarMessage(ABM_GETSTATE, &mut abd) }) as u32)
}

fn set_state(state: u32) {
    if let Some(mut abd) = appbar_data() {
        abd.lParam = LPARAM(state as isize);
        crate::tray::bypass(|| unsafe { SHAppBarMessage(ABM_SETSTATE, &mut abd) });
    }
}

fn set_visible(visible: bool) {
    for h in tray_windows() {
        unsafe {
            let _ = ShowWindow(h, if visible { SW_SHOWNA } else { SW_HIDE });
        }
    }
}

/// Onthoudt de oorspronkelijke stand (één keer), ook op schijf.
fn remember_original() {
    let mut orig = ORIGINAL.lock().unwrap();
    if orig.is_some() {
        return;
    }
    // Ligt er nog een bestand van een vorige (gecrashte) sessie? Dan is dát de
    // echte oorspronkelijke stand, niet wat Windows nu meldt.
    let from_disk = std::fs::read_to_string(state_file()).ok().and_then(|s| s.trim().parse::<u32>().ok());
    let state = from_disk.or_else(get_state).unwrap_or(0);
    let _ = std::fs::create_dir_all(data_dir());
    let _ = std::fs::write(state_file(), state.to_string());
    *orig = Some(state);
}

/// Past de gekozen modus toe: "hidden", "autohide" of "none".
pub fn apply(mode: &str) {
    match mode {
        "hidden" => {
            remember_original();
            set_state(ABS_AUTOHIDE);
            set_visible(false);
        }
        "autohide" => {
            remember_original();
            set_state(ABS_AUTOHIDE);
            set_visible(true);
        }
        _ => restore(),
    }
}

/// Explorer laat de taakbalk soms vanzelf weer zien (bijv. bij het Startmenu);
/// in de modus "hidden" verbergen we hem dan opnieuw.
pub fn enforce(mode: &str) {
    if mode != "hidden" {
        return;
    }
    for h in tray_windows() {
        unsafe {
            if IsWindowVisible(h).as_bool() {
                let _ = ShowWindow(h, SW_HIDE);
            }
        }
    }
}

/// Zet de taakbalk terug zoals hij was.
pub fn restore() {
    let orig = ORIGINAL.lock().unwrap().take();
    if let Some(state) = orig {
        set_state(state);
        set_visible(true);
    }
    let _ = std::fs::remove_file(state_file());
}

/// Voor `dockey.exe --restore`: herstel zonder dat de app draait.
pub fn restore_from_disk() {
    let state = std::fs::read_to_string(state_file()).ok().and_then(|s| s.trim().parse::<u32>().ok()).unwrap_or(0);
    set_state(state);
    set_visible(true);
    let _ = std::fs::remove_file(state_file());
}
