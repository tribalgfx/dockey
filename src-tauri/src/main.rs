// Geen consolevenster in release-builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod appbar;
mod backdrop;
mod config;
mod icons;
mod keys;
mod look;
mod media;
mod net;
mod pins;
mod preview;
mod shell;
mod stats;
mod taskband;
mod taskbar;
mod tray;
mod update;
mod util;

use config::{Config, Edge, Pinned};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Emitter, Manager, RunEvent, State, WebviewUrl, WebviewWindowBuilder, WindowEvent};
use tauri_plugin_autostart::ManagerExt;
use windows::Win32::Foundation::{HWND, RECT};
use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};

#[derive(Default)]
struct Popup {
    /// Welk paneel openstaat of het laatst openstond (bijv. "menu:chrome").
    id: String,
    /// De balk die het paneel opende (bijv. "dock" of "dock-DISPLAY2").
    dock: String,
    /// Midden van het aangeklikte element, in schermpixels.
    anchor_x: i32,
    anchor_y: i32,
    visible: bool,
    /// Moment waarop het paneel dichtging doordat het de focus verloor. Een
    /// klik op de knop die het opende sluit het paneel al (focusverlies) vóórdat
    /// de klik zelf binnenkomt; die klik mag het dan niet meteen heropenen.
    blurred_at: Option<Instant>,
}

struct AppState {
    config: Mutex<Config>,
    shell: Mutex<shell::Snapshot>,
    stats: Mutex<stats::Stats>,
    media: Mutex<Option<media::Media>>,
    icons: Mutex<HashMap<String, String>>,
    popup: Mutex<Popup>,
    preview: Mutex<preview::Preview>,
    /// De balk waarboven het voorbeeld staat.
    preview_dock: Mutex<String>,
    /// Een nieuwere release die nog niet geïnstalleerd is.
    update: Mutex<Option<update::Release>>,
    /// De balken: één per scherm (of alleen op het primaire scherm).
    docks: Mutex<Vec<Dock>>,
}

/// Een balkvenster en het scherm waar het bij hoort.
#[derive(Clone, PartialEq, Debug)]
struct Dock {
    /// "dock" op het primaire scherm, "dock-DISPLAY2" enz. op de andere.
    label: String,
    /// Naam van het scherm, zoals `\\.\DISPLAY2`.
    monitor: String,
}

/// De balk op het primaire scherm (met widgets, systeemvak en instellingen).
const MAIN_DOCK: &str = "dock";

fn window_hwnd(app: &AppHandle, label: &str) -> Option<HWND> {
    app.get_webview_window(label).and_then(|w| w.hwnd().ok())
}

fn docks(app: &AppHandle) -> Vec<Dock> {
    app.state::<AppState>().docks.lock().unwrap().clone()
}

/// Zet de vervaging achter een venster weer op zijn plek (na verplaatsen,
/// tonen of verbergen van dat venster).
fn sync_backdrop(app: &AppHandle, label: &str) {
    let handle = app.clone();
    let label = label.to_string();
    let _ = app.run_on_main_thread(move || backdrop::apply(&handle, &label));
}

// ------------------------------------------------------------- indeling --

/// Het Tauri-scherm bij een scherm uit `shell::screens` (zelfde linkerbovenhoek).
fn tauri_monitor(app: &AppHandle, screen: &shell::Screen) -> Option<tauri::Monitor> {
    app.available_monitors()
        .ok()?
        .into_iter()
        .find(|m| m.position().x == screen.rect.left && m.position().y == screen.rect.top)
}

/// Het scherm waar een balk bij hoort.
fn dock_monitor(app: &AppHandle, label: &str) -> Option<tauri::Monitor> {
    let name = docks(app).into_iter().find(|d| d.label == label)?.monitor;
    let screen = shell::screens().into_iter().find(|s| s.name == name)?;
    tauri_monitor(app, &screen)
}

/// Zet de balken op hun plek aan de schermrand en reserveert daar ruimte.
/// Moet op de hoofdthread draaien.
fn layout(app: &AppHandle) {
    let cfg = app.state::<AppState>().config.lock().unwrap().clone();
    for dock in docks(app) {
        let Some(win) = app.get_webview_window(&dock.label) else { continue };
        let Ok(hwnd) = win.hwnd() else { continue };
        let Some(monitor) = dock_monitor(app, &dock.label) else { continue };

        let scale = monitor.scale_factor();
        let (pos, size) = (monitor.position(), monitor.size());
        let thick = (cfg.strip_thickness() * scale).round() as i32;
        let (l, t) = (pos.x, pos.y);
        let (r, b) = (l + size.width as i32, t + size.height as i32);
        let mut rect = match cfg.edge() {
            Edge::Top => RECT { left: l, top: t, right: r, bottom: t + thick },
            Edge::Bottom => RECT { left: l, top: b - thick, right: r, bottom: b },
            Edge::Left => RECT { left: l, top: t, right: l + thick, bottom: b },
            Edge::Right => RECT { left: r - thick, top: t, right: r, bottom: b },
        };

        if cfg.reserve_space {
            rect = appbar::reserve(hwnd, cfg.edge(), rect);
        } else {
            appbar::remove(hwnd);
        }
        shell::place(hwnd, rect.left, rect.top, rect.right - rect.left, rect.bottom - rect.top);
        backdrop::apply(app, &dock.label);
    }
}

