import { invoke, listen, emit, applyLook, setAcrylicSupport, trackBackdrop } from './api.js';
import { t, locale, setSystemLanguage, errorText } from './i18n.js';
import { glyph, batteryGlyph } from './glyphs.js';
import { renderTemplate, parseStatus, effectiveOrder, mergeOrder } from './widgets.js';
import { loadCalendar, upcoming, eventsOnDay, startOfDay, addDays, fmtTime, fmtWhen } from './agenda.js';
import { currentWeather } from './weather.js';

const $ = (sel) => document.querySelector(sel);
const el = (tag, cls, html) => {
  const node = document.createElement(tag);
  if (cls) node.className = cls;
  if (html != null) node.innerHTML = html;
  return node;
};

const vertical = () => document.documentElement.dataset.orient === 'v';
const state = { config: null, groups: [], maximized: false, stats: null, media: null, weather: null, cpu: [], events: [], calErrors: [], tray: [] };

// Een pictogram uit het systeemvak; klikken gaan door naar de app.
function trayButton(item) {
  const b = el('button', 'tray-icon');
  b.title = item.tip;
  const img = el('img');
  img.alt = '';
  img.draggable = false;
  img.src = item.icon;
  b.append(img);
  const send = (right) => {
    const r = b.getBoundingClientRect();
    invoke('tray_click', { key: item.key, right, x: r.left + r.width / 2, y: r.top + r.height / 2 });
  };
  b.addEventListener('click', () => send(false));
  b.addEventListener('contextmenu', (e) => {
    e.preventDefault();
    e.stopPropagation();
    send(true);
  });
  return b;
}
let openId = null; // id van het paneel dat nu openstaat

// ----------------------------------------------------------------- paneel --

async function openPopup(id, kind, payload, anchorEl) {
  const r = anchorEl.getBoundingClientRect();
  const opened = await invoke('open_popup', { id, kind, payload, anchorX: r.left + r.width / 2, anchorY: r.top + r.height / 2 });
  markOpen(opened ? id : null);
}

function markOpen(id) {
  openId = id;
  for (const node of document.querySelectorAll('[data-popup]')) node.classList.toggle('open', node.dataset.popup === id);
}

const dockMenu = () => ({
  items: [
    { label: t('menu.settings'), icon: 'gear', emit: 'open-settings' },
    { label: t('menu.taskManager'), icon: 'chart', action: { cmd: 'launch', args: { target: 'taskmgr.exe' } } },
    { label: t('menu.taskView'), icon: 'windows', action: { cmd: 'send_keys', args: { name: 'taskview' } } },
    { label: t('menu.showDesktop'), icon: 'desktop', action: { cmd: 'send_keys', args: { name: 'desktop' } } },
    { sep: true },
    { label: t('menu.quit'), icon: 'power', danger: true, action: { cmd: 'quit' } },
  ],
});

function appMenu(g) {
  const items = [];
  const act = (hwnd, action) => ({ cmd: 'window_action', args: { hwnd, action } });
  for (const w of g.windows) {
    items.push({
      label: w.title || g.name, icon: 'windows', bold: w.active,
      action: act(w.hwnd, 'focus'),
      side: { icon: 'close', title: t('app.closeWindow'), action: act(w.hwnd, 'close') },
    });
  }
  if (g.windows.length) items.push({ sep: true });
  if (g.launch) items.push({ label: g.windows.length ? t('app.newWindow') : t('app.open'), icon: 'plus', action: { cmd: 'launch', args: { target: g.launch } } });
  if (g.pinned) {
    items.push({ label: t('app.unpin'), icon: 'pin', action: { cmd: 'unpin', args: { key: g.key } } });
    items.push({ label: t('app.moveLeft'), icon: 'left', action: { cmd: 'move_pin', args: { key: g.key, delta: -1 } } });
    items.push({ label: t('app.moveRight'), icon: 'right', action: { cmd: 'move_pin', args: { key: g.key, delta: 1 } } });
  } else if (g.launch) {
    items.push({ label: t('app.pin'), icon: 'pin', action: { cmd: 'pin', args: { key: g.key } } });
  }
  if (g.windows.length) {
    items.push({ sep: true });
    items.push({
      label: g.windows.length > 1 ? t('app.closeAll', g.windows.length) : t('app.closeWindow'),
      icon: 'close', danger: true, actions: g.windows.map((w) => act(w.hwnd, 'close')),
    });
  }
  return { title: g.name, items };
}

