//! Instellingen: opgeslagen als JSON in %APPDATA%\Dockey\config.json.

use serde::{Deserialize, Serialize};
use std::{fs, path::PathBuf};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct Widgets {
    pub media: bool,
    pub weather: bool,
    pub system: bool,
    pub network: bool,
    pub volume: bool,
    pub battery: bool,
    pub agenda: bool,
    pub tray: bool,
    pub clock: bool,
}

impl Default for Widgets {
    fn default() -> Self {
        Self { media: true, weather: false, system: true, network: false, volume: true, battery: true, agenda: true, tray: true, clock: true }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct Weather {
    pub place: String,
    pub lat: f64,
    pub lon: f64,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct Pinned {
    /// Groepssleutel: exe-pad in kleine letters, of `aumid:<AppUserModelID>`.
    pub key: String,
    pub name: String,
    /// Wat ShellExecute moet openen (snelkoppeling, exe of shell:AppsFolder\...).
    pub launch: String,
}

/// Een widget die de gebruiker zelf toevoegt.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct CustomWidget {
    pub id: String,
    /// "status" (het statusformaat, zie de README) | "json" (tekst of waarde van een adres)
    pub kind: String,
    pub name: String,
    /// Het adres om op te halen.
    pub url: String,
    /// json: tekst met {pad.naar.waarde}; leeg = de hele (korte) tekst.
    pub template: String,
    /// json: emoji of een paar tekens vóór de waarde.
    pub icon: String,
    /// Wat een klik opent; leeg = bij status de link uit het antwoord, anders niets.
    pub open_url: String,
    /// Seconden tussen twee keer ophalen.
    pub interval: u32,
    /// Zelfondertekend certificaat accepteren (thuisnetwerk).
    pub insecure: bool,
}

/// Een gekoppelde agenda: een (geheime) iCal-link van Google, Outlook, Proton, iCloud, …
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct Calendar {
    pub id: String,
    pub name: String,
    pub url: String,
    pub color: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct Config {
    /// "bottom" | "top" | "left" | "right"
    pub position: String,
    /// "dock" (zwevende pil) | "panel" (zwevend over de volle breedte, zoals KDE
    /// Plasma) | "bar" (volle breedte, tegen de rand)
    pub layout: String,
    /// Kleuren: "auto" (die van de stijl; licht/donker en accentkleur volgen
    /// Windows) | "glas" | "rookglas" | "zwart" | "wit" | "middernacht" | "contrast"
    pub theme: String,
    /// Vorm en gedrag van de balk: "dockey" | "fluent" (Windows 11) |
    /// "cupertino" (macOS) | "adwaita" (GNOME) | "breeze" (KDE Plasma)
    pub style: String,
    /// "s" | "m" | "l"
    pub size: String,
    /// Taal van de interface: "auto" (de weergavetaal van Windows) | "nl" | "en"
    pub language: String,
    /// Wat er met de Windows-taakbalk gebeurt: "hidden" | "autohide" | "none"
    pub taskbar: String,
    /// Schermruimte reserveren zodat gemaximaliseerde vensters de balk niet bedekken.
    pub reserve_space: bool,
    /// Bij een gemaximaliseerd venster: balk over de volle breedte en ondoorzichtig,
    /// zodat er geen bureaubladachtergrond rond de balk te zien is.
    pub expand_on_maximize: bool,
    pub autostart: bool,
    pub clock24: bool,
    pub show_seconds: bool,
    /// Transparantie van de balk in procenten (0 = zoals het thema, tot 80).
    pub transparency: u32,
    /// Eens per dag bij GitHub kijken of er een nieuwe versie is.
    pub update_check: bool,
    /// Live voorbeelden van de vensters tonen als de muis boven een app-icoon blijft.
    pub previews: bool,
    /// Ook een balk op de andere schermen (met apps, Start en de klok).
    pub all_monitors: bool,
    /// Met meerdere balken: elke balk toont alleen de vensters op zijn eigen
    /// scherm (vastgemaakte apps staan op elke balk).
    pub apps_per_monitor: bool,
    pub widgets: Widgets,
    pub custom_widgets: Vec<CustomWidget>,
    /// Volgorde van de widgets (sleutels als "clock" of "custom:<id>"); leeg =
    /// standaardvolgorde. Ook uitgezette widgets houden zo hun plek.
    pub widget_order: Vec<String>,
    pub calendars: Vec<Calendar>,
    /// Systeemvakpictogrammen die in de balk staan; de rest zit achter ^.
    pub tray_pinned: Vec<String>,
    pub weather: Weather,
    pub pinned: Vec<Pinned>,
    pub pins_imported: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            position: "bottom".into(),
            layout: "dock".into(),
            theme: "rookglas".into(),
            style: "dockey".into(),
            size: "m".into(),
            language: "auto".into(),
            taskbar: "hidden".into(),
            reserve_space: true,
            expand_on_maximize: true,
            autostart: false,
            clock24: true,
            show_seconds: false,
            transparency: 0,
            update_check: true,
            previews: true,
            all_monitors: false,
            apps_per_monitor: true,
            widgets: Widgets::default(),
            custom_widgets: Vec::new(),
            widget_order: Vec::new(),
            calendars: Vec::new(),
            tray_pinned: Vec::new(),
            weather: Weather::default(),
            pinned: Vec::new(),
            pins_imported: false,
        }
    }
}

/// Schermrand waar de balk staat.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Edge {
    Bottom,
    Top,
    Left,
    Right,
}