/// Label van de balk voor een scherm.
fn dock_label(screen: &shell::Screen) -> String {
    if screen.primary {
        return MAIN_DOCK.to_string();
    }
    let tail: String = screen.name.chars().filter(|c| c.is_ascii_alphanumeric()).collect();
    format!("dock-{tail}")
}

/// Maakt (of sluit) balken zodat er precies één per gewenst scherm is. Niet
/// op de hoofdthread aanroepen: een venster maken wacht op de hoofdthread.
/// Geeft `true` als er iets veranderde.
fn sync_docks(app: &AppHandle) -> bool {
    let all = app.state::<AppState>().config.lock().unwrap().all_monitors;
    let wanted: Vec<Dock> = shell::screens()
        .iter()
        .filter(|s| s.primary || all)
        .map(|s| Dock { label: dock_label(s), monitor: s.name.clone() })
        .collect();
    if wanted.is_empty() {
        return false; // geen schermen gevonden (bijv. tijdens wisselen): niets veranderen
    }
    let current = docks(app);
    if current == wanted {
        return false;
    }

    for old in current.iter().filter(|d| !wanted.iter().any(|w| w.label == d.label)) {
        if let Some(win) = app.get_webview_window(&old.label) {
            if let Ok(h) = win.hwnd() {
                appbar::remove(h);
            }
            backdrop::hide(&old.label);
            let _ = win.destroy();
        }
    }
    // Eerst bewaren: een nieuwe balk vraagt bij het laden al op welk scherm hij staat.
    *app.state::<AppState>().docks.lock().unwrap() = wanted.clone();
    for new in &wanted {
        if app.get_webview_window(&new.label).is_some() {
            continue;
        }
        let built = WebviewWindowBuilder::new(app, &new.label, WebviewUrl::App("index.html".into()))
            .title("Dockey")
            .decorations(false)
            .transparent(true)
            .shadow(false)
            .always_on_top(true)
            .skip_taskbar(true)
            .resizable(false)
            .maximizable(false)
            .minimizable(false)
            .visible(false)
            .focused(false)
            .disable_drag_drop_handler()
            .build();
        match built {
            Ok(win) => {
                let _ = win.show();
                if let Ok(h) = win.hwnd() {
                    shell::make_tool_window(h);
                    shell::make_no_activate(h);
                }
            }
            Err(e) => util::log(&format!("balk voor {} maken mislukt: {e}", new.monitor)),
        }
    }
    true
}

fn request_layout(app: &AppHandle) {
    let handle = app.clone();
    let _ = app.run_on_main_thread(move || layout(&handle));
}

fn apply_config(app: &AppHandle, cfg: &Config) {
    taskbar::apply(&cfg.taskbar);
    tray::set_enabled(cfg.widgets.tray);
    let auto = app.autolaunch();
    if auto.is_enabled().unwrap_or(false) != cfg.autostart {
        let _ = if cfg.autostart { auto.enable() } else { auto.disable() };
    }
    refresh_tray_menu(app); // de taal kan veranderd zijn
    request_layout(app);
}

/// Bewaart een gewijzigde configuratie en vertelt het aan alle vensters.
fn commit_config(app: &AppHandle, cfg: Config) {
    *app.state::<AppState>().config.lock().unwrap() = cfg.clone();
    cfg.save();
    let _ = app.emit("config", &cfg);
}

// ----------------------------------------------------------- commando's --

#[tauri::command]
fn get_initial(window: tauri::WebviewWindow, state: State<AppState>) -> Value {
    let label = window.label();
    let monitor = state.docks.lock().unwrap().iter().find(|d| d.label == label).map(|d| d.monitor.clone());
    json!({
        // Voor een balk: op welk scherm hij staat en of het de hoofdbalk is.
        "monitor": monitor,
        "mainDock": label == MAIN_DOCK,
        "config": *state.config.lock().unwrap(),
        "shell": *state.shell.lock().unwrap(),
        "stats": *state.stats.lock().unwrap(),
        "media": *state.media.lock().unwrap(),
        "acrylic": backdrop::supported(),
        "look": look::read(),
        "systemLanguage": config::system_language(),
        "version": update::current(),
        "update": *state.update.lock().unwrap(),
    })
}

/// Bewaart de uitkomst van een updatecontrole en vertelt het de vensters en
/// het traymenu, als er iets veranderde.
fn set_update(app: &AppHandle, found: Option<update::Release>) {
    let state = app.state::<AppState>();
    let changed = {
        let mut current = state.update.lock().unwrap();
        let changed = *current != found;
        *current = found.clone();
        changed
    };
    if changed {
        let _ = app.emit("update", &found);
        refresh_tray_menu(app);
    }
}

/// "Nu controleren" in de instellingen.
#[tauri::command(async)]
fn check_update(app: AppHandle) -> Result<Value, String> {
    let found = update::check()?;
    set_update(&app, found.clone());
    Ok(json!({ "update": found, "current": update::current() }))
}