// ------------------------------------------------------------------- apps --

const tiles = new Map(); // groepssleutel → knop

// Iconen. Een gelukte opvraging blijft bewaard. Mislukt het, dan proberen we
// het opnieuw zodra er een venster van de app open is (de backend kan dan het
// vensterpictogram gebruiken), en verder hooguit eens per halve minuut.
const icons = new Map(); // groepssleutel → { url, pending, failedAt, triedWithWindow }

function showIcon(tile, url) {
  const img = tile.querySelector('img');
  if (img) {
    if (img.src !== url) img.src = url;
    return;
  }
  const fresh = el('img');
  fresh.alt = '';
  fresh.draggable = false;
  fresh.src = url;
  tile.querySelector('.letter')?.replaceWith(fresh);
}

function ensureIcon(tile, g) {
  let entry = icons.get(g.key);
  if (!entry) icons.set(g.key, (entry = {}));
  if (entry.url) return showIcon(tile, entry.url);
  if (entry.pending) return;
  const hwnd = g.windows[0]?.hwnd ?? null;
  const newChance = hwnd != null && !entry.triedWithWindow;
  if (entry.failedAt && !newChance && Date.now() - entry.failedAt < 30000) return;
  entry.pending = true;
  invoke('get_icon', { key: g.key, hwnd })
    .then((url) => {
      entry.url = url;
      const current = tiles.get(g.key);
      if (current) showIcon(current, url);
    })
    .catch(() => {
      entry.failedAt = Date.now();
      if (hwnd != null) entry.triedWithWindow = true;
    })
    .finally(() => (entry.pending = false));
}

function createTile(g) {
  const tile = el('button', 'tile app');
  tile.dataset.popup = `menu:${g.key}`;
  const letter = el('span', 'letter');
  letter.textContent = (g.name || '?').trim().charAt(0);
  tile.append(letter);

  tile.addEventListener('click', (e) => {
    const group = tile.group;
    if (e.shiftKey || group.windows.length === 0) return void invoke('launch', { target: group.launch });
    if (group.windows.length === 1) {
      const w = group.windows[0];
      return void invoke('window_action', { hwnd: w.hwnd, action: w.active ? 'minimize' : 'focus' });
    }
    openPopup(tile.dataset.popup, 'menu', appMenu(group), tile);
  });
  // Middelste muisknop: nieuw venster.
  tile.addEventListener('auxclick', (e) => {
    if (e.button === 1 && tile.group.launch) invoke('launch', { target: tile.group.launch });
  });
  tile.addEventListener('contextmenu', (e) => {
    e.preventDefault();
    e.stopPropagation();
    openPopup(tile.dataset.popup, 'menu', appMenu(tile.group), tile);
  });
  return tile;
}

function renderApps() {
  const container = $('#apps');
  const alive = new Set();
  let cursor = container.firstChild;
  for (const g of state.groups) {
    alive.add(g.key);
    let tile = tiles.get(g.key);
    if (!tile) {
      tile = createTile(g);
      tiles.set(g.key, tile);
    }
    tile.group = g;
    ensureIcon(tile, g);
    const n = g.windows.length;
    tile.classList.toggle('running', n > 0);
    tile.classList.toggle('multi', n > 1);
    tile.classList.toggle('active', g.windows.some((w) => w.active));
    tile.title = n === 1 ? g.windows[0].title || g.name : n > 1 ? t('app.windows', g.name, n) : g.name;
    if (tile !== cursor) container.insertBefore(tile, cursor);
    else cursor = cursor.nextSibling;
  }
  for (const [key, tile] of tiles) {
    if (!alive.has(key)) {
      tile.remove();
      tiles.delete(key);
    }
  }
}

