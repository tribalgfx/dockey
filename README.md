<img src="src-tauri/icons/icon.svg" width="96" alt="Dockey logo: a D with donkey ears">

# Dockey

**English** · [Nederlands](README.nl.md)

A taskbar/dock of your own for Windows 11 (and 10): one bar with your apps, the
system tray and live widgets, while the regular Windows taskbar moves out of
sight.

> **Status: early version.** Runs on Windows 11; not every part has been
> widely tested on different PCs yet. See [Known limitations](#known-limitations).
> The latest version and what changed in each version are listed under
> [releases](https://github.com/tribalgfx/dockey/releases).
>
> Dockey used to be called *Lichtbalk*. On first start, Dockey takes over
> Lichtbalk's settings automatically.

## What it does

- **Apps**: pinned apps and all open windows, grouped per app, with the real
  app icons (if the shell can't provide an icon, Dockey tries the system image
  list, the exe file and finally the icon of an open window). On first start,
  the apps already pinned to the Windows taskbar are taken over. What apps
  show on their taskbar button shows in Dockey too: a button that wants
  attention pulses orange, a progress bar (copying, downloads) appears under
  the icon, and a badge (such as unread messages) sits on the icon. Drag apps
  to change their order.
- **Multiple displays**: optionally a bar on every display (Settings →
  Position and shape → Bar on all displays), with Start, the apps and the clock. Each
  bar then shows the windows on its own display; pinned apps are on every bar.
- **Widgets**: now playing with album art and controls, weather (click for the
  coming hours and days), CPU graph and memory, network speed, volume,
  battery, calendar and a clock with a month view (including week numbers),
  plus [custom widgets](#custom-widgets) and status widgets for your own
  services.
- **System tray**: the icons of apps (OneDrive, Discord, Steam, …) grouped in
  the bar. Pinned icons sit in the bar, the rest is behind ^; clicks and right
  clicks go straight through to the app.
- **Styles**: the shape and behaviour of the bar like Windows 11 (Fluent),
  macOS (Cupertino), GNOME (Adwaita) or KDE Plasma (Breeze), or Dockey's own
  style: corners, the line or dot under apps, centred apps (Fluent) or icons
  that grow on hover (Cupertino).
- **Appearance**: colours Automatic (the style's own colours, following light
  or dark mode and the accent colour of Windows), Smoke, Glass, Black, White,
  Night and Contrast; three sizes, and three shapes: a floating dock, a floating full-width panel (like KDE
  Plasma) or a bar against the screen edge; at the bottom, top, left or right.
  Left or right gives a look like Ubuntu's dock: apps at the top, compact
  widgets below them and the button with the dot grid at the bottom. A slider
  sets how transparent the bar is (Settings → Appearance).
- **Language**: English or Dutch, following the Windows display language. To
  pick another language, go to Settings → Appearance → Language; it applies
  right away.
- **Updates**: Dockey tells you when a new version is out and updates with one
  click, checking the download first (see
  [Updating and uninstalling](#updating-and-uninstalling)).
- **Windows taskbar**: hide it, auto-hide it or leave it. When Dockey exits, it
  comes back the way it was; nothing in Windows is patched.
- **Screen space**: the bar registers as an "AppBar", so maximised windows
  stop at the bar. When a window touches the bar (maximised, or snapped across
  the full width at the bottom), the dock stretches to the full width and
  turns opaque, so no desktop background shows around the bar. This can be
  turned off in the settings. During a full-screen game or video, the bar gets
  out of the way.

## Controls

| Action | Result |
| --- | --- |
| Click an app | Start it, bring it to the front, or (if it's already active) minimise it |
| Click an app with several windows | List of windows |
| Rest the pointer on an app | Live previews of its windows; click one to bring it to the front, × closes it (can be turned off in Settings) |
| Middle click or Shift+click on an app | New window |
| Right-click an app | Menu: windows, pin/unpin, move, close |
| Button with the dot grid | Windows Start menu |
| Right-click an empty spot or the dot grid | Settings, Task Manager, Task view, exit |
| Click the clock | Calendar, with a button to the notification centre |
| Scroll on volume | Louder/quieter; middle click mutes; a click opens Quick Settings |
| Click CPU/RAM | Task Manager |
| Click the weather | Now, the coming hours and seven days |
| Click ^ (system tray) | The other icons; "Choose what's in the bar" pins them |
| Click a custom widget | The address that belongs to it (for a status widget: the link from the response) |
| Grab a widget and drag it | Change the order of the widgets (Settings → Default order resets it) |
| Grab an app and drag it | Change the order of the apps: pinned apps among each other (kept), open apps among each other (until Dockey restarts) |
| Dot on the dot grid | A new version is available: right-click the bar → Update to version … |

Menus close with the same button that opened them (Start, Quick Settings, the
calendar, app menus) or with a click on an empty spot of the bar. That's
because the bar doesn't take focus when clicked, just like the Windows
taskbar.

## Custom widgets

Under Settings → Custom widgets you add widgets of your own:

- **Status widget**: an address that returns the status format below, such as
  a dashboard or a script on your server. If the response includes an icon,
  the bar shows only that icon with the status as a ring: no ring means all is
  well, yellow/orange needs attention, red is an outage, dashed is
  unreachable. Without an icon, the text sits in the bar in the colour of the
  status. The details are in the tooltip; a click opens the link from the
  response.
- **Custom widget**: an address that returns text or JSON, plus a template
  such as `{temperature}°` or `{data.status}`. Leave the template empty to
  show the text itself. You choose how often it refreshes, an icon (emoji) and
  optionally what a click opens.

Both can accept a self-signed certificate, for services on your own network.
Dockey fetches the data itself (not the web view), so the address doesn't need
to deal with CORS.

### The status format

```json
{
  "status": "warning",
  "text": "1 item needs attention",
  "short": "1",
  "detail": "• Disk almost full",
  "icon": "/logo.png",
  "link": "https://my-server:8080/"
}
```

| Field | Required | Meaning |
| --- | --- | --- |
| `status` | yes | `ok`, `warning` or `critical` |
| `text` | yes | Short sentence for the bar and the tooltip (up to 80 characters) |
| `short` | no | Very short, for a vertical dock (up to 6 characters); ✓ or ! by default |
| `detail` | no | Extra lines in the tooltip |
| `icon` | no | Square image (PNG, JPG or SVG); may be relative to the address |
| `link` | no | What a click opens; may be relative too. "Open on click" in the settings takes precedence |

Dockey fetches the address at the interval from the settings (every minute by
default). A response in any other format, or no response, shows the widget as
unreachable.

## Linking calendars

Under Settings → Calendars you link one or more calendars by their iCal link.
That works with almost any service; the settings show where each service keeps
the link:

- **Google**: Settings → your calendar → Integrate calendar → "Secret address
  in iCal format"
- **Outlook.com**: Settings → Calendar → Shared calendars → Publish a calendar
  → ICS link
- **Proton**: share a calendar via a link (requires a paid plan)
- **iCloud**: Public Calendar → link (`webcal://` works too)

The **Calendar** widget is just an icon, with a dot in the calendar's colour
while something is still coming up today. Hover over it to see today's
appointments; a click opens the calendar with dots on days that have
appointments, the appointments of today (or of the day you click) and what's
coming up soon. Dockey refreshes the calendars every fifteen minutes.
Recurring appointments, exceptions and time zones are taken into account.

An iCal link is secret: anyone who has it can read your calendar. Dockey
stores it only in `%APPDATA%\Dockey\config.json` and the settings show only the
name of the service.

## Installing

1. Download `Dockey-Setup-<version>.exe` from the
   [latest release](https://github.com/tribalgfx/dockey/releases/latest).
   Each release lists the SHA-256 checksum of the installer.
2. Run it. Windows SmartScreen may warn because the file isn't signed
   ("More info → Run anyway").
   - No administrator rights needed: Dockey goes into
     `%LOCALAPPDATA%\Programs\Dockey`, with a shortcut in the Start menu.
   - The installer is in English or Dutch, following the Windows language.
3. Let Dockey start at the end.

Windows 11 already has everything it needs. On Windows 10 the
[WebView2 runtime](https://developer.microsoft.com/microsoft-edge/webview2/)
must be installed; the installer warns if it's missing.

Want the very latest state, or rather no installer? Under **Actions →
Windows-build**, every build on `main` has the installer (`dockey-setup`) and
a standalone `dockey.exe` (`dockey-windows-x64`) that you can put anywhere and
run. These files can only be downloaded with a GitHub account and are kept for
90 days.

### Updating and uninstalling

- **Updating**: Dockey checks for a new version by itself: a minute after
  starting and then once a day, at the
  [releases](https://github.com/tribalgfx/dockey/releases). If there is one,
  the Start button gets a dot, and **Update to version …** appears at the top
  of the bar menu (right-click the bar) and the tray icon's menu. There you see
  what's new and choose **Update now**: Dockey downloads the installer, checks
  it against the SHA-256 checksum GitHub lists for the release (if it doesn't
  match, the file is thrown away), and runs it. The installer closes Dockey
  cleanly, replaces the program and starts Dockey again; your settings stay.
  Nothing is downloaded or installed without your click. Automatic checking
  can be turned off in Settings → Updates; you can also check right away
  there.

  Dockey can do this from version 0.3.0. If you have 0.2.0, update once
  yourself with the installer from the latest release (see below); after that
  it happens by itself.

  You can also update manually: run the new `Dockey-Setup-<version>.exe` from
  the releases. It closes a running Dockey cleanly (the Windows taskbar comes
  back briefly), replaces the program and keeps your settings.
  - An older Dockey (from before the installer) doesn't know that way of
    closing yet. The installer then asks whether it may stop Dockey, stops it
    and then restores the Windows taskbar.
  - If you ran Dockey as a standalone `dockey.exe`: as soon as the installed
    Dockey starts, "Start with Windows" points to the new location. You can
    delete the standalone file afterwards; your settings stay as they are.
- **Uninstalling**: via Settings → Apps → Installed apps → Dockey. Dockey
  closes cleanly first, "Start with Windows" is removed, and you choose
  whether your settings (in `%APPDATA%\Dockey`, including the calendar links)
  should go too. By default they stay. If Dockey can't be stopped, the
  uninstall stops with a message, so no half-removed installation is left
  behind.

**Without questions** (for example in a script), install with
`Dockey-Setup-<version>.exe /SILENT /SUPPRESSMSGBOXES` (or `/VERYSILENT` for no
window at all). A running Dockey is then stopped without asking; Dockey
doesn't start by itself afterwards. A silent uninstall is
`unins000.exe /SILENT /SUPPRESSMSGBOXES` in the installation folder; your
settings stay.

`dockey.exe --quit` closes a running Dockey cleanly; the installer uses it
too. If no Dockey is running, it does nothing.

### Building the installer

Needed: Rust and the Visual Studio C++ Build Tools (see below) and
[Inno Setup 6](https://jrsoftware.org/isinfo.php). If Inno Setup is missing,
the script installs it with winget.

```powershell
.\scripts\build-installer.ps1
```

The script builds `dockey.exe` and has Inno Setup (`installer\Dockey.iss`)
turn it into `artifacts\Dockey-Setup-<version>.exe` (about 3 MB). The version
comes from `src-tauri\tauri.conf.json`; `artifacts\` is in `.gitignore`. The
Windows build on GitHub makes the same installer on every push (the
`dockey-setup` artifact).

### Building it yourself (on Windows)

With [Rust](https://rustup.rs) and the Visual Studio C++ Build Tools:

```bash
cd src-tauri
```

```bash
cargo build --release
```

The result is in `target\release\dockey.exe`. Node or npm isn't needed: the
interface is plain HTML/CSS/JS without a build step.

## If the Windows taskbar doesn't come back

Normally Dockey restores the taskbar when it exits (also after a crash, on the
next start). Should it still stay away or remain on "auto-hide":

```bash
dockey.exe --restore
```

If that doesn't work either: restart Explorer via Task Manager
(Ctrl+Shift+Esc → "Windows Explorer" → Restart) and turn off "Automatically
hide the taskbar" in Settings → Personalization → Taskbar.

Settings are stored in `%APPDATA%\Dockey\config.json`. Deleting that file
resets everything to the defaults. If the file has become unreadable (for
example after a crash while saving), Dockey starts with the default settings
and keeps a copy as `config.onleesbaar.json`. Problems (such as an icon that
can't be loaded) go into `%APPDATA%\Dockey\dockey.log`.

## Known limitations

- **System tray**: Dockey registers as a second system tray and passes
  everything on to the Windows taskbar. An app that doesn't respond to the
  "register again" when Dockey starts only appears after that app restarts.
  Windows' own volume, network and battery aren't tray icons; Dockey has its
  own widgets for those. You can turn the tray off under widgets (Tray).
- **Progress bars and badges on app buttons** come in through Dockey's
  system tray: with the tray turned off (under widgets), Dockey doesn't see
  them. Pulsing when an app wants attention always works. An app that set its
  badge before Dockey started is asked to send it again; an app that doesn't,
  shows it the next time the badge changes.
- **Other displays** get a bar with Start, the apps and the clock; the other
  widgets and the system tray are on the bar of the primary display.
- **Vertical dock on a short screen**: if not everything fits, the app list
  scrolls; in that case, turn off some widgets.
- **Glass only blurs on Windows 11.** There Windows blurs the desktop behind
  the bar and the panels (acrylic), with Windows' own rounded corners (8 px,
  so less round than the other styles). On Windows 10, Glass is translucent
  without blur.
- **Windows of programs running as administrator** can't be minimised or
  closed by Dockey (Windows blocks that), unless Dockey itself also runs as
  administrator.
- **Pinned Store apps** from the Windows taskbar aren't taken over
  automatically; pin them with a right click once they're open.

## Viewing the interface without Windows

The `ui/` folder also works on its own in a browser, with a simulated backend
(`ui/mock.js`):

```bash
python3 -m http.server 8765 --directory ui
```

Then open `http://127.0.0.1:8765/index.html`. URL parameters pick a variant,
for example `?theme=glas&style=fluent&size=l&layout=bar&pos=left&popup=settings`;
`?max=1` simulates a maximised window, `?badges` shows attention, progress and
badges on app buttons, and `?extra` shows the bar of another display.

## Structure

```
ui/                  interface (HTML/CSS/JS, no build step)
  index.html, dock.js    the bar itself
  popup.html, popup.js   the panel: menus, settings, calendar
  mock.js                simulated backend for the browser
src-tauri/           Windows side (Rust, Tauri 2)
  src/main.rs            windows, commands, background threads
  src/shell.rs           tracking open windows, activate/minimise/close
  src/taskbar.rs         hiding and restoring the Windows taskbar
  src/appbar.rs          reserving screen space
  src/backdrop.rs        real blur (acrylic) behind Glass, Fluent and Cupertino
  src/look.rs            light/dark mode and accent colour of Windows
  src/icons.rs           fetching app icons
  src/pins.rs            taking over pinned apps from Windows
  src/stats.rs           CPU, memory, battery, network, volume
  src/media.rs           now playing and playback controls
  src/keys.rs            Windows shortcuts (Start, Task view, …)
  src/config.rs          settings
  src/update.rs          finding, downloading, checking and installing a new version
  src/taskband.rs        attention, progress and badges on app buttons
  src/tray.rs            the system tray in the bar
```

The code comments are in Dutch.

The logo is a D with donkey ears (a nod to *Donkie*); the hole in the D is the
dock and the line underneath is the indicator of an active app. The source
files are in `src-tauri/icons/`: `icon.svg` for 32 px and up,
`icon-klein.svg` (bolder, without details) for 16–24 px.

The weather comes from [Open-Meteo](https://open-meteo.com) (no account or key
needed). For updates, Dockey asks GitHub for the latest release once a day;
nothing about you or your PC is sent along (can be turned off in Settings →
Updates). Otherwise Dockey only connects to the addresses you fill in yourself
(calendars and custom widgets).

## License

Dockey is licensed under the [GNU General Public License, version 3 or later](LICENSE)
(GPL-3.0-or-later). You may use, modify and distribute it; a modified version
you distribute is then also under the GPL, with the source code included.