/// "Nu bijwerken": downloaden, controleren en de installer starten.
#[tauri::command(async)]
fn install_update(state: State<AppState>) -> Result<(), String> {
    let release = state.update.lock().unwrap().clone().ok_or("ERR_UPDATE_NONE")?;
    update::download_and_start(&release)
}

/// De interface meldt waar zijn glasvlak staat; daar komt de vervaging.
#[tauri::command]
fn set_backdrop(app: AppHandle, window: tauri::WebviewWindow, rect: Option<backdrop::Rect>, radius: f64) {
    let label = window.label();
    backdrop::store(label, rect, radius);
    sync_backdrop(&app, label);
}

#[tauri::command(async)]
fn set_config(app: AppHandle, config: Config) {
    commit_config(&app, config.clone());
    apply_config(&app, &config);
}

#[tauri::command(async)]
fn window_action(hwnd: isize, action: String) {
    match action.as_str() {
        "focus" => shell::focus(hwnd),
        "minimize" => shell::minimize(hwnd),
        "close" => shell::close(hwnd),
        _ => {}
    }
}

#[tauri::command(async)]
fn launch(target: String) {
    shell::open(&target);
}

#[tauri::command(async)]
fn pin(app: AppHandle, state: State<AppState>, key: String) {
    let group = state.shell.lock().unwrap().groups.iter().find(|g| g.key == key).cloned();
    let Some(group) = group else { return };
    if group.launch.is_empty() {
        return;
    }
    let mut cfg = state.config.lock().unwrap().clone();
    if !cfg.pinned.iter().any(|p| p.key == key) {
        cfg.pinned.push(Pinned { key, name: group.name, launch: group.launch });
        commit_config(&app, cfg);
    }
}

#[tauri::command(async)]
fn unpin(app: AppHandle, state: State<AppState>, key: String) {
    let mut cfg = state.config.lock().unwrap().clone();
    cfg.pinned.retain(|p| p.key != key);
    commit_config(&app, cfg);
}

/// Verschuift een vastgemaakte app één plek naar links (-1) of rechts (1).
#[tauri::command(async)]
fn move_pin(app: AppHandle, state: State<AppState>, key: String, delta: i32) {
    let mut cfg = state.config.lock().unwrap().clone();
    let Some(i) = cfg.pinned.iter().position(|p| p.key == key) else { return };
    let j = i as i32 + delta;
    if j >= 0 && (j as usize) < cfg.pinned.len() {
        cfg.pinned.swap(i, j as usize);
        commit_config(&app, cfg);
    }
}

/// Nieuwe volgorde van de vastgemaakte apps (na slepen in de balk). Apps die
/// niet in `keys` staan, houden hun plek achteraan.
#[tauri::command(async)]
fn reorder_pins(app: AppHandle, state: State<AppState>, keys: Vec<String>) {
    let mut cfg = state.config.lock().unwrap().clone();
    let rank = |k: &str| keys.iter().position(|x| x == k).unwrap_or(usize::MAX);
    let before: Vec<String> = cfg.pinned.iter().map(|p| p.key.clone()).collect();
    cfg.pinned.sort_by_key(|p| rank(&p.key)); // stabiel: de rest blijft in volgorde
    if cfg.pinned.iter().map(|p| &p.key).ne(before.iter()) {
        commit_config(&app, cfg);
    }
}

#[tauri::command(async)]
fn get_icon(state: State<AppState>, key: String, hwnd: Option<isize>) -> Result<String, String> {
    if let Some(hit) = state.icons.lock().unwrap().get(&key) {
        return Ok(hit.clone());
    }
    let url = icons::icon_data_url(&key, hwnd, 64)?;
    state.icons.lock().unwrap().insert(key, url.clone());
    Ok(url)
}

/// Haalt een adres op voor een eigen widget (zie net.rs).
#[tauri::command(async)]
fn fetch_url(url: String, insecure: bool) -> Result<String, String> {
    net::fetch(&url, insecure)
}

#[tauri::command(async)]
fn fetch_image(url: String, insecure: bool) -> Result<String, String> {
    net::fetch_image(&url, insecure)
}

#[tauri::command]
fn tray_icons() -> Vec<tray::TrayItem> {
    tray::items()
}

/// `x`/`y` = midden van het aangeklikte pictogram, in CSS-pixels binnen het venster.
#[tauri::command(async)]
fn tray_click(window: tauri::WebviewWindow, key: String, right: bool, x: f64, y: f64) {
    let scale = window.scale_factor().unwrap_or(1.0);
    let origin = window.inner_position().map(|p| (p.x, p.y)).unwrap_or((0, 0));
    tray::click(&key, right, origin.0 + (x * scale).round() as i32, origin.1 + (y * scale).round() as i32);
}

/// Waar het systeemvak in de balk staat (CSS-pixels in het balkvenster).
#[tauri::command(async)]
fn tray_rect(window: tauri::WebviewWindow, left: f64, top: f64, right: f64, bottom: f64) {
    let scale = window.scale_factor().unwrap_or(1.0);
    let (ox, oy) = window.inner_position().map(|p| (p.x, p.y)).unwrap_or((0, 0));
    let px = |v: f64| (v * scale).round() as i32;
    tray::set_rect(Some((ox + px(left), oy + px(top), ox + px(right), oy + px(bottom))));
}