// ---------------------------------------------------------------- widgets --

const widgets = {}; // naam → { root, update() }

function fmtRate(bytesPerSec, short = false) {
  const kb = bytesPerSec / 1024;
  if (short) return kb < 1000 ? `${Math.round(kb)}k` : `${(kb / 1024).toLocaleString(locale(), { maximumFractionDigits: 1 })}M`;
  if (kb < 1000) return `${Math.round(kb)} kB/s`;
  return `${(kb / 1024).toLocaleString(locale(), { maximumFractionDigits: 1, minimumFractionDigits: 1 })} MB/s`;
}

const builders = {
  media() {
    const root = el('div', 'w media', `<img class="art" alt="" draggable="false" hidden><div class="two"><span class="main"></span><span class="sub"></span></div>
      <div class="ctl"><button data-cmd="prev" title="${t('media.prev')}">${glyph.prev}</button><button data-cmd="toggle"></button><button data-cmd="next" title="${t('media.next')}">${glyph.next}</button></div>`);
    const [art, title, artist, toggle] = [root.querySelector('.art'), root.querySelector('.main'), root.querySelector('.sub'), root.querySelector('[data-cmd=toggle]')];
    root.querySelector('.ctl').addEventListener('click', (e) => {
      const cmd = e.target.closest('button')?.dataset.cmd;
      if (!cmd) return;
      invoke('media_cmd', { cmd });
      if (cmd === 'toggle' && state.media) {
        state.media.playing = !state.media.playing; // meteen tonen; de echte stand volgt
        widgets.media.update();
      }
    });
    return {
      root,
      update() {
        const m = state.media;
        root.hidden = !m;
        if (!m) return;
        title.textContent = m.title;
        artist.textContent = m.artist;
        artist.hidden = !m.artist;
        root.title = m.artist ? `${m.title} — ${m.artist}` : m.title;
        art.hidden = !m.art;
        if (m.art && art.src !== m.art) art.src = m.art;
        toggle.innerHTML = m.playing ? glyph.pause : glyph.play;
        toggle.title = m.playing ? t('media.pause') : t('media.play');
      },
    };
  },

  // Compact: icoon en temperatuur. Een klik toont de rest en de komende dagen.
  weather() {
    const root = el('button', 'w click weather', '<span class="emoji"></span><span class="main"></span>');
    root.dataset.popup = 'weather';
    const [emoji, temp] = [root.querySelector('.emoji'), root.querySelector('.main')];
    root.addEventListener('click', () => openPopup('weather', 'weather', {}, root));
    return {
      root,
      update() {
        const w = state.weather;
        root.hidden = !w;
        if (!w) return;
        emoji.textContent = w.emoji;
        temp.textContent = `${Math.round(w.temp)}°`;
        root.title = t('weather.tip', state.config.weather.place, w.text, w.temp.toLocaleString(locale(), { maximumFractionDigits: 1 }));
      },
    };
  },

  system() {
    const root = el('button', 'w click system', `<svg class="spark" viewBox="0 0 30 20" preserveAspectRatio="none" aria-hidden="true"><polygon/><polyline/></svg>
      <div class="two"><span class="main"></span><span class="sub"></span></div>`);
    const [area, line, cpu, ram] = [root.querySelector('polygon'), root.querySelector('polyline'), root.querySelector('.main'), root.querySelector('.sub')];
    root.addEventListener('click', () => invoke('launch', { target: 'taskmgr.exe' }));
    return {
      root,
      update() {
        const s = state.stats;
        if (!s) return;
        // Vaste schaal 0–100 %, laatste 30 metingen (± 45 s).
        const pts = state.cpu.map((v, i) => `${i + (30 - state.cpu.length)},${(19 - (v / 100) * 18).toFixed(1)}`);
        line.setAttribute('points', pts.join(' '));
        area.setAttribute('points', pts.length ? `${30 - state.cpu.length},20 ${pts.join(' ')} 29,20` : '');
        cpu.textContent = vertical() ? `${Math.round(s.cpu)}%` : `CPU ${Math.round(s.cpu)}%`;
        ram.textContent = `RAM ${Math.round(s.ram)}%`;
        const gb = (v) => v.toLocaleString(locale(), { maximumFractionDigits: 1 });
        root.title = t('system.tip', Math.round(s.cpu), gb(s.ramUsedGb), gb(s.ramTotalGb));
      },
    };
  },

  network() {
    const root = el('div', 'w network', `<div class="two"><span class="row main">${glyph.down}<span class="d"></span></span><span class="row sub">${glyph.up}<span class="u"></span></span></div>`);
    const [d, u] = [root.querySelector('.d'), root.querySelector('.u')];
    return {
      root,
      update() {
        const s = state.stats;
        if (!s) return;
        d.textContent = fmtRate(s.down, vertical());
        u.textContent = fmtRate(s.up, vertical());
        root.title = t('network.tip', fmtRate(s.down), fmtRate(s.up));
      },
    };
  },

  volume() {
    const root = el('button', 'w click volume', '<span class="ico"></span><span class="main"></span>');
    const [ico, val] = [root.querySelector('.ico'), root.querySelector('.main')];
    root.addEventListener('click', () => invoke('send_keys', { name: 'quick' }));
    root.addEventListener('auxclick', (e) => {
      if (e.button !== 1 || !state.stats) return;
      state.stats.muted = !state.stats.muted;
      invoke('set_mute', { muted: state.stats.muted });
      widgets.volume.update();
    });
    root.addEventListener('wheel', (e) => {
      const s = state.stats;
      if (!s || s.volume == null) return;
      e.preventDefault();
      s.volume = Math.min(1, Math.max(0, Math.round(s.volume * 50 + (e.deltaY < 0 ? 1 : -1)) / 50)); // stappen van 2%
      invoke('set_volume', { level: s.volume });
      widgets.volume.update();
    }, { passive: false });
    return {
      root,
      update() {
        const s = state.stats;
        root.hidden = !s || s.volume == null;
        if (root.hidden) return;
        const pct = Math.round(s.volume * 100);
        ico.innerHTML = s.muted || pct === 0 ? glyph.mute : pct < 40 ? glyph.volumeLow : glyph.volume;
        val.textContent = s.muted ? t('volume.off') : `${pct}%`;
        root.title = t('volume.tip', pct, s.muted);
      },
    };
  },

  battery() {
    const root = el('button', 'w click battery', '<span class="ico"></span><span class="main"></span>');
    const [ico, val] = [root.querySelector('.ico'), root.querySelector('.main')];
    root.addEventListener('click', () => invoke('send_keys', { name: 'quick' }));
    return {
      root,
      update() {
        const s = state.stats;
        root.hidden = !s || s.battery == null;
        if (root.hidden) return;
        ico.innerHTML = batteryGlyph(s.battery, s.charging);
        val.textContent = `${s.battery}%`;
        root.title = t('battery.tip', s.battery, s.charging);
      },
    };
  },

  // Systeemvak: vastgezette pictogrammen, de rest achter ^.
  tray() {
    const root = el('div', 'w tray', `<div class="tray-icons"></div><button class="tray-more" title="${t('tray.more')}">${glyph.chevron}</button>`);
    const [icons, more] = [root.querySelector('.tray-icons'), root.querySelector('.tray-more')];
    more.dataset.popup = 'tray';
    more.addEventListener('click', () => openPopup('tray', 'tray', {}, more));
    let lastRect = '';
    return {
      root,
      update() {
        const items = state.tray ?? [];
        root.hidden = !items.length;
        if (root.hidden) return;
        const pinned = (state.config.trayPinned ?? []).map((k) => items.find((i) => i.key === k)).filter(Boolean);
        more.hidden = pinned.length === items.length;
        icons.replaceChildren(...pinned.map(trayButton));
        // Apps vragen soms waar hun pictogram staat (om een venster ernaast te openen).
        requestAnimationFrame(() => {
          const r = root.getBoundingClientRect();
          const rect = { left: r.left, top: r.top, right: r.right, bottom: r.bottom };
          if (JSON.stringify(rect) !== lastRect) {
            lastRect = JSON.stringify(rect);
            invoke('tray_rect', rect);
          }
        });
      },
    };
  },

  // Altijd alleen het icoon. Muis erop: de afspraken van vandaag; klik: de kalender.
  // Een stipje in de kleur van de agenda zolang er vandaag nog iets komt.
  agenda() {
    const root = el('button', 'w click agenda', `<span class="ico">${glyph.calendar}<i class="dot"></i></span>`);
    root.dataset.popup = 'agenda';
    const dot = root.querySelector('.dot');
    root.addEventListener('click', () => openPopup('agenda', 'calendar', {}, root));
    return {
      root,
      update() {
        root.hidden = !(state.config.calendars ?? []).length;
        if (root.hidden) return;
        const now = Date.now();
        const today = eventsOnDay(state.events, startOfDay(now));
        const coming = today.filter((e) => e.end > now);
        dot.hidden = !coming.length;
        if (coming.length) dot.style.background = coming[0].color;
        const unreachable = state.calErrors.length && !state.events.length;
        root.dataset.status = unreachable ? 'error' : '';

        const line = (e) => {
          const when = e.allDay || (e.start <= startOfDay(now) && e.end >= addDays(startOfDay(now), 1)) ? t('agenda.allDay') : `${fmtTime(e.start)}–${fmtTime(e.end)}`;
          return `• ${when}  ${e.title}${e.end <= now ? t('agenda.past') : ''}`;
        };
        const tip = [];
        if (unreachable) tip.push(t('agenda.unreachable'));
        else if (today.length) tip.push(t('agenda.today'), ...today.map(line));
        else {
          tip.push(t('agenda.noneToday'));
          const next = upcoming(state.events, now, 8, 1)[0];
          if (next) tip.push(t('agenda.next', fmtWhen(next, now), next.title));
        }
        tip.push(...state.calErrors.map((x) => t('agenda.unreachableItem', x)), t('agenda.click'));
        root.title = tip.join('\n');
      },
    };
  },

  clock() {
    const root = el('button', 'w click clock', '<div class="two end"><span class="main"></span><span class="sub"></span></div>');
    root.dataset.popup = 'calendar';
    const [time, date] = [root.querySelector('.main'), root.querySelector('.sub')];
    root.addEventListener('click', () => openPopup('calendar', 'calendar', {}, root));
    return {
      root,
      update() {
        const now = new Date();
        const c = state.config;
        time.textContent = now.toLocaleTimeString(locale(), {
          hour: c.clock24 ? '2-digit' : 'numeric', minute: '2-digit', second: c.showSeconds ? '2-digit' : undefined, hour12: !c.clock24,
        });
        date.textContent = now.toLocaleDateString(locale(), vertical() ? { day: 'numeric', month: 'short' } : { weekday: 'short', day: 'numeric', month: 'short' });
        root.title = now.toLocaleDateString(locale(), { weekday: 'long', day: 'numeric', month: 'long', year: 'numeric' });
      },
    };
  },
};

