// Venstervoorbeelden: een kaart per open venster van een app, met titel,
// icoon en sluitknop. Het beeld zelf tekent Windows (een DWM-thumbnail) op
// de plek die wij doorgeven; deze pagina laat daar een leeg vlak voor vrij.
// Het venster pakt nooit de focus; de balk regelt wanneer het verschijnt.
import { invoke, listen, emit, applyLook, setAcrylicSupport, setSystemLook, trackBackdrop, isMock } from './api.js';
import { t, setSystemLanguage } from './i18n.js';
import { glyph } from './glyphs.js';

const root = document.getElementById('preview');
const el = (tag, cls) => {
  const node = document.createElement(tag);
  if (cls) node.className = cls;
  return node;
};

const initial = await invoke('get_initial');
setAcrylicSupport(initial.acrylic);
setSystemLook(initial.look);
setSystemLanguage(initial.systemLanguage);
applyLook(initial.config);
trackBackdrop(root);
if (isMock) document.documentElement.classList.add('mock');
listen('config', (config) => applyLook(config));

/** Hoogte van een beeld in CSS-pixels, en de grenzen voor de breedte
 *  (als verhouding): heel brede of smalle vensters krijgen randen. */
const THUMB_H = 120;
const MIN_RATIO = 1.25; // smaller niet: dan past de titel er nauwelijks boven
const MAX_RATIO = 2.1;
/** Verhouding van het vak voor een venster zonder beeld (geminimaliseerd). */
const NO_IMAGE_RATIO = 1.5;
/** Ruimte rond het voorbeeld voor de schaduw (zie style.css). */
const MARGIN = 12;

let current = null; // laatste aanvraag: { serial, key, name, icon, windows, maxWidth }

function boxRatio(w) {
  return w.ratio ? Math.min(MAX_RATIO, Math.max(MIN_RATIO, w.ratio)) : NO_IMAGE_RATIO;
}

function card(w, height) {
  const c = el('div', `pv-card${w.active ? ' active' : ''}`);
  const head = el('div', 'pv-head');
  if (current.icon) {
    const img = el('img');
    img.alt = '';
    img.draggable = false;
    img.src = current.icon;
    head.append(img);
  }
  const title = el('span', 'pv-title');
  title.textContent = w.title || current.name;
  const x = el('button', 'pv-close');
  x.innerHTML = glyph.close;
  x.title = t('app.closeWindow');
  x.addEventListener('click', (e) => {
    e.stopPropagation();
    invoke('window_action', { hwnd: w.hwnd, action: 'close' });
    // Meteen weghalen; de balk stuurt de nieuwe stand zodra het venster weg is.
    current.windows = current.windows.filter((v) => v.hwnd !== w.hwnd);
    if (current.windows.length) render();
    else invoke('close_preview');
  });
  head.append(title, x);

  // Het vak; het beeld komt er gecentreerd in, met de verhouding van het venster.
  const box = el('div', 'pv-box');
  const width = Math.round(height * boxRatio(w));
  c.style.width = `${width + 16}px`; // + opvulling; een lange titel maakt de kaart niet breder
  box.style.width = `${width}px`;
  box.style.height = `${height}px`;
  if (w.ratio) {
    const fit = Math.min(width / (height * w.ratio), 1);
    const shot = el('div', 'pv-shot');
    shot.dataset.hwnd = String(w.hwnd);
    shot.style.width = `${Math.round(height * w.ratio * fit)}px`;
    shot.style.height = `${Math.round(height * fit)}px`;
    box.append(shot);
  } else if (current.icon) {
    const big = el('img', 'pv-icon');
    big.alt = '';
    big.draggable = false;
    big.src = current.icon;
    box.append(big);
  }
  c.append(head, box);
  c.addEventListener('click', () => {
    invoke('window_action', { hwnd: w.hwnd, action: 'focus' });
    invoke('close_preview');
  });
  return c;
}

function render() {
  const vertical = document.documentElement.dataset.orient === 'v';
  root.classList.toggle('v', vertical);
  // Past het niet naast elkaar op het scherm, dan worden de beelden kleiner.
  let height = THUMB_H;
  if (!vertical) {
    // Opvulling per kaart (16) en tussenruimte (6); opvulling en rand rondom (18) en de marge.
    const overhead = 22 * current.windows.length + 18 + MARGIN * 2;
    const units = current.windows.reduce((sum, w) => sum + boxRatio(w), 0);
    height = Math.max(56, Math.min(THUMB_H, Math.floor((current.maxWidth - overhead) / units)));
  }
  root.replaceChildren(...current.windows.map((w) => card(w, height)));
  // Een verborgen venster kan nog een hover-stand van de vorige keer hebben;
  // pas na een echte muisbeweging hier lichten kaarten op.
  root.classList.add('idle');

  const slots = [...root.querySelectorAll('.pv-shot')].map((shot) => {
    const r = shot.getBoundingClientRect();
    return { hwnd: Number(shot.dataset.hwnd), x: r.left, y: r.top, width: r.width, height: r.height };
  });
  return invoke('preview_ready', {
    serial: current.serial,
    width: root.offsetWidth + MARGIN * 2,
    height: root.offsetHeight + MARGIN * 2,
    slots,
  });
}

listen('preview:render', (request) => {
  current = request;
  render();
});

// De balk wil weten of de muis hier is: dan blijft het voorbeeld open.
document.documentElement.addEventListener('mouseenter', () => emit('preview:hover', true));
document.addEventListener('mousemove', () => root.classList.remove('idle'));
document.documentElement.addEventListener('mouseleave', () => emit('preview:hover', false));