#[tauri::command(async)]
fn send_keys(name: String) {
    keys::send(&name);
}

#[tauri::command(async)]
fn media_cmd(cmd: String) {
    let _ = media::command(&cmd);
}

#[tauri::command(async)]
fn set_volume(level: f32) {
    let _ = stats::set_volume(level);
}

#[tauri::command(async)]
fn set_mute(muted: bool) {
    let _ = stats::set_mute(muted);
}

#[tauri::command]
fn quit(app: AppHandle) {
    app.exit(0);
}

// ------------------------------------------------------------- paneel --
//
// Menu's, instellingen en de kalender staan in een tweede, los venster dat
// boven (of onder) de balk verschijnt. De balk vraagt het aan met
// `open_popup`; het paneel tekent de inhoud, meet zichzelf op en meldt zich
// met `popup_ready`, waarna wij het op de juiste plek tonen.

/// `anchor_x`/`anchor_y` = midden van het aangeklikte element, in CSS-pixels
/// binnen de balk. Geeft `false` terug als het paneel juist dichtging door
/// dezelfde klik.
#[tauri::command(async)]
#[allow(clippy::too_many_arguments)]
fn open_popup(
    app: AppHandle,
    window: tauri::WebviewWindow,
    state: State<AppState>,
    id: String,
    kind: String,
    payload: Value,
    anchor_x: f64,
    anchor_y: f64,
) -> bool {
    // Het paneel hoort bij de balk die erom vroeg (het paneel zelf kan ook
    // een paneel openen, bijv. een submenu: dan dezelfde balk als nu).
    let label = if window.label().starts_with(MAIN_DOCK) {
        window.label().to_string()
    } else {
        let current = state.popup.lock().unwrap().dock.clone();
        if current.is_empty() { MAIN_DOCK.to_string() } else { current }
    };
    let Some(dock) = app.get_webview_window(&label) else { return false };
    hide_preview(&app);
    let scale = dock.scale_factor().unwrap_or(1.0);
    let origin = dock.outer_position().map(|p| (p.x, p.y)).unwrap_or((0, 0));
    {
        let mut popup = state.popup.lock().unwrap();
        let just_blurred = popup.blurred_at.map(|t| t.elapsed() < Duration::from_millis(350)).unwrap_or(false);
        if popup.id == id && popup.dock == label && (popup.visible || just_blurred) {
            drop(popup);
            hide_popup(&app, false);
            return false;
        }
        popup.id = id.clone();
        popup.dock = label.clone();
        popup.anchor_x = origin.0 + (anchor_x * scale).round() as i32;
        popup.anchor_y = origin.1 + (anchor_y * scale).round() as i32;
    }
    // Vrije hoogte voor het paneel, in CSS-pixels: het paneel maakt zichzelf
    // nooit hoger dan dat. Boven/onder: de ruimte tussen balk en andere rand;
    // links/rechts: de hele schermhoogte.
    let edge = state.config.lock().unwrap().edge();
    let max_height = match (dock_monitor(&app, &label).ok_or(()).map(Some), dock.outer_position(), dock.outer_size()) {
        (Ok(Some(m)), Ok(pos), Ok(size)) => {
            let free = match edge {
                Edge::Top => m.position().y + m.size().height as i32 - (pos.y + size.height as i32),
                Edge::Bottom => pos.y - m.position().y,
                Edge::Left | Edge::Right => m.size().height as i32,
            };
            free as f64 / scale
        }
        _ => 600.0,
    };
    let _ = app.emit_to("popup", "popup:render", json!({ "id": id, "kind": kind, "payload": payload, "maxHeight": max_height }));
    true
}

/// Linksboven voor een venster van `w`×`h` fysieke pixels naast de balk
/// `label`, gecentreerd op `anchor` (schermpixels) en binnen het scherm.
fn beside_dock(app: &AppHandle, label: &str, anchor: (i32, i32), w: i32, h: i32) -> Option<(i32, i32)> {
    let dock = app.get_webview_window(label)?;
    let monitor = dock_monitor(app, label)?;
    let (dock_pos, dock_size) = (dock.outer_position().ok()?, dock.outer_size().ok()?);
    let (mon_l, mon_t) = (monitor.position().x, monitor.position().y);
    let (mon_r, mon_b) = (mon_l + monitor.size().width as i32, mon_t + monitor.size().height as i32);
    let edge = app.state::<AppState>().config.lock().unwrap().edge();
    let centered_x = (anchor.0 - w / 2).min(mon_r - w).max(mon_l);
    let centered_y = (anchor.1 - h / 2).min(mon_b - h).max(mon_t);
    Some(match edge {
        Edge::Top => (centered_x, dock_pos.y + dock_size.height as i32),
        Edge::Bottom => (centered_x, (dock_pos.y - h).max(mon_t)),
        Edge::Left => (dock_pos.x + dock_size.width as i32, centered_y),
        Edge::Right => ((dock_pos.x - w).max(mon_l), centered_y),
    })
}