// --------------------------------------------------------- eigen widgets --

const customTimers = [];

const CUSTOM_KINDS = ['json', 'status'];

/** Een eigen widget: "json" (tekst of waarde van een adres, met sjabloon) of
 *  "status" (het statusformaat, zie widgets.js). Onbekende soorten (bijv. uit
 *  een oudere versie) slaan we over. */
function buildCustom(cw) {
  if (!cw || !CUSTOM_KINDS.includes(cw.kind)) return null;
  const root = el('button', 'w custom', '<span class="ico"></span><div class="two"><span class="main">…</span><span class="sub"></span></div>');
  const [ico, main, sub] = [root.querySelector('.ico'), root.querySelector('.main'), root.querySelector('.sub')];
  const isStatus = cw.kind === 'status';
  sub.textContent = cw.name || '';
  ico.textContent = isStatus ? '' : cw.icon || '';
  ico.hidden = isStatus || !cw.icon;

  // Klikken opent het adres uit de instellingen, of anders de link uit het antwoord.
  let link = '';
  root.addEventListener('click', () => {
    const target = cw.openUrl || link;
    if (target) invoke('launch', { target });
  });
  root.classList.toggle('click', Boolean(cw.openUrl));

  // Het icoon uit het antwoord: dan alleen het icoon, met de status als rand.
  let iconUrl = '';
  const showIcon = (url) => {
    if (!url || url === iconUrl) return;
    iconUrl = url;
    invoke('fetch_image', { url, insecure: cw.insecure })
      .then((src) => {
        const img = el('img');
        img.alt = '';
        img.draggable = false;
        img.src = src;
        ico.replaceChildren(img);
        ico.hidden = false;
        root.classList.add('status-icon');
      })
      .catch(() => {});
  };

  let result = null; // { status, text, short, tip }
  const render = () => {
    if (!result) return;
    root.dataset.status = result.status;
    main.textContent = vertical() ? result.short : result.text;
    root.title = result.tip;
  };

  async function refresh() {
    try {
      const body = await invoke('fetch_url', { url: cw.url, insecure: cw.insecure });
      if (isStatus) {
        const s = parseStatus(body, cw.url);
        result = { status: s.status, text: s.text, short: s.short, tip: [cw.name ? `${cw.name}: ${s.text}` : s.text, s.detail].filter(Boolean).join('\n') };
        link = s.link;
        root.classList.toggle('click', Boolean(cw.openUrl || link));
        showIcon(s.icon);
      } else {
        const text = renderTemplate(cw.template, body);
        result = { status: '', text, short: text.length > 5 ? text.slice(0, 5) : text, tip: cw.name ? `${cw.name}: ${text}` : text };
      }
    } catch (err) {
      result = { status: 'error', text: t('custom.noConnection'), short: '!', tip: `${cw.name || t('custom.widget')}: ${errorText(err)}` };
    }
    render();
  }

  refresh();
  customTimers.push(setInterval(refresh, Math.max(15, Number(cw.interval) || 60) * 1000));
  return { root, update: render };
}

