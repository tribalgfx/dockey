//! Licht of donker en de accentkleur van Windows, voor het thema "Automatisch".

use serde::Serialize;

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct Look {
    /// De taakbalk van Windows staat op licht ("Kies je standaard Windows-modus").
    pub light: bool,
    /// De accentkleur als "#rrggbb"; leeg als die niet te lezen is.
    pub accent: String,
}

pub fn read() -> Look {
    let user = windows_registry::CURRENT_USER;
    let light = user
        .open(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize")
        .and_then(|k| k.get_u32("SystemUsesLightTheme"))
        .map(|v| v != 0)
        .unwrap_or(false);
    // AccentColor staat er als 0xAABBGGRR.
    let accent = user
        .open(r"Software\Microsoft\Windows\DWM")
        .and_then(|k| k.get_u32("AccentColor"))
        .map(|v| format!("#{:02x}{:02x}{:02x}", v & 0xff, (v >> 8) & 0xff, (v >> 16) & 0xff))
        .unwrap_or_default();
    Look { light, accent }
}
