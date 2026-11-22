//! Onze eigen balk aanmelden als "AppBar", zodat Windows schermruimte vrijhoudt
//! en gemaximaliseerde vensters er niet overheen vallen.

use crate::config::Edge;
use std::mem::size_of;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use windows::core::w;
use windows::Win32::Foundation::{HWND, RECT};
use windows::Win32::Graphics::Gdi::{GetMonitorInfoW, MonitorFromRect, MONITORINFO, MONITOR_DEFAULTTONEAREST};
use windows::Win32::UI::Shell::{
    SHAppBarMessage, ABE_BOTTOM, ABE_LEFT, ABE_RIGHT, ABE_TOP, ABM_NEW, ABM_QUERYPOS, ABM_REMOVE, ABM_SETPOS, APPBARDATA,
};
use windows::Win32::UI::WindowsAndMessaging::{IsWindowVisible, RegisterWindowMessageW};

static ACTIVE: AtomicBool = AtomicBool::new(false);
/// De laatst gereserveerde rand en rechthoek.
static RESERVED: Mutex<Option<(Edge, RECT)>> = Mutex::new(None);

fn data(hwnd: HWND) -> APPBARDATA {
    APPBARDATA { cbSize: size_of::<APPBARDATA>() as u32, hWnd: hwnd, ..Default::default() }
}

/// Reserveert `rect` aan de gegeven schermrand. Geeft de rechthoek terug die
/// Windows ons daadwerkelijk toewijst (kan verschuiven door andere balken).
pub fn reserve(hwnd: HWND, edge: Edge, rect: RECT) -> RECT {
    let rc = crate::tray::bypass(|| unsafe {
        let mut abd = data(hwnd);
        // ABM_NEW faalt onschuldig als we al aangemeld zijn; na een herstart
        // van Explorer is de aanmelding weg en slaagt hij opnieuw.
        abd.uCallbackMessage = RegisterWindowMessageW(w!("DockeyAppBar"));
        SHAppBarMessage(ABM_NEW, &mut abd);

        let thickness = if edge.vertical() { rect.right - rect.left } else { rect.bottom - rect.top };
        abd.uEdge = match edge {
            Edge::Top => ABE_TOP,
            Edge::Bottom => ABE_BOTTOM,
            Edge::Left => ABE_LEFT,
            Edge::Right => ABE_RIGHT,
        };
        abd.rc = rect;
        SHAppBarMessage(ABM_QUERYPOS, &mut abd);
        match edge {
            Edge::Top => abd.rc.bottom = abd.rc.top + thickness,
            Edge::Bottom => abd.rc.top = abd.rc.bottom - thickness,
            Edge::Left => abd.rc.right = abd.rc.left + thickness,
            Edge::Right => abd.rc.left = abd.rc.right - thickness,
        }
        SHAppBarMessage(ABM_SETPOS, &mut abd);
        abd.rc
    });
    ACTIVE.store(true, Ordering::SeqCst);
    *RESERVED.lock().unwrap() = Some((edge, rc));
    rc
}

/// Laat het werkgebied van Windows onze balk niet meer vrij? Bijvoorbeeld
/// omdat de balk verborgen was toen we reserveerden (Explorer telt een
/// verborgen AppBar niet mee), of omdat Explorer opnieuw rekende. Dan vallen
/// gemaximaliseerde vensters over de balk heen.
pub fn lost(hwnd: HWND) -> bool {
    if !ACTIVE.load(Ordering::SeqCst) || !unsafe { IsWindowVisible(hwnd) }.as_bool() {
        return false;
    }
    let Some((edge, rc)) = *RESERVED.lock().unwrap() else { return false };
    let mut mi = MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..Default::default() };
    if !unsafe { GetMonitorInfoW(MonitorFromRect(&rc, MONITOR_DEFAULTTONEAREST), &mut mi) }.as_bool() {
        return false;
    }
    let work = mi.rcWork;
    match edge {
        Edge::Top => work.top < rc.bottom,
        Edge::Bottom => work.bottom > rc.top,
        Edge::Left => work.left < rc.right,
        Edge::Right => work.right > rc.left,
    }
}

pub fn remove(hwnd: HWND) {
    if ACTIVE.swap(false, Ordering::SeqCst) {
        crate::tray::bypass(|| unsafe {
            let mut abd = data(hwnd);
            SHAppBarMessage(ABM_REMOVE, &mut abd);
        });
        *RESERVED.lock().unwrap() = None;
    }
}