function buildWidgets() {
  const container = $('#widgets');
  container.replaceChildren();
  for (const name of Object.keys(widgets)) delete widgets[name];
  customTimers.splice(0).forEach(clearInterval);
  for (const key of effectiveOrder(state.config)) {
    if (key.startsWith('custom:')) {
      const built = buildCustom(state.config.customWidgets.find((c) => `custom:${c.id}` === key));
      if (!built) continue;
      widgets[key] = built;
    } else if (state.config.widgets[key]) {
      widgets[key] = builders[key]();
      widgets[key].update();
    } else continue;
    widgets[key].root.dataset.key = key;
    container.append(widgets[key].root);
  }
  $('#sep').hidden = container.children.length === 0;
}

/** Zet de bestaande widgets in de bewaarde volgorde, zonder ze opnieuw te bouwen. */
function arrangeWidgets() {
  const container = $('#widgets');
  for (const key of effectiveOrder(state.config)) if (widgets[key]) container.append(widgets[key].root);
}

// ------------------------------------------------- slepen = volgorde --
//
// Een widget vastpakken en minstens 6 px bewegen begint met slepen; daaronder
// blijft het een gewone klik. De andere widgets schuiven soepel opzij.

let drag = null;

function flip(container, move) {
  const before = new Map([...container.children].map((el) => [el, el.getBoundingClientRect()]));
  move();
  for (const [el, r] of before) {
    if (el === drag?.item) continue;
    const now = el.getBoundingClientRect();
    const dx = r.left - now.left;
    const dy = r.top - now.top;
    if (dx || dy) el.animate([{ transform: `translate(${dx}px, ${dy}px)` }, { transform: 'none' }], { duration: 160, easing: 'ease-out' });
  }
}

