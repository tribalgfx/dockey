// Dunne laag boven Tauri. Buiten Tauri (gewoon in een browser) laden we een
// nagebootste backend, zodat de balk te bekijken is zonder Windows.
import { setLanguage } from './i18n.js';

const tauri = window.__TAURI__ ?? (await import('./mock.js')).default;

export const isMock = !window.__TAURI__;
export const invoke = (cmd, args) => tauri.core.invoke(cmd, args);
export const listen = (event, handler) => tauri.event.listen(event, (e) => handler(e.payload));
export const emit = (event, payload) => tauri.event.emit(event, payload);

// Kan Windows echte vervaging (acrylic) achter het glasthema tekenen? Dat
// meldt de backend in get_initial; de voorbeeldmodus kan het met ?acrylic=1
// nabootsen (zonder echte vervaging).
let acrylicSupported = isMock && new URLSearchParams(location.search).has('acrylic');
export function setAcrylicSupport(supported) {
  if (!isMock) acrylicSupported = Boolean(supported);
}

/** Zet thema, grootte, vorm en positie als attributen op <html>, en kiest de
 *  taal. Geeft true als de taal veranderde (dan teksten opnieuw tekenen). */
export function applyLook(config) {
  const root = document.documentElement;
  const languageChanged = setLanguage(config);
  root.dataset.theme = config.theme;
  root.dataset.size = config.size;
  root.dataset.layout = config.layout;
  root.dataset.pos = config.position;
  root.dataset.orient = config.position === 'left' || config.position === 'right' ? 'v' : 'h';
  root.toggleAttribute('data-acrylic', acrylicSupported && config.theme === 'glas');
  // Transparantie 0–80 %: 0 = zo dekkend als het thema, 80 = veel doorzichtiger.
  const transparency = Math.min(80, Math.max(0, Number(config.transparency) || 0));
  root.style.setProperty('--bg-opacity', String(1 - transparency / 100));
  return languageChanged;
}

/** Meldt de backend waar het glasvlak `node` staat, zodat Windows het
 *  bureaublad precies daarachter vervaagt. Volgt grootte, plek en de ronde of
 *  rechte hoeken (balk tegen de rand); zonder data-acrylic: geen vervaging. */
export function trackBackdrop(node) {
  if (isMock) return;
  const root = document.documentElement;
  let last = '';
  const send = () => {
    let rect = null;
    let round = false;
    if (root.hasAttribute('data-acrylic')) {
      const r = node.getBoundingClientRect();
      rect = { x: r.left, y: r.top, width: r.width, height: r.height };
      round = (parseFloat(getComputedStyle(node).borderTopLeftRadius) || 0) >= 4;
    }
    const key = JSON.stringify([rect, round]);
    if (key === last) return;
    last = key;
    invoke('set_backdrop', { rect, round }).catch(() => {});
  };
  new ResizeObserver(send).observe(node);
  new MutationObserver(send).observe(root, { attributes: true });
  node.addEventListener('transitionend', send);
  send();
}
