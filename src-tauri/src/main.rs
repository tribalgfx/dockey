// Geen consolevenster in release-builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod appbar;
mod backdrop;
mod config;
mod icons;
mod keys;
mod media;
mod net;
mod pins;
mod shell;
mod stats;
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
use tauri::{AppHandle, Emitter, Manager, RunEvent, State, WindowEvent};
use tauri_plugin_autostart::ManagerExt;
use windows::Win32::Foundation::{HWND, RECT};
use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};

#[derive(Default)]
struct Popup {
    /// Welk paneel openstaat of het laatst openstond (bijv. "menu:chrome").
    id: String,
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
    /// Een nieuwere release die nog niet geïnstalleerd is.
    update: Mutex<Option<update::Release>>,
}

fn window_hwnd(app: &AppHandle, label: &str) -> Option<HWND> {
    app.get_webview_window(label).and_then(|w| w.hwnd().ok())
}

/// Zet de vervaging achter een venster weer op zijn plek (na verplaatsen,
/// tonen of verbergen van dat venster).
fn sync_backdrop(app: &AppHandle, label: &'static str) {
    let handle = app.clone();
    let _ = app.run_on_main_thread(move || backdrop::apply(&handle, label));
}

// ------------------------------------------------------------- indeling --

/// Zet de balk op zijn plek aan de schermrand en reserveert daar ruimte.
/// Moet op de hoofdthread draaien.
fn layout(app: &AppHandle) {
    let cfg = app.state::<AppState>().config.lock().unwrap().clone();
    let Some(win) = app.get_webview_window("dock") else { return };
    let Ok(hwnd) = win.hwnd() else { return };
    let Ok(Some(monitor)) = win.primary_monitor() else { return };

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
    backdrop::apply(app, "dock");
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
fn get_initial(state: State<AppState>) -> Value {
    json!({
        "config": *state.config.lock().unwrap(),
        "shell": *state.shell.lock().unwrap(),
        "stats": *state.stats.lock().unwrap(),
        "media": *state.media.lock().unwrap(),
        "acrylic": backdrop::supported(),
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
fn set_backdrop(app: AppHandle, window: tauri::WebviewWindow, rect: Option<backdrop::Rect>, round: bool) {
    let label: &'static str = match window.label() {
        "dock" => "dock",
        "popup" => "popup",
        _ => return,
    };
    backdrop::store(label, rect, round);
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
fn open_popup(
    app: AppHandle,
    state: State<AppState>,
    id: String,
    kind: String,
    payload: Value,
    anchor_x: f64,
    anchor_y: f64,
) -> bool {
    let Some(dock) = app.get_webview_window("dock") else { return false };
    let scale = dock.scale_factor().unwrap_or(1.0);
    let origin = dock.outer_position().map(|p| (p.x, p.y)).unwrap_or((0, 0));
    {
        let mut popup = state.popup.lock().unwrap();
        let just_blurred = popup.blurred_at.map(|t| t.elapsed() < Duration::from_millis(350)).unwrap_or(false);
        if popup.id == id && (popup.visible || just_blurred) {
            drop(popup);
            hide_popup(&app, false);
            return false;
        }
        popup.id = id.clone();
        popup.anchor_x = origin.0 + (anchor_x * scale).round() as i32;
        popup.anchor_y = origin.1 + (anchor_y * scale).round() as i32;
    }
    // Vrije hoogte voor het paneel, in CSS-pixels: het paneel maakt zichzelf
    // nooit hoger dan dat. Boven/onder: de ruimte tussen balk en andere rand;
    // links/rechts: de hele schermhoogte.
    let edge = state.config.lock().unwrap().edge();
    let max_height = match (dock.primary_monitor(), dock.outer_position(), dock.outer_size()) {
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

/// `width`/`height` = gemeten grootte van de paneelinhoud in CSS-pixels.
#[tauri::command(async)]
fn popup_ready(app: AppHandle, state: State<AppState>, width: f64, height: f64) {
    let (Some(dock), Some(popup)) = (app.get_webview_window("dock"), app.get_webview_window("popup")) else { return };
    let Ok(Some(monitor)) = dock.primary_monitor() else { return };
    let (Ok(dock_pos), Ok(dock_size), Ok(hwnd)) = (dock.outer_position(), dock.outer_size(), popup.hwnd()) else { return };

    let scale = monitor.scale_factor();
    let (w, h) = ((width * scale).ceil() as i32, (height * scale).ceil() as i32);
    let (mon_l, mon_t) = (monitor.position().x, monitor.position().y);
    let (mon_r, mon_b) = (mon_l + monitor.size().width as i32, mon_t + monitor.size().height as i32);
    let edge = state.config.lock().unwrap().edge();

    let (anchor_x, anchor_y) = {
        let mut p = state.popup.lock().unwrap();
        p.visible = true;
        p.blurred_at = None;
        (p.anchor_x, p.anchor_y)
    };
    // Naast de balk, gecentreerd op het aangeklikte element, binnen het scherm.
    let centered_x = (anchor_x - w / 2).min(mon_r - w).max(mon_l);
    let centered_y = (anchor_y - h / 2).min(mon_b - h).max(mon_t);
    let (x, y) = match edge {
        Edge::Top => (centered_x, dock_pos.y + dock_size.height as i32),
        Edge::Bottom => (centered_x, (dock_pos.y - h).max(mon_t)),
        Edge::Left => (dock_pos.x + dock_size.width as i32, centered_y),
        Edge::Right => ((dock_pos.x - w).max(mon_l), centered_y),
    };

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
    let mut geometry = None;
    let mut was_fullscreen = false;
    let mut tick: u32 = 0;
    loop {
        let cfg = state.config.lock().unwrap().clone();
        let snapshot = tracker.tick(&cfg);

        if snapshot.fullscreen != was_fullscreen {
            was_fullscreen = snapshot.fullscreen;
            // Schermvullende game of video: de balk gaat uit de weg.
            if snapshot.fullscreen {
                hide_popup(&app, false);
            }
            if let Some(dock) = window_hwnd(&app, "dock") {
                shell::set_shown(dock, !snapshot.fullscreen);
            }
            sync_backdrop(&app, "dock");
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

        // Elke 2 s: is het scherm of het werkgebied veranderd (resolutie,
        // schaal, herstart van Explorer)? Dan de balk opnieuw plaatsen.
        // Ook als Windows onze gereserveerde ruimte kwijt is.
        if tick % 8 == 0 {
            let now = shell::primary_geometry();
            let lost = window_hwnd(&app, "dock").is_some_and(appbar::lost);
            if now != geometry || lost {
                geometry = now;
                request_layout(&app);
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

    taskbar::apply(&cfg.taskbar);
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
    for label in ["dock", "popup"] {
        if let Some(h) = window_hwnd(&handle, label) {
            shell::make_tool_window(h);
        }
    }
    // De balk pakt bij een klik de focus niet, net als de Windows-taakbalk.
    // Zo blijft een open Startmenu of paneel open tot de klik zelf is
    // verwerkt, en sluit een tweede klik op dezelfde knop het weer.
    if let Some(h) = window_hwnd(&handle, "dock") {
        shell::make_no_activate(h);
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
        update: Mutex::default(),
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
            if let Some(h) = window_hwnd(app, "dock") {
                appbar::remove(h);
            }
            taskbar::restore();
        }
    });
}