function startDrag(e) {
  if (e.button !== 0) return;
  const item = e.target.closest('#widgets > [data-key]');
  if (!item || e.target.closest('.ctl')) return; // mediaknoppen blijven knoppen
  drag = { item, x: e.clientX, y: e.clientY, pointer: e.pointerId, active: false, before: [...item.parentNode.children] };
}

function moveDrag(e) {
  if (!drag || e.pointerId !== drag.pointer) return;
  const container = drag.item.parentNode;
  if (!drag.active) {
    if (Math.hypot(e.clientX - drag.x, e.clientY - drag.y) < 6) return;
    drag.active = true;
    container.setPointerCapture(e.pointerId);
    drag.item.classList.add('dragging');
    document.documentElement.classList.add('sorting');
  }
  const v = vertical();
  const pos = v ? e.clientY : e.clientX;
  // Verborgen widgets (zoals de agenda zonder gekoppelde agenda) tellen niet mee.
  const items = [...container.children].filter((el) => !el.hidden);
  const own = items.indexOf(drag.item);
  const mids = items.map((el) => {
    const r = el.getBoundingClientRect();
    return v ? r.top + r.height / 2 : r.left + r.width / 2;
  });
  let target = own;
  for (let i = own + 1; i < items.length; i++) if (pos > mids[i]) target = i;
  for (let i = own - 1; i >= 0; i--) if (pos < mids[i]) target = i;
  if (target !== own) flip(container, () => (target > own ? items[target].after(drag.item) : items[target].before(drag.item)));
}