/// Schaal van het scherm met de balk `label`.
fn dock_scale(app: &AppHandle, label: &str) -> f64 {
    dock_monitor(app, label).map(|m| m.scale_factor()).unwrap_or(1.0)
}

/// `width`/`height` = gemeten grootte van de paneelinhoud in CSS-pixels.
#[tauri::command(async)]
fn popup_ready(app: AppHandle, state: State<AppState>, width: f64, height: f64) {
    let Some(popup) = app.get_webview_window("popup") else { return };
    let Ok(hwnd) = popup.hwnd() else { return };
    let (anchor, label) = {
        let mut p = state.popup.lock().unwrap();
        p.visible = true;
        p.blurred_at = None;
        ((p.anchor_x, p.anchor_y), p.dock.clone())
    };
    let scale = dock_scale(&app, &label);
    let (w, h) = ((width * scale).ceil() as i32, (height * scale).ceil() as i32);
    // Naast de balk, gecentreerd op het aangeklikte element, binnen het scherm.
    let Some((x, y)) = beside_dock(&app, &label, anchor, w, h) else { return };

    shell::place(hwnd, x, y, w, h);
    let _ = popup.show();
    let _ = popup.set_focus();
    sync_backdrop(&app, "popup");
    shell::make_tool_window(hwnd);
}

fn hide_popup(app: &AppHandle, by_blur: bool) {
    let state = app.state::<AppState>();
    {
        let mut p = state.popup.lock().unwrap();
        if !p.visible {
            return;
        }
        p.visible = false;
        p.blurred_at = by_blur.then(Instant::now);
    }
    if let Some(popup) = app.get_webview_window("popup") {
        let _ = popup.hide();
    }
    backdrop::hide("popup");
    let _ = app.emit("popup:closed", ());
}

#[tauri::command(async)]
fn close_popup(app: AppHandle) {
    hide_popup(&app, false);
}

// ---------------------------------------------------------- voorbeelden --
//
// Live voorbeelden van de vensters van een app als je de muis boven het
// icoon houdt (zie preview.rs). De balk regelt het hoveren (vertraging,
// speling); hier koppelen we de thumbnails en tonen we het venster zonder
// het te activeren.

#[derive(serde::Deserialize, serde::Serialize, Clone)]
struct PreviewWindow {
    hwnd: isize,
    title: String,
    #[serde(default)]
    active: bool,
    /// Breedte/hoogte van het venster; ontbreekt als er geen beeld is.
    #[serde(default)]
    ratio: Option<f64>,
}

/// Plek van een thumbnail binnen het voorbeeldvenster, in CSS-pixels.
#[derive(serde::Deserialize)]
struct PreviewSlot {
    hwnd: isize,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
}

/// `anchor_x`/`anchor_y` = midden van het icoon, in CSS-pixels binnen de balk.
/// Geeft `false` als er geen voorbeeld komt (uitgezet, of er staat een paneel open).
#[tauri::command(async)]
#[allow(clippy::too_many_arguments)]
fn open_preview(
    app: AppHandle,
    window: tauri::WebviewWindow,
    state: State<AppState>,
    key: String,
    name: String,
    icon: Option<String>,
    mut windows: Vec<PreviewWindow>,
    anchor_x: f64,
    anchor_y: f64,
) -> bool {
    if !state.config.lock().unwrap().previews || state.popup.lock().unwrap().visible || windows.is_empty() {
        return false;
    }
    let Some(dest) = window_hwnd(&app, "preview") else { return false };
    let dock = window;
    *state.preview_dock.lock().unwrap() = dock.label().to_string();
    let scale = dock.scale_factor().unwrap_or(1.0);
    let origin = dock.outer_position().map(|p| (p.x, p.y)).unwrap_or((0, 0));
    let max_width =
        dock_monitor(&app, dock.label()).map(|m| m.size().width as f64 / m.scale_factor()).unwrap_or(1200.0);

    let serial = {
        let mut p = state.preview.lock().unwrap();
        p.serial += 1;
        p.key = key.clone();
        p.anchor = (origin.0 + (anchor_x * scale).round() as i32, origin.1 + (anchor_y * scale).round() as i32);
        let hwnds: Vec<isize> = windows.iter().map(|w| w.hwnd).collect();
        let ratios = p.sync(dest, &hwnds);
        for w in &mut windows {
            w.ratio = ratios.get(&w.hwnd).copied();
        }
        p.serial
    };
    let _ = app.emit_to(
        "preview",
        "preview:render",
        json!({ "serial": serial, "key": key, "name": name, "icon": icon, "windows": windows, "maxWidth": max_width }),
    );
    true
}

