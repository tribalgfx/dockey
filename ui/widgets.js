// Gedeelde logica voor eigen widgets (balk én instellingen).
import { t, locale } from './i18n.js';

const valueAt = (data, path) => path.split('.').reduce((v, key) => (v == null ? undefined : v[key]), data);

function fmtValue(v) {
  if (typeof v === 'number') return v.toLocaleString(locale(), { maximumFractionDigits: 1 });
  if (v == null) return '–';
  return typeof v === 'object' ? JSON.stringify(v) : String(v);
}

function parse(text) {
  try {
    return JSON.parse(text);
  } catch {
    return text;
  }
}

/** Vult {pad.naar.waarde} in met waarden uit de JSON. Zonder sjabloon: de tekst zelf. */
export function renderTemplate(template, text) {
  const data = parse(text);
  if (!template?.trim()) return (typeof data === 'string' ? data : fmtValue(data)).trim().slice(0, 40);
  return template.replace(/\{([^}]*)\}/g, (_, path) => fmtValue(path.trim() ? valueAt(data, path.trim()) : data));
}

// --------------------------------------------------------- statuswidget --
//
// Een statuswidget leest een klein JSON-formaat dat elke dienst kan leveren
// (zie "Statuswidget" in de README):
//   { "status": "ok" | "warning" | "critical", "text": "Alles in orde",
//     "short": "✓", "detail": "• …", "icon": "/logo.png", "link": "https://…" }
// Alleen status en text zijn verplicht. icon en link mogen relatief zijn
// (ten opzichte van het adres van de widget).

const STATUSES = ['ok', 'warning', 'critical'];

const absolute = (value, base) => {
  if (typeof value !== 'string' || !value.trim()) return '';
  try {
    const url = new URL(value.trim(), base);
    return url.protocol === 'http:' || url.protocol === 'https:' ? url.href : '';
  } catch {
    return '';
  }
};

const line = (value, max) => (typeof value === 'string' ? value.trim().slice(0, max) : '');

/** Leest het antwoord van een statuswidget; gooit een fout bij een ander formaat. */
export function parseStatus(text, url) {
  let data;
  try {
    data = JSON.parse(text);
  } catch {
    data = null;
  }
  const status = typeof data?.status === 'string' ? data.status.toLowerCase() : '';
  if (!STATUSES.includes(status) || !line(data.text, 80)) throw new Error(t('status.invalid'));
  const shown = line(data.text, 80);
  return {
    status,
    text: shown,
    short: line(data.short, 6) || (status === 'ok' ? '✓' : '!'),
    detail: line(data.detail, 600),
    icon: absolute(data.icon, url),
    link: absolute(data.link, url),
  };
}

// ------------------------------------------------------------- volgorde --

const BUILTIN = ['media', 'weather', 'system', 'network', 'tray', 'volume', 'battery', 'agenda', 'clock'];

/** Standaardvolgorde: eigen widgets tussen de systeemwidgets en systeemvak/volume/klok. */
function defaultOrder(config) {
  const customs = (config.customWidgets ?? []).map((c) => `custom:${c.id}`);
  const at = BUILTIN.indexOf('tray');
  return [...BUILTIN.slice(0, at), ...customs, ...BUILTIN.slice(at)];
}

/** De volgorde zoals de gebruiker die sleepte; nieuwe widgets komen direct na
 *  hun voorganger uit de standaardvolgorde. Bevat alle widgets, ook uitgezette. */
export function effectiveOrder(config) {
  const known = defaultOrder(config);
  const order = (config.widgetOrder ?? []).filter((key, i, all) => known.includes(key) && all.indexOf(key) === i);
  known.forEach((key, i) => {
    if (order.includes(key)) return;
    const prev = known.slice(0, i).reverse().find((p) => order.includes(p));
    order.splice(prev ? order.indexOf(prev) + 1 : 0, 0, key);
  });
  return order;
}

/** Nieuwe volgorde van de zichtbare widgets inpassen in de volledige lijst: de
 *  zichtbare plekken krijgen de nieuwe volgorde, verborgen widgets blijven staan. */
export function mergeOrder(full, visible) {
  const queue = [...visible];
  return full.map((key) => (visible.includes(key) ? queue.shift() : key));
}

export const newId = () => Date.now().toString(36) + Math.random().toString(36).slice(2, 6);