function endDrag(e) {
  if (!drag || e.pointerId !== drag.pointer) return;
  const done = drag;
  drag = null;
  if (!done.active) return;
  done.item.classList.remove('dragging');
  document.documentElement.classList.remove('sorting');
  const container = done.item.parentNode;
  if (e.type === 'pointercancel') {
    container.append(...done.before);
    return;
  }
  // De klik die op het loslaten volgt, mag niets openen.
  const swallow = (ev) => {
    ev.stopPropagation();
    ev.preventDefault();
  };
  window.addEventListener('click', swallow, true);
  setTimeout(() => window.removeEventListener('click', swallow, true), 0);

  const visible = [...container.children].map((el) => el.dataset.key);
  const widgetOrder = mergeOrder(effectiveOrder(state.config), visible);
  if (JSON.stringify(widgetOrder) !== JSON.stringify(effectiveOrder(state.config))) {
    invoke('set_config', { config: { ...state.config, widgetOrder } });
  }
}

$('#widgets').addEventListener('pointerdown', startDrag);
$('#widgets').addEventListener('pointermove', moveDrag);
$('#widgets').addEventListener('pointerup', endDrag);
$('#widgets').addEventListener('pointercancel', endDrag);

const update = (...names) => names.forEach((n) => widgets[n]?.update());

// ------------------------------------------------------------- agenda's --

let calendarKey = '';

async function refreshCalendars() {
  const cals = state.config.calendars ?? [];
  const results = await Promise.all(
    cals.map((cal) => loadCalendar(invoke, cal).then((events) => ({ events }), (err) => ({ err: `${cal.name}: ${errorText(err)}` }))),
  );
  state.events = results.flatMap((r) => r.events ?? []).sort((a, b) => a.start - b.start);
  state.calErrors = results.filter((r) => r.err).map((r) => r.err);
  shareAgenda();
  update('agenda');
}

/** Het paneel (kalender) krijgt dezelfde afspraken. */
const shareAgenda = () => emit('agenda', { events: state.events, errors: state.calErrors, calendars: state.config.calendars ?? [] });

// ------------------------------------------------------------------- weer --

let weatherKey = '';
async function refreshWeather() {
  const { widgets: on, weather } = state.config;
  if (!on.weather || !weather.place) {
    state.weather = null;
    return update('weather');
  }
  try {
    state.weather = await currentWeather(weather);
  } catch {
    // Geen verbinding: laat de vorige waarde staan.
  }
  update('weather');
}