/// De interface heeft de voorbeelden getekend: `width`/`height` = grootte van
/// het venster, `slots` = waar de thumbnails komen (alles in CSS-pixels).
#[tauri::command(async)]
fn preview_ready(app: AppHandle, state: State<AppState>, serial: u64, width: f64, height: f64, slots: Vec<PreviewSlot>) {
    let Some(hwnd) = window_hwnd(&app, "preview") else { return };
    let label = state.preview_dock.lock().unwrap().clone();
    let scale = dock_scale(&app, &label);
    let px = |v: f64| (v * scale).round() as i32;
    let (w, h) = ((width * scale).ceil() as i32, (height * scale).ceil() as i32);
    {
        // Vergrendeld tot het venster staat: een `hide_preview` die intussen
        // binnenkomt, mag het niet vóór ons verbergen.
        let mut p = state.preview.lock().unwrap();
        if p.serial != serial {
            return;
        }
        let slots: Vec<preview::Slot> = slots
            .iter()
            .map(|s| preview::Slot {
                hwnd: s.hwnd,
                rect: RECT { left: px(s.x), top: px(s.y), right: px(s.x + s.width), bottom: px(s.y + s.height) },
            })
            .collect();
        let Some((x, y)) = beside_dock(&app, &label, p.anchor, w, h) else { return };
        p.place(&slots);
        shell::place(hwnd, x, y, w, h);
        shell::set_shown(hwnd, true);
        p.visible = true;
    }
    sync_backdrop(&app, "preview");
}

/// Verbergt het voorbeeld en laat alle thumbnails los.
fn hide_preview(app: &AppHandle) {
    let state = app.state::<AppState>();
    let was_visible = {
        let mut p = state.preview.lock().unwrap();
        p.serial += 1;
        let was = p.visible;
        p.visible = false;
        if was {
            if let Some(h) = window_hwnd(app, "preview") {
                shell::set_shown(h, false);
            }
        }
        p.clear();
        was
    };
    if was_visible {
        backdrop::hide("preview");
        let _ = app.emit("preview:closed", ());
    }
}

#[tauri::command(async)]
fn close_preview(app: AppHandle) {
    hide_preview(&app);
}

/// Vangnet naast de muisgebeurtenissen van de interface: staat de muis een
/// tijdje buiten zowel de balk als het voorbeeld, dan gaat het dicht.
fn preview_watch(app: &AppHandle, outside: &mut u32) {
    if !app.state::<AppState>().preview.lock().unwrap().visible {
        *outside = 0;
        return;
    }
    let over = std::iter::once("preview".to_string())
        .chain(docks(app).into_iter().map(|d| d.label))
        .any(|l| window_hwnd(app, &l).is_some_and(shell::cursor_inside));
    *outside = if over { 0 } else { *outside + 1 };
    if *outside >= 3 {
        *outside = 0;
        hide_preview(app);
    }
}

// ----------------------------------------------------- achtergrondwerk --

/// Volgt vensters (4× per seconde) en bewaakt taakbalk en schermindeling.
fn shell_thread(app: AppHandle) {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
    }
    let state = app.state::<AppState>();

    // Eerste start: neem de apps over die aan de Windows-taakbalk vastzitten.
    if !state.config.lock().unwrap().pins_imported {
        let mut cfg = state.config.lock().unwrap().clone();
        cfg.pinned = pins::import_windows_pins();
        cfg.pins_imported = true;
        commit_config(&app, cfg);
    }

    let mut tracker = shell::Tracker::new();
    let mut geometry = Vec::new();
    let mut was_fullscreen: Option<String> = None;
    let mut all_monitors = state.config.lock().unwrap().all_monitors;
    let mut tick: u32 = 0;
    let mut outside: u32 = 0;
    let mut system_look = look::read();
    loop {
        let cfg = state.config.lock().unwrap().clone();
        let snapshot = tracker.tick(&cfg);

        // Een balk erbij of eraf (instelling "alle schermen" gewijzigd).
        if cfg.all_monitors != all_monitors {
            all_monitors = cfg.all_monitors;
            if sync_docks(&app) {
                request_layout(&app);
                was_fullscreen = None;
            }
        }

        if snapshot.fullscreen != was_fullscreen {
            // Schermvullende game of video: de balk op dat scherm gaat uit de weg.
            if snapshot.fullscreen.is_some() {
                hide_popup(&app, false);
                hide_preview(&app);
            }
            for dock in docks(&app) {
                let hide = snapshot.fullscreen.as_ref() == Some(&dock.monitor);
                if let Some(h) = window_hwnd(&app, &dock.label) {
                    shell::set_shown(h, !hide);
                }
                sync_backdrop(&app, &dock.label);
            }
            was_fullscreen = snapshot.fullscreen.clone();
        }

        let changed = {
            let mut current = state.shell.lock().unwrap();
            let changed = *current != snapshot;
            if changed {
                *current = snapshot.clone();
            }
            changed
        };
        if changed {
            let _ = app.emit("shell", &snapshot);
        }

        taskbar::enforce(&cfg.taskbar);
        preview_watch(&app, &mut outside);

        // Elke 2 s: is het scherm of het werkgebied veranderd (resolutie,
        // schaal, herstart van Explorer)? Dan de balk opnieuw plaatsen.
        // Ook als Windows onze gereserveerde ruimte kwijt is.
        // Ook schermen die erbij komen of weggaan: dan balken maken of sluiten.
        if tick % 8 == 0 {
            let now = shell::screens();
            let lost = docks(&app).iter().any(|d| window_hwnd(&app, &d.label).is_some_and(appbar::lost));
            if now != geometry || lost {
                geometry = now;
                sync_docks(&app);
                request_layout(&app);
            }
            // Licht/donker of accentkleur van Windows gewijzigd (thema Automatisch).
            let now = look::read();
            if now != system_look {
                system_look = now;
                let _ = app.emit("look", &system_look);
            }
        }
        tick = tick.wrapping_add(1);
        std::thread::sleep(Duration::from_millis(250));
    }
}

