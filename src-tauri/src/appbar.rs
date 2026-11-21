//! Onze eigen balk aanmelden als "AppBar", zodat Windows schermruimte vrijhoudt
//! en gemaximaliseerde vensters er niet overheen vallen.

use crate::config::Edge;
use std::mem::size_of;
use std::sync::atomic::{AtomicBool, Ordering};
use windows::core::w;
use windows::Win32::Foundation::{HWND, RECT};
use windows::Win32::UI::Shell::{
    SHAppBarMessage, ABE_BOTTOM, ABE_LEFT, ABE_RIGHT, ABE_TOP, ABM_NEW, ABM_QUERYPOS, ABM_REMOVE, ABM_SETPOS, APPBARDATA,
};
use windows::Win32::UI::WindowsAndMessaging::RegisterWindowMessageW;

static ACTIVE: AtomicBool = AtomicBool::new(false);

fn data(hwnd: HWND) -> APPBARDATA {
    APPBARDATA { cbSize: size_of::<APPBARDATA>() as u32, hWnd: hwnd, ..Default::default() }
}

/// Reserveert `rect` aan de gegeven schermrand. Geeft de rechthoek terug die
/// Windows ons daadwerkelijk toewijst (kan verschuiven door andere balken).
pub fn reserve(hwnd: HWND, edge: Edge, rect: RECT) -> RECT {
    unsafe {
        let mut abd = data(hwnd);
        // ABM_NEW faalt onschuldig als we al aangemeld zijn; na een herstart
        // van Explorer is de aanmelding weg en slaagt hij opnieuw.
        abd.uCallbackMessage = RegisterWindowMessageW(w!("DockeyAppBar"));
        SHAppBarMessage(ABM_NEW, &mut abd);
        ACTIVE.store(true, Ordering::SeqCst);

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
    }
}

pub fn remove(hwnd: HWND) {
    if ACTIVE.swap(false, Ordering::SeqCst) {
        unsafe {
            let mut abd = data(hwnd);
            SHAppBarMessage(ABM_REMOVE, &mut abd);
        }
    }
}