// ------------------------------------------------------------------ start --

/** Volle breedte zodra een venster tegen de balk aan staat (zie style.css). */
function applyMaximized() {
  document.documentElement.toggleAttribute('data-max', state.maximized && !!state.config?.expandOnMaximize);
}

function setShell(snapshot) {
  state.groups = snapshot.groups;
  state.maximized = snapshot.maximized;
  renderApps();
  applyMaximized();
}

function setConfig(config) {
  const first = !state.config;
  const orderChanged = !first && JSON.stringify(config.widgetOrder) !== JSON.stringify(state.config.widgetOrder);
  const widgetsChanged =
    first ||
    JSON.stringify(config.widgets) !== JSON.stringify(state.config.widgets) ||
    JSON.stringify(config.customWidgets) !== JSON.stringify(state.config.customWidgets);
  state.config = config;
  const languageChanged = applyLook(config) && !first;
  start.title = t('start');
  applyMaximized();
  if (languageChanged) renderApps(); // titels van de apps
  if (widgetsChanged || languageChanged) buildWidgets();
  else {
    if (orderChanged) arrangeWidgets();
    update(...Object.keys(widgets)); // teksten kunnen per stand (horizontaal/verticaal) verschillen
  }
  const calKey = JSON.stringify(config.calendars ?? []);
  if (calKey !== calendarKey) {
    calendarKey = calKey;
    refreshCalendars();
  }
  const key = `${config.widgets.weather}|${config.weather.lat}|${config.weather.lon}`;
  if (key !== weatherKey) {
    weatherKey = key;
    refreshWeather();
  }
}

function setStats(stats) {
  if (!stats) return;
  state.stats = stats;
  state.cpu.push(stats.cpu);
  if (state.cpu.length > 30) state.cpu.shift();
  update('system', 'network', 'volume', 'battery');
}

const start = $('#start');
start.innerHTML = glyph.start;
start.dataset.popup = 'dock-menu';
start.addEventListener('click', () => invoke('send_keys', { name: 'start' }));

const openSettings = () => openPopup('settings', 'settings', {}, start);
// Klik op een lege plek van de balk: een open paneel gaat dicht. (De balk pakt
// zelf geen focus, dus het paneel merkt die klik niet vanzelf.)
document.addEventListener('pointerdown', (e) => {
  if (e.button === 0 && !e.target.closest('button, .w')) invoke('close_popup');
});

// Rechtsklik op een lege plek (of op de startknop): het balkmenu.
document.addEventListener('contextmenu', (e) => {
  e.preventDefault();
  openPopup('dock-menu', 'menu', dockMenu(), start);
});

// Eerst luisteren, dan pas de beginstand ophalen: zo gaat er geen wijziging
// verloren die precies tijdens het opstarten binnenkomt.
await Promise.all([
  listen('config', setConfig),
  listen('shell', setShell),
  listen('stats', setStats),
  listen('media', (media) => {
    state.media = media;
    update('media');
  }),
  listen('popup:closed', () => markOpen(null)),
  listen('open-settings', openSettings),
  listen('agenda:request', shareAgenda),
  listen('tray', (items) => {
    state.tray = items;
    update('tray');
  }),
]);

const initial = await invoke('get_initial');
setAcrylicSupport(initial.acrylic);
setSystemLanguage(initial.systemLanguage);
setConfig(initial.config);
trackBackdrop($('#dock'));
state.media = initial.media;
setShell(initial.shell);
invoke('tray_icons').then((items) => {
  state.tray = items;
  update('tray');
});
setStats(initial.stats);
update('media');

setInterval(() => update('clock'), 1000);
setInterval(() => update('agenda'), 30 * 1000);
setInterval(refreshCalendars, 15 * 60 * 1000);
setInterval(renderApps, 30 * 1000); // o.a. mislukte iconen opnieuw proberen
setInterval(refreshWeather, 15 * 60 * 1000);