fn stats_thread(app: AppHandle) {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
    }
    let state = app.state::<AppState>();
    let mut sampler = stats::Sampler::new();
    loop {
        std::thread::sleep(Duration::from_millis(1500));
        let s = sampler.sample();
        *state.stats.lock().unwrap() = s.clone();
        let _ = app.emit_to("dock", "stats", &s);
    }
}

fn media_thread(app: AppHandle) {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
    }
    let state = app.state::<AppState>();
    let mut watcher = media::Watcher::new();
    loop {
        let now = watcher.poll();
        let changed = {
            let mut current = state.media.lock().unwrap();
            let changed = *current != now;
            if changed {
                *current = now.clone();
            }
            changed
        };
        if changed {
            let _ = app.emit_to("dock", "media", &now);
        }
        std::thread::sleep(Duration::from_millis(1000));
    }
}

// --------------------------------------------------------------- start --

const TRAY_ID: &str = "dockey";

/// Het menu van het systeemvakicoon, in de taal van de interface. Staat er een
/// update klaar, dan staat die bovenaan.
fn tray_menu(app: &AppHandle, lang: &str) -> tauri::Result<Menu<tauri::Wry>> {
    let (settings, quit) = match lang {
        "nl" => ("Instellingen", "Dockey afsluiten"),
        _ => ("Settings", "Exit Dockey"),
    };
    let settings = MenuItem::with_id(app, "settings", settings, true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", quit, true, None::<&str>)?;
    let pending = app.state::<AppState>().update.lock().unwrap().as_ref().map(|r| r.version.clone());
    match pending {
        Some(version) => {
            let label = match lang {
                "nl" => format!("Bijwerken naar versie {version}…"),
                _ => format!("Update to version {version}…"),
            };
            let update = MenuItem::with_id(app, "update", label, true, None::<&str>)?;
            Menu::with_items(app, &[&update, &settings, &quit])
        }
        None => Menu::with_items(app, &[&settings, &quit]),
    }
}

fn refresh_tray_menu(app: &AppHandle) {
    let lang = app.state::<AppState>().config.lock().unwrap().ui_language();
    if let (Some(tray), Ok(menu)) = (app.tray_by_id(TRAY_ID), tray_menu(app, lang)) {
        let _ = tray.set_menu(Some(menu));
    }
}

/// Een minuut na het starten en daarna eens per dag: is er een nieuwe versie?
fn update_thread(app: AppHandle) {
    std::thread::sleep(Duration::from_secs(60));
    loop {
        let enabled = app.state::<AppState>().config.lock().unwrap().update_check;
        if enabled {
            match update::check() {
                Ok(found) => set_update(&app, found),
                Err(e) => util::log(&format!("updatecontrole mislukt: {e}")),
            }
        }
        std::thread::sleep(Duration::from_secs(24 * 60 * 60));
    }
}

fn setup(app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    // Wie met --quit hier belandt, is zelf de enige Dockey geworden (de andere
    // stopte net): er is niets af te sluiten en niets te starten.
    if std::env::args().any(|a| a == "--quit") {
        std::process::exit(0);
    }
    let handle = app.handle().clone();
    let cfg = handle.state::<AppState>().config.lock().unwrap().clone();

    // De hoofdbalk (uit tauri.conf.json) staat op het primaire scherm; balken
    // voor andere schermen maakt de shell-thread erbij (zie sync_docks).
    let primary = shell::screens().into_iter().find(|s| s.primary).map(|s| s.name).unwrap_or_default();
    *handle.state::<AppState>().docks.lock().unwrap() = vec![Dock { label: MAIN_DOCK.into(), monitor: primary }];

    taskbar::apply(&cfg.taskbar);
    // Vóór het systeemvak: dat wijst apps naar dit venster voor voortgang en badges.
    taskband::start();
    tray::init(handle.clone());
    tray::set_enabled(cfg.widgets.tray);
    // Autostart gelijktrekken met de instelling (o.a. na het overnemen van
    // Lichtbalk-instellingen, of als het exe-bestand verplaatst is).
    let auto = handle.autolaunch();
    if cfg.autostart {
        let _ = auto.enable(); // ook opnieuw, zodat het pad naar dockey.exe klopt
    } else if auto.is_enabled().unwrap_or(false) {
        let _ = auto.disable();
    }
    layout(&handle);
    if let Some(dock) = handle.get_webview_window("dock") {
        let _ = dock.show();
    }
    // Nogmaals nu de balk zichtbaar is: Explorer telt een verborgen AppBar niet
    // mee bij het werkgebied.
    layout(&handle);
    backdrop::apply(&handle, "dock");
    // Pas ná het tonen: het vensterframework zet de vensterstijl opnieuw
    // wanneer het een venster toont.
    for label in ["dock", "popup", "preview"] {
        if let Some(h) = window_hwnd(&handle, label) {
            shell::make_tool_window(h);
        }
    }
    // De balk pakt bij een klik de focus niet, net als de Windows-taakbalk.
    // Zo blijft een open Startmenu of paneel open tot de klik zelf is
    // verwerkt, en sluit een tweede klik op dezelfde knop het weer.
    // Het voorbeeldvenster (bij hoveren) ook niet.
    for label in ["dock", "preview"] {
        if let Some(h) = window_hwnd(&handle, label) {
            shell::make_no_activate(h);
        }
    }

    // Systeemvakicoon als noodrem: instellingen openen of afsluiten.
    let menu = tray_menu(&handle, cfg.ui_language())?;
    let mut tray = TrayIconBuilder::with_id(TRAY_ID).tooltip("Dockey").menu(&menu).on_menu_event(|app, event| {
        match event.id().as_ref() {
            "settings" => {
                let _ = app.emit_to("dock", "open-settings", ());
            }
            "update" => {
                let _ = app.emit_to("dock", "open-update", ());
            }
            "quit" => app.exit(0),
            _ => {}
        }
    });
    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }
    tray.build(app)?;

    for worker in [shell_thread as fn(AppHandle), stats_thread, media_thread, update_thread] {
        let h = handle.clone();
        std::thread::spawn(move || worker(h));
    }
    Ok(())
}

