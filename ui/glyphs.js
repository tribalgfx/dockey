// Kleine lijniconen (16×16). Kleur volgt de tekstkleur.
const svg = (body, extra = '') =>
  `<svg viewBox="0 0 16 16" width="16" height="16" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" ${extra}>${body}</svg>`;
const solid = (body) => `<svg viewBox="0 0 16 16" width="16" height="16" fill="currentColor">${body}</svg>`;

export const glyph = {
  // Raster van 3×3 punten, zoals de knop "Toon apps" in Ubuntu en GNOME.
  start: `<svg viewBox="0 0 20 20" width="20" height="20" fill="currentColor">${[3.5, 10, 16.5]
    .flatMap((y) => [3.5, 10, 16.5].map((x) => `<circle cx="${x}" cy="${y}" r="2"/>`))
    .join('')}</svg>`,
  prev: solid('<path d="M4 3.5v9M12.5 3.6v8.8L5.8 8z" stroke="currentColor" stroke-width="1.4" stroke-linejoin="round" stroke-linecap="round"/>'),
  next: solid('<path d="M12 3.5v9M3.5 3.6v8.8L10.2 8z" stroke="currentColor" stroke-width="1.4" stroke-linejoin="round" stroke-linecap="round"/>'),
  play: solid('<path d="M4.5 2.8v10.4L13 8z" stroke="currentColor" stroke-width="1.2" stroke-linejoin="round"/>'),
  pause: solid('<rect x="3.6" y="2.8" width="3.2" height="10.4" rx="1"/><rect x="9.2" y="2.8" width="3.2" height="10.4" rx="1"/>'),
  volume: svg('<path d="M2.5 6.2h2.3L8 3.4v9.2L4.8 9.8H2.5z"/><path d="M10.6 5.8a3 3 0 0 1 0 4.4M12.4 4a5.5 5.5 0 0 1 0 8"/>'),
  volumeLow: svg('<path d="M2.5 6.2h2.3L8 3.4v9.2L4.8 9.8H2.5z"/><path d="M10.6 5.8a3 3 0 0 1 0 4.4"/>'),
  mute: svg('<path d="M2.5 6.2h2.3L8 3.4v9.2L4.8 9.8H2.5z"/><path d="M10.8 6l3 4M13.8 6l-3 4"/>'),
  down: svg('<path d="M8 3v9M4.5 8.8 8 12.3l3.5-3.5"/>'),
  up: svg('<path d="M8 13V4M4.5 7.2 8 3.7l3.5 3.5"/>'),
  gear: svg('<circle cx="8" cy="8" r="2.2"/><path d="M8 1.8v1.6M8 12.6v1.6M1.8 8h1.6M12.6 8h1.6M3.6 3.6l1.1 1.1M11.3 11.3l1.1 1.1M3.6 12.4l1.1-1.1M11.3 4.7l1.1-1.1"/>'),
  close: svg('<path d="M4 4l8 8M12 4l-8 8"/>'),
  pin: svg('<path d="M9.8 2.2 13.8 6.2 11.6 7 9.4 9.2 9.6 11.8 4.2 6.4 6.8 6.6 9 4.4z"/><path d="M6.8 9.2 2.8 13.2"/>'),
  plus: svg('<path d="M8 3.2v9.6M3.2 8h9.6"/>'),
  power: svg('<path d="M8 2.2v5.2"/><path d="M4.6 4.4a5 5 0 1 0 6.8 0"/>'),
  desktop: svg('<rect x="2" y="3" width="12" height="8" rx="1.5"/><path d="M6 13.5h4"/>'),
  chart: svg('<path d="M2.5 11.5 5.5 8l2.5 2 5-6"/>'),
  left: svg('<path d="M10 3.5 5.5 8l4.5 4.5"/>'),
  right: svg('<path d="M6 3.5 10.5 8 6 12.5"/>'),
  bell: svg('<path d="M4 11V7.4a4 4 0 0 1 8 0V11l1 1.3H3z"/><path d="M6.8 14h2.4"/>'),
  windows: svg('<rect x="2.5" y="3" width="8" height="6.5" rx="1.2"/><path d="M5.5 12.5h6.8a1.2 1.2 0 0 0 1.2-1.2V6"/>'),
  chevron: svg('<path d="M4 10l4-4 4 4"/>'),
  calendar: svg('<rect x="2.5" y="3.5" width="11" height="10" rx="2"/><path d="M2.5 6.8h11M5.5 2v3M10.5 2v3"/>'),
  bolt: solid('<path d="M9 1.5 3.5 9H7.4L6.6 14.5 12.5 6.8H8.4z"/>'),
};

/** Accu-icoon met gevuld niveau (0–100). */
export function batteryGlyph(percent, charging) {
  const w = Math.max(1, Math.round((percent / 100) * 8.6));
  return `<svg viewBox="0 0 18 16" width="18" height="16" fill="none" stroke="currentColor" stroke-width="1.3" stroke-linejoin="round"><rect x="1.2" y="4" width="13" height="8" rx="2.2"/><path d="M16 6.8v2.4" stroke-linecap="round"/><rect x="3.4" y="6.2" width="${w}" height="3.6" rx="1" fill="currentColor" stroke="none"/>${
    charging ? '<path d="M8.4 2.4 5.6 8.3h2.6l-.9 5.3 3.3-6.4H8z" fill="currentColor" stroke="var(--bg-solid)" stroke-width=".9"/>' : ''
  }</svg>`;
}
