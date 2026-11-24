// Nagebootste backend voor in de browser (ontwikkelen en screenshots).
// In de echte app wordt dit bestand niet geladen.
//
// URL-parameters: ?lang=en&theme=wit&size=l&layout=bar&pos=top&max=1&popup=settings|calendar|menu|dock-menu
//                 &geenvoorbeelden (venstervoorbeelden uit)

function create() {
  const params = new URLSearchParams(location.search);
  const listeners = {};
  const bus = {
    listen(event, handler) {
      (listeners[event] ??= []).push(handler);
      return Promise.resolve(() => {});
    },
    emit(event, payload) {
      // Net als in het echt krijgt elk venster zijn eigen kopie.
      for (const h of listeners[event] ?? []) h({ payload: payload === undefined ? undefined : structuredClone(payload) });
      return Promise.resolve();
    },
  };

  const tint = (letter, a, b) =>
    `data:image/svg+xml,${encodeURIComponent(
      `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 64 64"><defs><linearGradient id="g" x1="0" y1="0" x2="1" y2="1"><stop offset="0" stop-color="${a}"/><stop offset="1" stop-color="${b}"/></linearGradient></defs><rect x="4" y="4" width="56" height="56" rx="15" fill="url(#g)"/><text x="32" y="43" font-family="Segoe UI,Arial,sans-serif" font-size="30" font-weight="700" text-anchor="middle" fill="#fff">${letter}</text></svg>`,
    )}`;
  const apps = {
    'c:\\windows\\explorer.exe': ['Verkenner', tint('V', '#ffc93c', '#f08a1c')],
    'c:\\apps\\browser.exe': ['Browser', tint('B', '#35c3f3', '#1b6fe0')],
    'c:\\apps\\code.exe': ['Code-editor', tint('C', '#3aa0ff', '#2157c9')],
    'c:\\apps\\terminal.exe': ['Terminal', tint('>', '#3a3f4b', '#14161c')],
    'c:\\apps\\muziek.exe': ['Muziek', tint('M', '#3bdc7c', '#12a150')],
    'c:\\apps\\mail.exe': ['Mail', tint('@', '#ff7a7a', '#d6336c')],
    'c:\\apps\\notities.exe': ['Notities', tint('N', '#b58bff', '#6d3be0')],
    'c:\\apps\\firefox.exe': ['Firefox', tint('F', '#ffb340', '#e8492e')],
  };
  // Net als soms in het echt: dit icoon lukt alleen via een open venster.
  const windowOnlyIcons = new Set(['c:\\apps\\firefox.exe']);

  // ?update=1 bootst een nieuwere release na.
  const mockUpdate = params.has('update')
    ? {
        version: '0.3.0',
        notes: '## Wat is er nieuw\n\n- **Updates**: Dockey kijkt zelf of er een nieuwe versie is.\n- Instelbare transparantie van de balk.',
        page: 'https://github.com/voorbeeld/dockey/releases/tag/v0.3.0',
      }
    : null;

  const config = {
    position: params.get('pos') ?? 'bottom',
    layout: params.get('layout') ?? 'dock',
    theme: params.get('theme') ?? 'rookglas',
    size: params.get('size') ?? 'm',
    language: params.get('lang') ?? 'auto',
    transparency: Number(params.get('transparantie') ?? 0),
    updateCheck: true,
    previews: !params.has('geenvoorbeelden'),
    allMonitors: params.has('extra'),
    appsPerMonitor: true,
    customWidgets: params.has('custom')
      ? [
          { id: 'st', kind: 'status', name: 'Server', url: 'https://server.voorbeeld/status.json', template: '', icon: '', openUrl: '', interval: 60, insecure: true },
          { id: 'tmp', kind: 'json', name: 'Zolder', url: 'https://voorbeeld/sensor', template: '{temperatuur}°', icon: '🌡️', openUrl: '', interval: 60, insecure: false },
        ]
      : [],
    widgetOrder: [],
    trayPinned: params.has('tray') ? ['onedrive', 'discord'] : [],
    calendars: params.has('agenda')
      ? [
          { id: 'g', name: 'Google privé', url: 'https://calendar.voorbeeld/private-geheim/basic.ics', color: '#5b9cff' },
          { id: 'w', name: 'Werk', url: 'webcal://outlook.voorbeeld/werk/calendar.ics', color: '#3ecf8e' },
        ]
      : [],
    taskbar: 'hidden', reserveSpace: true, expandOnMaximize: true, autostart: false, clock24: true, showSeconds: false,
    widgets: { media: true, weather: true, system: true, network: params.has('net'), volume: true, battery: true, agenda: true, tray: true, clock: true },
    weather: { place: 'Utrecht', lat: 52.09, lon: 5.12 },
    pinned: ['explorer.exe', 'browser.exe', 'firefox.exe', 'code.exe', 'terminal.exe', 'muziek.exe'].map((exe) => {
      const key = Object.keys(apps).find((k) => k.endsWith(exe));
      return { key, name: apps[key][0], launch: key };
    }),
    pinsImported: true,
  };

  let maximized = params.has('max');
  let nextHwnd = 100;
  let active = 0;
  const windows = []; // { hwnd, key, title, min }
  const openWindow = (key, title) => {
    const w = { hwnd: nextHwnd++, key, title: title ?? apps[key]?.[0] ?? key, min: false };
    windows.push(w);
    active = w.hwnd;
  };
  openWindow('c:\\windows\\explorer.exe', 'Documenten');
  openWindow('c:\\apps\\browser.exe', 'Weerbericht — Browser');
  openWindow('c:\\apps\\browser.exe', 'Recepten — Browser');
  openWindow('c:\\apps\\muziek.exe', 'Muziek');
  openWindow('c:\\apps\\mail.exe', 'Postvak IN — Mail');
  openWindow('c:\\apps\\code.exe', 'dock.js — dockey');

  // Wat apps aan hun knop doorgeven (?badges): aandacht, voortgang, badge.
  const MONITOR = '\\\\.\\DISPLAY1';
  const badgeIcon = `data:image/svg+xml,${encodeURIComponent('<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16"><circle cx="8" cy="8" r="7.5" fill="#e5484d"/><text x="8" y="11.6" font-family="Arial" font-size="10" font-weight="700" text-anchor="middle" fill="#fff">3</text></svg>')}`;
  const badges = params.has('badges')
    ? { 'Documenten': { progress: 'normal', value: 0.62 }, 'Postvak IN — Mail': { flash: true, overlay: badgeIcon }, 'dock.js — dockey': { progress: 'error', value: 0.3 }, 'Muziek': { progress: 'busy' } }
    : {};
  const snapshot = () => {
    const groups = config.pinned.map((p) => ({ ...p, pinned: true, windows: [] }));
    for (const w of windows) {
      let g = groups.find((x) => x.key === w.key);
      if (!g) groups.push((g = { key: w.key, name: apps[w.key]?.[0] ?? w.key, launch: w.key, pinned: false, windows: [] }));
      g.windows.push({ hwnd: w.hwnd, title: w.title, active: w.hwnd === active && !w.min, min: w.min, monitor: MONITOR, ...(badges[w.title] ?? {}) });
    }
    return { groups, fullscreen: null, maximized: maximized ? [MONITOR] : [] };
  };
  const pushShell = () => bus.emit('shell', snapshot());
  const pushConfig = () => bus.emit('config', structuredClone(config));

  const stats = { cpu: 18, ram: 61, ramUsedGb: 9.8, ramTotalGb: 16, battery: 82, charging: true, down: 2.4e6, up: 2.1e5, volume: 0.46, muted: false };
  const media = {
    title: 'Nachtrit', artist: 'De Voorbeeldband', playing: true,
    art: `data:image/svg+xml,${encodeURIComponent('<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 64 64"><defs><linearGradient id="g" x1="0" y1="0" x2="1" y2="1"><stop offset="0" stop-color="#ff8a5c"/><stop offset="1" stop-color="#6a3de8"/></linearGradient></defs><rect width="64" height="64" fill="url(#g)"/><circle cx="44" cy="22" r="9" fill="#fff" fill-opacity=".85"/><path d="M0 64 22 34l14 16 10-10 18 24z" fill="#1c1440" fill-opacity=".55"/></svg>')}`,
  };
  // Nagebootste systeemvakpictogrammen.
  const trayIcon = (fill, letter) =>
    `data:image/svg+xml,${encodeURIComponent(`<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16"><rect x="1" y="1" width="14" height="14" rx="4" fill="${fill}"/><text x="8" y="11.5" font-family="Arial" font-size="9" font-weight="700" text-anchor="middle" fill="#fff">${letter}</text></svg>`)}`;
  const trayIcons = [
    { key: 'onedrive', tip: 'OneDrive – bijgewerkt', icon: trayIcon('#2f7de1', 'O') },
    { key: 'discord', tip: 'Discord', icon: trayIcon('#5865f2', 'D') },
    { key: 'steam', tip: 'Steam', icon: trayIcon('#1b2838', 'S') },
    { key: 'nvidia', tip: 'NVIDIA', icon: trayIcon('#76b900', 'N') },
    { key: 'teams', tip: 'Microsoft Teams', icon: trayIcon('#6264a7', 'T') },
    { key: 'defender', tip: 'Windows-beveiliging', icon: trayIcon('#2b8a3e', 'W') },
  ];

  // Nagebootste agenda's met afspraken rond vandaag (kloktijden zonder zone = lokaal).
  const mockIcs = (work) => {
    const pad = (n) => String(n).padStart(2, '0');
    const at = (days, h, m) => { const d = new Date(); d.setDate(d.getDate() + days); d.setHours(h, m, 0, 0); return d; };
    const stamp = (d) => `${d.getFullYear()}${pad(d.getMonth() + 1)}${pad(d.getDate())}T${pad(d.getHours())}${pad(d.getMinutes())}00`;
    const date = (d) => `${d.getFullYear()}${pad(d.getMonth() + 1)}${pad(d.getDate())}`;
    const now = new Date();
    const ev = (body) => `BEGIN:VEVENT\r\n${body}\r\nEND:VEVENT`;
    const soon = new Date(now.getTime() + 40 * 60000);
    const items = work
      ? [
          ev(`UID:w1\r\nSUMMARY:Overleg team\r\nDTSTART:${stamp(new Date(now.getTime() - 20 * 60000))}\r\nDTEND:${stamp(new Date(now.getTime() + 25 * 60000))}`),
          ev(`UID:w2\r\nSUMMARY:Sprintreview\r\nLOCATION:Vergaderzaal 2\r\nDTSTART:${stamp(at(2, 14, 0))}\r\nDTEND:${stamp(at(2, 15, 0))}`),
        ]
      : [
          ev(`UID:g1\r\nSUMMARY:Tandarts\r\nLOCATION:Mondzorg Centrum\r\nDTSTART:${stamp(soon)}\r\nDTEND:${stamp(new Date(soon.getTime() + 30 * 60000))}`),
          ev(`UID:g2\r\nSUMMARY:Voetbal\r\nDTSTART:${stamp(at(1, 19, 30))}\r\nDTEND:${stamp(at(1, 21, 0))}\r\nRRULE:FREQ=WEEKLY`),
          ev(`UID:g3\r\nSUMMARY:Verjaardag Sanne\r\nDTSTART;VALUE=DATE:${date(at(3, 0, 0))}\r\nDTEND;VALUE=DATE:${date(at(4, 0, 0))}`),
          ev(`UID:g4\r\nSUMMARY:Garage APK\r\nDTSTART:${stamp(at(5, 10, 0))}\r\nDTEND:${stamp(at(5, 11, 0))}`),
        ];
    return `BEGIN:VCALENDAR\r\nVERSION:2.0\r\n${items.join('\r\n')}\r\nEND:VCALENDAR`;
  };

  // Geseede willekeur, zodat screenshots herhaalbaar zijn.
  let seed = 7;
  const rnd = () => ((seed = (seed * 16807) % 2147483647) / 2147483647);
  const stepStats = () => {
    stats.cpu = Math.max(3, Math.min(96, stats.cpu + (rnd() - 0.48) * 22));
    stats.down = Math.max(0, stats.down * (0.6 + rnd() * 0.9));
    stats.up = Math.max(0, stats.up * (0.6 + rnd() * 0.9));
  };
  setInterval(() => {
    stepStats();
    bus.emit('stats', { ...stats });
  }, 1500);

  // Open-Meteo is in de voorbeeldmodus niet nodig: vaste antwoorden.
  const realFetch = window.fetch.bind(window);
  const fakeFetch = (url, ...rest) => {
    const json = (body) => Promise.resolve(new Response(JSON.stringify(body), { headers: { 'content-type': 'application/json' } }));
    if (String(url).includes('api.open-meteo.com') && String(url).includes('daily=')) {
      const pad = (n) => String(n).padStart(2, '0');
      const iso = (d) => `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}T${pad(d.getHours())}:00`;
      const hours = Array.from({ length: 48 }, (_, i) => { const d = new Date(); d.setMinutes(0, 0, 0); d.setHours(d.getHours() + i - 2); return d; });
      const days = Array.from({ length: 7 }, (_, i) => { const d = new Date(); d.setDate(d.getDate() + i); return iso(d).slice(0, 10); });
      return json({
        current: { temperature_2m: 11.4, apparent_temperature: 9.2, relative_humidity_2m: 78, wind_speed_10m: 17.6, weather_code: 2, is_day: 1 },
        hourly: {
          time: hours.map(iso),
          temperature_2m: hours.map((_, i) => 11 - Math.abs(6 - i) * 0.6),
          weather_code: hours.map((_, i) => (i > 5 ? 61 : 2)),
          precipitation_probability: hours.map((_, i) => (i > 5 ? 60 : 5)),
          is_day: hours.map((d) => (d.getHours() >= 8 && d.getHours() < 17 ? 1 : 0)),
        },
        daily: {
          time: days,
          weather_code: [2, 3, 61, 80, 1, 0, 3],
          temperature_2m_max: [12, 10, 9, 11, 13, 14, 10],
          temperature_2m_min: [6, 5, 4, 6, 7, 8, 5],
          precipitation_probability_max: [10, 35, 80, 55, 5, 0, 20],
          sunrise: days.map((d) => `${d}T08:02`),
          sunset: days.map((d) => `${d}T16:49`),
        },
      });
    }
    if (String(url).includes('api.open-meteo.com')) return json({ current: { temperature_2m: 11.4, weather_code: 2, is_day: 1 } });
    if (String(url).includes('geocoding-api.open-meteo.com')) return json({ results: [{ name: 'Utrecht', admin1: 'Utrecht', country: 'Nederland', latitude: 52.09, longitude: 5.12 }] });
    return realFetch(url, ...rest);
  };
  window.fetch = fakeFetch;

  // Het paneel is in het echt een tweede venster; hier een iframe.
  const frame = document.createElement('iframe');
  frame.className = 'mock-popup';
  frame.classList.add('off');
  const popup = { id: '', anchor: 0, visible: false, blurredAt: 0 };
  const framed = new Promise((resolve) => frame.addEventListener('load', resolve, { once: true }));
  frame.src = 'popup.html';
  document.body.append(frame);
  framed.then(() => (frame.contentWindow.fetch = fakeFetch));
  const hidePopup = (byBlur) => {
    if (!popup.visible) return;
    popup.visible = false;
    popup.blurredAt = byBlur ? Date.now() : 0;
    frame.classList.add('off');
    bus.emit('popup:closed');
  };
  document.addEventListener('mousedown', () => hidePopup(true), true);

  // De venstervoorbeelden: ook een eigen venster, hier een tweede iframe. Een
  // echt beeld (DWM-thumbnail) is er niet; preview.js tekent een nepvenster.
  const pvFrame = document.createElement('iframe');
  pvFrame.className = 'mock-popup off';
  const pv = { serial: 0, anchorX: 0, anchorY: 0, visible: false };
  const pvFramed = new Promise((resolve) => pvFrame.addEventListener('load', resolve, { once: true }));
  pvFrame.src = 'preview.html';
  document.body.append(pvFrame);
  const hidePreview = () => {
    pv.serial++;
    if (!pv.visible) return;
    pv.visible = false;
    pvFrame.classList.add('off');
    bus.emit('preview:closed');
  };
  // Verhouding van een nepvenster; geminimaliseerd = geen beeld.
  const ratioOf = (hwnd) => {
    const w = windows.find((x) => x.hwnd === hwnd);
    return !w || w.min ? null : w.key.endsWith('mail.exe') ? 4 / 3 : 16 / 10;
  };

  const commands = {
    get_initial: () => ({ monitor: params.has('extra') ? MONITOR.replace('1', '2') : MONITOR, mainDock: !params.has('extra'), config: structuredClone(config), shell: snapshot(), stats: { ...stats }, media: { ...media }, version: '0.2.0', update: mockUpdate }),
    check_update: async () => ({ update: mockUpdate, current: '0.2.0' }),
    install_update: async () => {
      await new Promise((r) => setTimeout(r, 800));
      if (params.has('updatefout')) throw 'ERR_UPDATE_HASH';
    },
    set_config: ({ config: next }) => {
      Object.assign(config, structuredClone(next));
      pushConfig();
    },
    window_action: ({ hwnd, action }) => {
      const w = windows.find((x) => x.hwnd === hwnd);
      if (!w) return;
      if (action === 'focus') {
        w.min = false;
        active = hwnd;
      } else if (action === 'minimize') w.min = true;
      else if (action === 'close') windows.splice(windows.indexOf(w), 1);
      pushShell();
    },
    launch: ({ target }) => {
      if (apps[target]) openWindow(target);
      pushShell();
    },
    pin: ({ key }) => {
      if (!config.pinned.some((p) => p.key === key)) config.pinned.push({ key, name: apps[key]?.[0] ?? key, launch: key });
      pushConfig();
      pushShell();
    },
    unpin: ({ key }) => {
      config.pinned = config.pinned.filter((p) => p.key !== key);
      pushConfig();
      pushShell();
    },
    move_pin: ({ key, delta }) => {
      const i = config.pinned.findIndex((p) => p.key === key);
      const j = i + delta;
      if (i < 0 || j < 0 || j >= config.pinned.length) return;
      [config.pinned[i], config.pinned[j]] = [config.pinned[j], config.pinned[i]];
      pushConfig();
      pushShell();
    },
    reorder_pins: ({ keys }) => {
      const rank = (k) => (keys.includes(k) ? keys.indexOf(k) : Infinity);
      config.pinned = [...config.pinned].sort((a, b) => rank(a.key) - rank(b.key));
      pushConfig();
      pushShell();
    },
    get_icon: ({ key, hwnd }) =>
      apps[key] && (hwnd != null || !windowOnlyIcons.has(key)) ? apps[key][1] : Promise.reject(new Error('geen icoon')),
    send_keys: ({ name }) => console.log('[mock] toetsen:', name),
    tray_icons: () => trayIcons,
    fetch_image: async ({ url }) => {
      if (url.endsWith('/logo.svg')) return 'data:image/svg+xml,' + encodeURIComponent('<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 40 40"><rect width="40" height="40" rx="9" fill="#3b82c4"/><rect x="10" y="10" width="20" height="8" rx="2" fill="#fff"/><rect x="10" y="22" width="20" height="8" rx="2" fill="#fff"/><circle cx="26" cy="14" r="1.5" fill="#3b82c4"/><circle cx="26" cy="26" r="1.5" fill="#3b82c4"/></svg>');
      throw 'Geen afbeelding';
    },
    tray_click: ({ key, right }) => {
      window.__dockeyMock.lastTrayClick = { key, right };
      console.log('[mock] systeemvak:', key, right ? 'rechts' : 'links');
    },
    tray_rect: () => {},
    fetch_url: async ({ url }) => {
      if (url.includes('kapot')) throw 'Verbinding geweigerd';
      if (url.includes('leeg')) return 'BEGIN:VCALENDAR\r\nVERSION:2.0\r\nEND:VCALENDAR';
      if (url.includes('.ics')) return mockIcs(url.includes('werk') || url.includes('outlook'));
      if (url.endsWith('/status.json')) {
        // ?status=warning|critical bootst een waarschuwing of storing na.
        const status = params.get('status') ?? 'ok';
        const text = { ok: 'Alles in orde', warning: '1 punt vraagt aandacht', critical: '2 storingen' }[status] ?? 'Alles in orde';
        return JSON.stringify({ status, text, detail: status === 'ok' ? '' : '• Schijf bijna vol', icon: '/logo.svg', link: 'https://server.voorbeeld/' });
      }
      return JSON.stringify({ temperatuur: 18.64, status: 'ok' });
    },
    media_cmd: ({ cmd }) => {
      if (cmd === 'toggle') media.playing = !media.playing;
      else Object.assign(media, cmd === 'next' ? { title: 'Ochtendgloren', artist: 'Het Voorbeeldkwartet' } : { title: 'Nachtrit', artist: 'De Voorbeeldband' });
      bus.emit('media', { ...media });
    },
    set_volume: ({ level }) => void (stats.volume = level),
    set_mute: ({ muted }) => void (stats.muted = muted),
    quit: () => console.log('[mock] afsluiten'),
    open_popup: async ({ id, kind, payload, anchorX, anchorY }) => {
      if (popup.id === id && (popup.visible || Date.now() - popup.blurredAt < 350)) {
        hidePopup(false);
        return false;
      }
      Object.assign(popup, { id, anchorX, anchorY });
      hidePreview();
      await framed;
      const strip = document.getElementById('strip').getBoundingClientRect();
      const pos = config.position;
      const maxHeight = pos === 'top' ? innerHeight - strip.bottom : pos === 'bottom' ? strip.top : innerHeight;
      bus.emit('popup:render', { id, kind, payload, maxHeight });
      return true;
    },
    popup_ready: ({ width, height }) => {
      const strip = document.getElementById('strip').getBoundingClientRect();
      const clamp = (v, max) => Math.max(0, Math.min(v, max));
      const cx = clamp(popup.anchorX - width / 2, innerWidth - width);
      const cy = clamp(popup.anchorY - height / 2, innerHeight - height);
      const [x, y] = {
        top: [cx, strip.bottom],
        bottom: [cx, strip.top - height],
        left: [strip.right, cy],
        right: [strip.left - width, cy],
      }[config.position];
      Object.assign(frame.style, { left: `${x}px`, top: `${y}px`, width: `${width}px`, height: `${height}px` });
      frame.classList.remove('off');
      popup.visible = true;
    },
    close_popup: () => hidePopup(false),
    open_preview: async ({ key, name, icon, windows: list, anchorX, anchorY }) => {
      if (!config.previews || popup.visible || !list.length) return false;
      const serial = ++pv.serial;
      Object.assign(pv, { anchorX, anchorY });
      await pvFramed;
      const ws = list.map((w) => ({ ...w, ratio: ratioOf(w.hwnd) }));
      bus.emit('preview:render', { serial, key, name, icon, windows: ws, maxWidth: innerWidth });
      return true;
    },
    preview_ready: ({ serial, width, height }) => {
      if (serial !== pv.serial) return;
      const strip = document.getElementById('strip').getBoundingClientRect();
      const clamp = (v, max) => Math.max(0, Math.min(v, max));
      const cx = clamp(pv.anchorX - width / 2, innerWidth - width);
      const cy = clamp(pv.anchorY - height / 2, innerHeight - height);
      const [x, y] = {
        top: [cx, strip.bottom],
        bottom: [cx, strip.top - height],
        left: [strip.right, cy],
        right: [strip.left - width, cy],
      }[config.position];
      Object.assign(pvFrame.style, { left: `${x}px`, top: `${y}px`, width: `${width}px`, height: `${height}px` });
      pvFrame.classList.remove('off');
      pv.visible = true;
    },
    close_preview: hidePreview,
  };

  document.documentElement.classList.add('mock');
  if (params.get('bg') === 'donker') document.documentElement.classList.add('dark-bg'); // donkere achtergrond om randen te beoordelen
  if (params.get('bg') === 'licht') document.documentElement.classList.add('light-bg');

  // Vul de CPU-grafiek meteen, en open eventueel direct een paneel (?popup=…).
  setTimeout(() => {
    for (let i = 0; i < 30; i++) {
      stepStats();
      bus.emit('stats', { ...stats });
    }
    const auto = params.get('popup');
    if (auto === 'settings') return bus.emit('open-settings');
    const node = document.querySelector({ 'dock-menu': '#start', calendar: '.clock', menu: '.tile.multi' }[auto]);
    node?.dispatchEvent(new MouseEvent(auto === 'calendar' ? 'click' : 'contextmenu', { bubbles: true, cancelable: true }));
  }, 300);

  return {
    // Alleen voor tests: een gemaximaliseerd venster nabootsen.
    debug: {
      setMaximized(on) {
        maximized = on;
        pushShell();
      },
    },
    core: { invoke: async (cmd, args = {}) => (commands[cmd] ?? (() => console.warn('[mock] onbekend commando', cmd)))(args) },
    event: bus,
  };
}

// Het paneel-iframe deelt de backend van de pagina eromheen.
const shared = window.parent !== window ? window.parent.__dockeyMock : null;
const mock = shared ?? create();
window.__dockeyMock = mock;
export default mock;
