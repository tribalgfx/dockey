//! Sneltoetsen nabootsen om Windows-onderdelen te openen (Start, Taakweergave…).

use std::mem::size_of;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, INPUT_MOUSE, KEYBDINPUT, KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP,
    MOUSEINPUT, VIRTUAL_KEY, VK_A, VK_D, VK_LWIN, VK_N, VK_S, VK_TAB,
};

fn key(vk: VIRTUAL_KEY, up: bool) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: vk,
                wScan: 0,
                dwFlags: if up { KEYEVENTF_KEYUP } else { KEYBD_EVENT_FLAGS(0) },
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

fn chord(keys: &[VIRTUAL_KEY]) {
    let mut inputs: Vec<INPUT> = keys.iter().map(|k| key(*k, false)).collect();
    inputs.extend(keys.iter().rev().map(|k| key(*k, true)));
    unsafe {
        SendInput(&inputs, size_of::<INPUT>() as i32);
    }
}

/// Een lege muisinvoer: telt voor Windows als eigen invoer (zodat
/// SetForegroundWindow mag), maar komt bij geen enkele app als toets aan.
/// Een Alt-tik deed hetzelfde, maar liet o.a. de menubalk van Firefox verschijnen.
pub fn nudge_input() {
    let inputs = [INPUT { r#type: INPUT_MOUSE, Anonymous: INPUT_0 { mi: MOUSEINPUT::default() } }];
    unsafe {
        SendInput(&inputs, size_of::<INPUT>() as i32);
    }
}

pub fn send(name: &str) {
    match name {
        "start" => chord(&[VK_LWIN]),
        "search" => chord(&[VK_LWIN, VK_S]),
        "taskview" => chord(&[VK_LWIN, VK_TAB]),
        "desktop" => chord(&[VK_LWIN, VK_D]),
        "quick" => chord(&[VK_LWIN, VK_A]),
        "notifications" => chord(&[VK_LWIN, VK_N]),
        _ => {}
    }
}