impl Edge {
    pub fn vertical(self) -> bool {
        matches!(self, Edge::Left | Edge::Right)
    }
}

/// De weergavetaal van Windows: "nl" bij Nederlands, anders "en".
pub fn system_language() -> &'static str {
    use windows::Win32::Globalization::GetUserDefaultUILanguage;
    const LANG_DUTCH: u16 = 0x13;
    if unsafe { GetUserDefaultUILanguage() } & 0x3ff == LANG_DUTCH {
        "nl"
    } else {
        "en"
    }
}

impl Config {
    /// De taal van de interface, met "auto" ingevuld.
    pub fn ui_language(&self) -> &'static str {
        match self.language.as_str() {
            "nl" => "nl",
            "en" => "en",
            _ => system_language(),
        }
    }

    /// Dikte van de vensterstrook in logische pixels (pil + marge): de hoogte
    /// bij een balk boven/onder, de breedte bij een balk links/rechts.
    pub fn strip_thickness(&self) -> f64 {
        let pill = match self.size.as_str() {
            "s" => 44.0,
            "l" => 64.0,
            _ => 54.0,
        };
        let margin = if self.layout == "bar" { 0.0 } else { 6.0 };
        pill + margin * 2.0
    }

    pub fn edge(&self) -> Edge {
        match self.position.as_str() {
            "top" => Edge::Top,
            "left" => Edge::Left,
            "right" => Edge::Right,
            _ => Edge::Bottom,
        }
    }

    pub fn load() -> Self {
        let path = data_dir().join("config.json");
        let Ok(text) = fs::read_to_string(&path) else { return Self::default() };
        match serde_json::from_str(&text) {
            Ok(cfg) => cfg,
            Err(e) => {
                // Onleesbaar (bijv. half geschreven): bewaren, zodat de volgende
                // keer opslaan de oude instellingen niet zomaar wegvaagt.
                let _ = fs::copy(&path, data_dir().join("config.onleesbaar.json"));
                crate::util::log(&format!("config.json onleesbaar ({e}); kopie in config.onleesbaar.json, standaardinstellingen gebruikt"));
                Self::default()
            }
        }
    }

    /// Schrijft eerst naar een tijdelijk bestand en zet dat dan op zijn plek:
    /// een stroomstoring of crash halverwege laat zo nooit een half bestand achter.
    pub fn save(&self) {
        let dir = data_dir();
        let _ = fs::create_dir_all(&dir);
        let Ok(json) = serde_json::to_string_pretty(self) else { return };
        let tmp = dir.join("config.json.tmp");
        if fs::write(&tmp, json).is_ok() {
            if let Err(e) = fs::rename(&tmp, dir.join("config.json")) {
                crate::util::log(&format!("instellingen opslaan mislukt: {e}"));
            }
        }
    }
}

fn appdata() -> PathBuf {
    std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir)
}

pub fn data_dir() -> PathBuf {
    appdata().join("Dockey")
}

/// Dockey heette eerder Lichtbalk. Neemt bij de eerste start de instellingen en
/// de bewaarde stand van de Windows-taakbalk over, en haalt de oude
/// autostart-vermelding weg (de nieuwe zet Dockey zelf, als autostart aanstaat).
pub fn migrate_from_lichtbalk() {
    let (old, new) = (appdata().join("Lichtbalk"), data_dir());
    if new.exists() || !old.exists() {
        return;
    }
    let _ = fs::create_dir_all(&new);
    for file in ["config.json", "taakbalk-origineel.txt"] {
        let _ = fs::copy(old.join(file), new.join(file));
    }
    if let Ok(run) = windows_registry::CURRENT_USER
        .options()
        .read()
        .write()
        .open("Software\\Microsoft\\Windows\\CurrentVersion\\Run")
    {
        let _ = run.remove_value("Lichtbalk");
    }
    crate::util::log("instellingen overgenomen van Lichtbalk");
}