/// Naam van de mutex die een draaiende Dockey vasthoudt. Eigen en vast, zodat
/// het niet afhangt van hoe de single-instance-plug-in de zijne noemt; de
/// installer (installer\Dockey.iss) kijkt naar dezelfde naam.
const RUNNING_MUTEX: windows::core::PCWSTR = windows::core::w!("Dockey.Running");

/// Draait Dockey al?
fn already_running() -> bool {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Threading::{OpenMutexW, SYNCHRONIZATION_SYNCHRONIZE};
    match unsafe { OpenMutexW(SYNCHRONIZATION_SYNCHRONIZE, false, RUNNING_MUTEX) } {
        Ok(h) => {
            unsafe {
                let _ = CloseHandle(h);
            }
            true
        }
        Err(_) => false,
    }
}

/// Houdt de mutex vast zolang dit proces leeft (het handvat gaat nooit dicht).
fn mark_running() {
    use windows::Win32::System::Threading::CreateMutexW;
    if let Err(e) = unsafe { CreateMutexW(None, false, RUNNING_MUTEX) } {
        util::log(&format!("mutex maken mislukt: {e}"));
    }
}

fn main() {
    // `dockey.exe --restore` zet de Windows-taakbalk terug en stopt.
    if std::env::args().any(|a| a == "--restore") {
        taskbar::restore_from_disk();
        return;
    }
    // `dockey.exe --quit` sluit een draaiende Dockey netjes af (de taakbalk komt
    // terug); de installer gebruikt dat. Het verzoek gaat via de single-instance-
    // plug-in naar de draaiende Dockey. Draait er geen, dan stoppen we meteen:
    // anders zou dit Dockey juist starten (zie ook het begin van `setup`).
    let quit_requested = std::env::args().any(|a| a == "--quit");
    if quit_requested && !already_running() {
        return;
    }
    if !quit_requested {
        mark_running();
    }

    // Ook bij een crash de taakbalk teruggeven.
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        taskbar::restore();
        default_hook(info);
    }));

    config::migrate_from_lichtbalk();
    let state = AppState {
        config: Mutex::new(Config::load()),
        shell: Mutex::default(),
        stats: Mutex::default(),
        media: Mutex::default(),
        icons: Mutex::default(),
        popup: Mutex::default(),
        preview: Mutex::default(),
        preview_dock: Mutex::default(),
        update: Mutex::default(),
        docks: Mutex::default(),
    };

    let app = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, args, _| {
            if args.iter().any(|a| a == "--quit") {
                app.exit(0);
            }
        }))
        .plugin(tauri_plugin_autostart::init(tauri_plugin_autostart::MacosLauncher::LaunchAgent, None))
        .manage(state)
        .invoke_handler(tauri::generate_handler![
            get_initial,
            set_config,
            window_action,
            launch,
            pin,
            unpin,
            move_pin,
            reorder_pins,
            get_icon,
            fetch_url,
            fetch_image,
            tray_icons,
            tray_click,
            tray_rect,
            send_keys,
            media_cmd,
            set_volume,
            set_mute,
            quit,
            open_popup,
            popup_ready,
            close_popup,
            open_preview,
            preview_ready,
            close_preview,
            set_backdrop,
            check_update,
            install_update,
        ])
        .on_window_event(|window, event| match event {
            // De vensters zelf sluiten nooit; afsluiten gaat via het menu.
            WindowEvent::CloseRequested { api, .. } => api.prevent_close(),
            WindowEvent::Focused(false) if window.label() == "popup" => hide_popup(window.app_handle(), true),
            _ => {}
        })
        .setup(setup)
        .build(tauri::generate_context!())
        .expect("Dockey kon niet starten");

    app.run(|app, event| {
        if let RunEvent::Exit = event {
            tray::stop();
            for dock in docks(app) {
                if let Some(h) = window_hwnd(app, &dock.label) {
                    appbar::remove(h);
                }
            }
            taskbar::restore();
        }
    });
}
