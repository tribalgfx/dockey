import { invoke, listen, emit, applyLook, setAcrylicSupport, trackBackdrop } from './api.js';
import { t, locale, language, setSystemLanguage, errorText } from './i18n.js';
import { glyph } from './glyphs.js';
import { renderTemplate, parseStatus, newId } from './widgets.js';
import { describe, forecast, deg } from './weather.js';
import { CAL_COLORS, icalUrl, loadCalendar, eventsOnDay, upcoming, startOfDay, addDays, fmtTime, fmtWhen } from './agenda.js';

const panel = document.getElementById('panel');
const el = (tag, cls, html) => {
  const node = document.createElement(tag);
  if (cls) node.className = cls;
  if (html != null) node.innerHTML = html;
  return node;
};

const initial = await invoke('get_initial');
let config = initial.config;
const version = initial.version ?? '';
let pendingUpdate = initial.update ?? null; // een nieuwere release, of null
let current = null; // { id, kind, payload }
setAcrylicSupport(initial.acrylic);
setSystemLanguage(initial.systemLanguage);
applyLook(config);
trackBackdrop(panel);

/** Meldt de gemeten grootte; de backend plaatst en toont daarna het venster.
 *  (Geen requestAnimationFrame: die loopt niet zolang het venster verborgen is.)
 *  Levert meten niets op, dan een veilige standaardmaat per soort paneel. */
const FALLBACK = { menu: [268, 320], settings: [372, 640], calendar: [292, 380], update: [340, 320] };
function ready() {
  const [w, h] = FALLBACK[current?.kind] ?? FALLBACK.menu;
  return invoke('popup_ready', { width: (panel.offsetWidth || w) + 24, height: (panel.offsetHeight || h) + 24 });
}
const close = () => invoke('close_popup');

/** Openingsanimatie. Een verborgen venster tekent niet, dus de animatie start
 *  pas als het venster zichtbaar wordt: de backend geeft het dan de focus.
 *  Tot die tijd is het paneel onzichtbaar (.pre). Komt de focus onverhoopt
 *  niet, dan verschijnt het paneel na een halve seconde alsnog, zonder animatie. */
let enterTimer;
function prepareEnter() {
  if (document.hasFocus()) return; // al zichtbaar: alleen de inhoud wisselt
  // Met vervaging niet: het vervaagde vlak erachter beweegt niet mee.
  if (document.documentElement.hasAttribute('data-acrylic')) return;
  panel.classList.remove('enter');
  panel.classList.add('pre');
  clearTimeout(enterTimer);
  enterTimer = setTimeout(() => panel.classList.remove('pre'), 450);
}
window.addEventListener('focus', () => {
  if (!panel.classList.contains('pre')) return;
  clearTimeout(enterTimer);
  panel.classList.remove('pre');
  void panel.offsetWidth; // animatie opnieuw laten beginnen
  panel.classList.add('enter');
});
panel.addEventListener('animationend', () => panel.classList.remove('enter'));

// ------------------------------------------------------------------- menu --

async function run(item) {
  if (item.emit) return emit(item.emit);
  for (const a of item.actions ?? [item.action]) await invoke(a.cmd, a.args).catch(() => {});
  close().catch(() => {});
}

function renderMenu({ title, icon, items }) {
  panel.className = 'menu';
  panel.replaceChildren();
  if (title) {
    const head = el('div', 'menu-title', '<span></span>');
    head.querySelector('span').textContent = title;
    if (icon) {
      const img = el('img');
      img.alt = '';
      img.src = icon;
      head.prepend(img);
    }
    panel.append(head);
  }
  for (const item of items) {
    if (item.sep) {
      panel.append(el('div', 'rule'));
      continue;
    }
    const row = el('div', `item${item.danger ? ' danger' : ''}${item.bold ? ' bold' : ''}${item.active ? ' active' : ''}`);
    const go = el('button', 'go', `${glyph[item.icon] ?? ''}<span></span>`);
    go.querySelector('span').textContent = item.label;
    go.addEventListener('click', () => run(item));
    row.append(go);
    if (item.side) {
      const side = el('button', 'side', glyph[item.side.icon]);
      side.title = item.side.title;
      side.addEventListener('click', async () => {
        await invoke(item.side.action.cmd, item.side.action.args);
        row.remove();
        ready();
      });
      row.append(side);
    }
    panel.append(row);
  }
}

// ----------------------------------------------------------- instellingen --

const THEMES = ['rookglas', 'glas', 'zwart', 'wit', 'middernacht'];
const WIDGETS = ['media', 'weather', 'system', 'network', 'tray', 'volume', 'battery', 'agenda', 'clock'];

async function save(change) {
  change(config);
  await invoke('set_config', { config });
  // De balk kan verschoven zijn (boven/onder, grootte): paneel opnieuw plaatsen.
  setTimeout(ready, 150);
}

function seg(key, options) {
  const box = el('div', 'seg');
  for (const [value, label] of options) {
    const b = el('button', config[key] === value ? 'on' : '');
    b.textContent = label;
    b.addEventListener('click', () => save((c) => (c[key] = value)));
    box.append(b);
  }
  return box;
}

function toggle(get, set) {
  const b = el('button', `toggle${get() ? ' on' : ''}`);
  b.setAttribute('role', 'switch');
  b.setAttribute('aria-checked', String(get()));
  b.addEventListener('click', () => save((c) => set(c, !get())));
  return b;
}

/** Schuifregelaar in procenten; bewaart pas bij loslaten. */
function slider(key, min, max, step) {
  const box = el('div', 'slider');
  const range = el('input');
  range.type = 'range';
  range.min = min;
  range.max = max;
  range.step = step;
  range.value = config[key] ?? 0;
  const value = el('span', 'slider-value');
  const show = () => {
    value.textContent = `${range.value}%`;
    range.style.setProperty('--fill', `${((range.value - min) / (max - min)) * 100}%`);
  };
  show();
  range.addEventListener('input', show);
  range.addEventListener('change', () => save((c) => (c[key] = Number(range.value))));
  box.append(range, value);
  return box;
}

function field(label, control, hint, stack) {
  const row = el('div', `field${stack ? ' stack' : ''}`);
  const text = el('div');
  text.textContent = label;
  if (hint) {
    const h = el('span', 'hint');
    h.textContent = hint;
    text.append(h);
  }
  row.append(text, control);
  return row;
}

function group(title, ...children) {
  const g = el('section', 'group');
  const h = el('h2');
  h.textContent = title;
  const card = el('div', 'card');
  card.append(...children);
  g.append(h, card);
  return g;
}

let placeNote = '';

function orderHint() {
  const row = el('div', 'order-hint');
  const text = el('span');
  text.textContent = t('order.hint');
  row.append(text);
  if (config.widgetOrder?.length) {
    const reset = el('button', 'link');
    reset.textContent = t('order.reset');
    reset.addEventListener('click', () => save((c) => (c.widgetOrder = [])));
    row.append(reset);
  }
  return row;
}

// ----------------------------------------------------- eigen widgets --

let customForm = null; // null | 'status' | 'json'
let customNote = '';
let form = {}; // invoer van het open formulier (blijft staan bij opnieuw tekenen)

const FORM_DEFAULTS = {
  status: { name: '', url: '', interval: 60, openUrl: '', insecure: false },
  json: { name: '', icon: '', url: '', template: '', interval: 60, openUrl: '', insecure: false },
};

function openForm(kind) {
  customForm = kind;
  form = { ...FORM_DEFAULTS[kind] };
  customNote = '';
  renderSettings();
  ready();
  const shown = panel.querySelector('.custom-form');
  shown?.scrollIntoView({ block: 'end' });
  shown?.querySelector('input')?.focus();
}

function input(key, placeholder, type = 'text', target = form) {
  const box = el('input', 'input');
  box.type = type;
  box.placeholder = placeholder;
  box.spellcheck = false;
  box.value = target[key] ?? '';
  box.addEventListener('input', () => (target[key] = type === 'number' ? Number(box.value) : box.value));
  return box;
}

function checkbox(key, label) {
  const row = el('label', 'check');
  const box = el('input');
  box.type = 'checkbox';
  box.checked = !!form[key];
  box.addEventListener('change', () => (form[key] = box.checked));
  row.append(box, label);
  return row;
}

function labeled(text, control) {
  const row = el('label', 'form-row');
  const span = el('span');
  span.textContent = text;
  row.append(span, control);
  return row;
}

async function testAndAdd(note) {
  try {
    if (!form.url.trim()) return void (note.textContent = t('custom.needUrl'));
    const interval = Math.max(15, Number(form.interval) || 60);
    if (customForm === 'status') {
      const url = form.url.trim();
      note.textContent = t('custom.connecting');
      const status = parseStatus(await invoke('fetch_url', { url, insecure: form.insecure }), url);
      customNote = t('status.added', status.text);
      customForm = null;
      await save((c) => c.customWidgets.push({ id: newId(), kind: 'status', name: form.name.trim(), url, template: '', icon: '', openUrl: form.openUrl.trim(), interval, insecure: form.insecure }));
    } else {
      note.textContent = t('custom.fetching');
      const text = renderTemplate(form.template, await invoke('fetch_url', { url: form.url.trim(), insecure: form.insecure }));
      customNote = t('custom.added', text);
      customForm = null;
      await save((c) => c.customWidgets.push({ id: newId(), kind: 'json', name: form.name.trim(), url: form.url.trim(), template: form.template, icon: form.icon.trim(), openUrl: form.openUrl.trim(), interval, insecure: form.insecure }));
    }
  } catch (err) {
    note.textContent = t('failed', errorText(err));
    ready();
  }
}

function customSection() {
  const box = el('div', 'customs');
  for (const cw of config.customWidgets ?? []) {
    const row = el('div', 'custom-row');
    const label = el('div');
    label.textContent = cw.name || (cw.kind === 'status' ? t('status.name') : t('custom.name'));
    const hint = el('span', 'hint');
    hint.textContent = cw.url;
    label.append(hint);
    const del = el('button', 'icon-btn', glyph.close);
    del.title = t('custom.remove', label.firstChild.textContent);
    del.addEventListener('click', () => save((c) => (c.customWidgets = c.customWidgets.filter((x) => x.id !== cw.id))));
    row.append(label, del);
    box.append(row);
  }

  if (!customForm) {
    const actions = el('div', 'actions');
    for (const [kind, text] of [['status', t('status.add')], ['json', t('custom.add')]]) {
      const b = el('button', 'btn', `${glyph.plus}<span></span>`);
      b.querySelector('span').textContent = text;
      b.addEventListener('click', () => openForm(kind));
      actions.append(b);
    }
    const note = el('div', 'note');
    note.textContent = customNote;
    box.append(actions, note);
    return box;
  }

  const f = el('form', 'custom-form');
  if (customForm === 'status') {
    f.append(
      labeled(t('custom.fieldName'), input('name', t('status.namePlaceholder'))),
      labeled(t('custom.fieldUrl'), input('url', t('status.urlPlaceholder'))),
      labeled(t('custom.fieldInterval'), input('interval', '60', 'number')),
      labeled(t('custom.fieldOpen'), input('openUrl', t('status.openPlaceholder'))),
      checkbox('insecure', t('custom.insecure')),
    );
    const hint = el('div', 'note');
    hint.textContent = t('status.hint');
    f.append(hint);
  } else {
    f.append(
      labeled(t('custom.fieldName'), input('name', t('custom.namePlaceholder'))),
      labeled(t('custom.fieldIcon'), input('icon', t('custom.iconPlaceholder'))),
      labeled(t('custom.fieldUrl'), input('url', t('custom.urlPlaceholder'))),
      labeled(t('custom.fieldTemplate'), input('template', t('custom.templatePlaceholder'))),
      labeled(t('custom.fieldInterval'), input('interval', '60', 'number')),
      labeled(t('custom.fieldOpen'), input('openUrl', t('custom.openPlaceholder'))),
      checkbox('insecure', t('custom.insecure')),
    );
  }
  const note = el('div', 'note');
  const buttons = el('div', 'actions');
  const cancel = el('button', 'btn');
  cancel.type = 'button';
  cancel.textContent = t('cancel');
  cancel.addEventListener('click', () => {
    customForm = null;
    renderSettings();
    ready();
  });
  const add = el('button', 'btn primary');
  add.textContent = t('custom.testAdd');
  buttons.append(cancel, add);
  f.append(buttons, note);
  f.addEventListener('submit', (e) => {
    e.preventDefault();
    testAndAdd(note);
  });
  box.append(f);
  return box;
}

// ----------------------------------------------------------- agenda's --

let calForm = null; // { name, url, color } zolang het formulier openstaat
let calNote = '';

const hostOf = (url) => {
  try {
    return new URL(icalUrl(url)).host;
  } catch {
    return '';
  }
};

const HELP = [['Google', 'cal.help.google'], ['Outlook.com', 'cal.help.outlook'], ['Proton', 'cal.help.proton'], ['iCloud', 'cal.help.icloud']];

function calendarSection() {
  const box = el('div', 'customs');
  for (const cal of config.calendars ?? []) {
    const row = el('div', 'custom-row');
    const label = el('div', 'cal-label');
    const dot = el('i', 'cal-dot');
    dot.style.background = cal.color;
    const text = el('div');
    text.textContent = cal.name;
    const hint = el('span', 'hint');
    hint.textContent = hostOf(cal.url); // de link zelf is geheim: niet tonen
    text.append(hint);
    label.append(dot, text);
    const del = el('button', 'icon-btn', glyph.close);
    del.title = t('cal.unlink', cal.name);
    del.addEventListener('click', () => save((c) => (c.calendars = c.calendars.filter((x) => x.id !== cal.id))));
    row.append(label, del);
    box.append(row);
  }

  if (!calForm) {
    const actions = el('div', 'actions');
    const add = el('button', 'btn', `${glyph.plus}<span></span>`);
    add.querySelector('span').textContent = t('cal.link');
    add.addEventListener('click', () => {
      calForm = { name: '', url: '', color: CAL_COLORS[(config.calendars ?? []).length % CAL_COLORS.length] };
      calNote = '';
      renderSettings();
      ready();
      const shown = panel.querySelector('.cal-form');
      shown?.scrollIntoView({ block: 'end' });
      shown?.querySelector('input')?.focus();
    });
    actions.append(add);
    const note = el('div', 'note');
    note.textContent = calNote;
    box.append(actions, note);
    return box;
  }

  const f = el('form', 'custom-form cal-form');
  const colors = el('div', 'cal-colors');
  for (const color of CAL_COLORS) {
    const b = el('button', `cal-color${calForm.color === color ? ' on' : ''}`);
    b.type = 'button';
    b.style.background = color;
    b.title = t('cal.color');
    b.addEventListener('click', () => {
      calForm.color = color;
      colors.querySelectorAll('.cal-color').forEach((x) => x.classList.toggle('on', x === b));
    });
    colors.append(b);
  }
  const help = el('details', 'cal-help', '<summary></summary>');
  help.querySelector('summary').textContent = t('cal.where');
  for (const [service, key] of HELP) {
    const p = el('p');
    const strong = el('strong');
    strong.textContent = `${service}: `;
    p.append(strong, t(key));
    help.append(p);
  }
  const secret = el('p');
  secret.textContent = t('cal.secret');
  help.append(secret);
  help.addEventListener('toggle', () => ready());

  f.append(
    labeled(t('custom.fieldName'), input('name', t('cal.namePlaceholder'), 'text', calForm)),
    labeled(t('cal.fieldUrl'), input('url', t('cal.urlPlaceholder'), 'text', calForm)),
    labeled(t('cal.color'), colors),
    help,
  );
  const note = el('div', 'note');
  const buttons = el('div', 'actions');
  const cancel = el('button', 'btn');
  cancel.type = 'button';
  cancel.textContent = t('cancel');
  cancel.addEventListener('click', () => {
    calForm = null;
    renderSettings();
    ready();
  });
  const add = el('button', 'btn primary');
  add.textContent = t('cal.testLink');
  buttons.append(cancel, add);
  f.append(buttons, note);
  f.addEventListener('submit', async (e) => {
    e.preventDefault();
    const url = calForm.url.trim();
    if (!url) return void (note.textContent = t('cal.needUrl'));
    const name = calForm.name.trim() || hostOf(url).replace(/^www\./, '') || t('cal.defaultName');
    note.textContent = t('cal.fetching');
    try {
      const events = await loadCalendar(invoke, { name, url, color: calForm.color });
      const soon = events.filter((x) => x.end > Date.now() && x.start < addDays(Date.now(), 30)).length;
      calNote = t('cal.linked', name, soon);
      const cal = { id: newId(), name, url, color: calForm.color };
      calForm = null;
      await save((c) => (c.calendars = [...(c.calendars ?? []), cal]));
    } catch (err) {
      note.textContent = t('failed', errorText(err));
      ready();
    }
  });
  box.append(f);
  return box;
}

function renderSettings() {
  const scroll = panel.querySelector('.settings-body')?.scrollTop ?? 0;
  panel.className = 'settings';
  panel.replaceChildren();

  const head = el('div', 'settings-head', '<h1></h1>');
  head.querySelector('h1').textContent = t('settings.title');
  const x = el('button', 'icon-btn', glyph.close);
  x.title = t('settings.close');
  x.addEventListener('click', close);
  head.append(x);

  const swatches = el('div', 'swatches');
  for (const value of THEMES) {
    const b = el('button', `swatch${config.theme === value ? ' on' : ''}`, `<i class="${value}"><b></b></i>`);
    b.append(t(`theme.${value}`));
    b.addEventListener('click', () => save((c) => (c.theme = value)));
    swatches.append(b);
  }

  const checks = el('div', 'checks');
  for (const key of WIDGETS) {
    const on = !!config.widgets[key];
    const b = el('button', `check-item${on ? ' on' : ''}`, `<i>${glyph.check}</i><span></span>`);
    b.setAttribute('role', 'checkbox');
    b.setAttribute('aria-checked', String(on));
    b.querySelector('span').textContent = t(`widget.${key}`);
    b.addEventListener('click', () => save((c) => (c.widgets[key] = !c.widgets[key])));
    checks.append(b);
  }

  const place = el('form', 'place', '<input type="text" spellcheck="false"><button class="btn"></button>');
  const input = place.querySelector('input');
  input.placeholder = t('place.placeholder');
  place.querySelector('button').textContent = t('place.search');
  input.value = config.weather.place;
  const note = el('div', 'note');
  note.textContent = placeNote;
  place.addEventListener('submit', async (e) => {
    e.preventDefault();
    const name = input.value.trim();
    if (!name) return;
    note.textContent = t('place.searching');
    try {
      const url = `https://geocoding-api.open-meteo.com/v1/search?count=1&language=${language()}&name=${encodeURIComponent(name)}`;
      const hit = (await (await fetch(url)).json()).results?.[0];
      if (!hit) return void (note.textContent = t('place.notFound', name));
      placeNote = t('place.found', [hit.name, hit.admin1, hit.country].filter(Boolean).join(', '));
      save((c) => {
        c.weather = { place: hit.name, lat: hit.latitude, lon: hit.longitude };
        c.widgets.weather = true;
      });
    } catch {
      note.textContent = t('place.failed');
    }
  });

  const placeRow = el('div', 'place-row');
  placeRow.append(place, note);

  const body = el('div', 'settings-body');
  body.addEventListener('scroll', () => body.classList.toggle('scrolled', body.scrollTop > 0));
  body.append(
    group(
      t('group.look'),
      swatches,
      field(t('transparency.label'), slider('transparency', 0, 80, 5), t('transparency.hint'), true),
      field(t('lang.label'), seg('language', [['auto', t('lang.auto')], ['nl', 'Nederlands'], ['en', 'English']])),
    ),
    group(
      t('group.place'),
      field(t('pos.label'), seg('position', ['bottom', 'top', 'left', 'right'].map((v) => [v, t(`pos.${v}`)]))),
      field(t('layout.label'), seg('layout', ['dock', 'panel', 'bar'].map((v) => [v, t(`layout.${v}`)]))),
      field(t('size.label'), seg('size', ['s', 'm', 'l'].map((v) => [v, t(`size.${v}`)]))),
      field(
        t('expand.label'),
        toggle(() => config.expandOnMaximize, (c, v) => (c.expandOnMaximize = v)),
        t('expand.hint'),
      ),
      field(t('monitors.label'), toggle(() => config.allMonitors, (c, v) => (c.allMonitors = v)), t('monitors.hint')),
      field(
        t('perMonitor.label'),
        toggle(() => config.appsPerMonitor !== false, (c, v) => (c.appsPerMonitor = v)),
        t('perMonitor.hint'),
      ),
    ),
    group(t('group.widgets'), checks, placeRow, orderHint()),
    group(t('group.custom'), customSection()),
    group(t('group.calendars'), calendarSection()),
    group(
      t('group.clock'),
      field(t('clock.24h'), toggle(() => config.clock24, (c, v) => (c.clock24 = v))),
      field(t('clock.seconds'), toggle(() => config.showSeconds, (c, v) => (c.showSeconds = v))),
    ),
    group(
      t('group.windows'),
      field(t('taskbar.label'), seg('taskbar', ['hidden', 'autohide', 'none'].map((v) => [v, t(`taskbar.${v}`)])), t('taskbar.hint'), true),
      field(t('reserve.label'), toggle(() => config.reserveSpace, (c, v) => (c.reserveSpace = v)), t('reserve.hint')),
      field(t('previews.label'), toggle(() => config.previews !== false, (c, v) => (c.previews = v)), t('previews.hint')),
      field(t('autostart.label'), toggle(() => config.autostart, (c, v) => (c.autostart = v))),
    ),
    group(
      t('group.updates'),
      field(t('update.auto'), toggle(() => config.updateCheck, (c, v) => (c.updateCheck = v)), t('update.autoHint')),
      updateCheckRow(),
    ),
  );

  const foot = el('div', 'settings-foot', '<span></span>');
  foot.querySelector('span').textContent = `Dockey ${version}`;
  const quit = el('button', 'btn', `${glyph.power}<span></span>`);
  quit.querySelector('span').textContent = t('settings.quit');
  quit.addEventListener('click', () => invoke('quit'));
  foot.append(quit);

  panel.append(head, body, foot);
  body.scrollTop = scroll;
  body.classList.toggle('scrolled', scroll > 0);
}

// --------------------------------------------------------------- kalender --

let calOffset = 0; // maanden vanaf nu
let selectedDay = startOfDay(Date.now());
let agenda = { events: [], errors: [], calendars: [] };

function eventRow(e, day, now) {
  const row = el('div', 'ev');
  const dot = el('i', 'cal-dot');
  dot.style.background = e.color;
  const when = el('span', 'ev-time');
  if (day == null) when.textContent = fmtWhen(e, now);
  else if (e.allDay || (e.start <= day && e.end >= addDays(day, 1))) when.textContent = t('cal.allDay');
  else when.textContent = `${e.start < day ? '…' : fmtTime(e.start)}–${e.end > addDays(day, 1) ? '…' : fmtTime(e.end)}`;
  const text = el('div', 'ev-text');
  const title = el('span', 'ev-title');
  title.textContent = e.title;
  text.append(title);
  if (e.location) {
    const loc = el('span', 'ev-loc');
    loc.textContent = e.location;
    text.append(loc);
  }
  row.title = [e.title, e.location, e.calendar].filter(Boolean).join(' · ');
  row.append(dot, when, text);
  return row;
}

function agendaList() {
  const box = el('div', 'ag-list');
  if (!agenda.calendars.length) {
    const link = el('button', 'link');
    link.textContent = t('cal.link');
    link.addEventListener('click', () => emit('open-settings'));
    const p = el('div', 'note');
    p.append(t('cal.none'), link);
    box.append(p);
    return box;
  }
  const now = Date.now();
  const today = startOfDay(now);
  const head = el('div', 'ag-head');
  head.textContent = selectedDay === today ? t('cal.today') : new Date(selectedDay).toLocaleDateString(locale(), { weekday: 'long', day: 'numeric', month: 'long' });
  box.append(head);
  const dayEvents = eventsOnDay(agenda.events, selectedDay);
  if (dayEvents.length) dayEvents.forEach((e) => box.append(eventRow(e, selectedDay, now)));
  else {
    const none = el('div', 'note');
    none.textContent = t('cal.noEvents');
    box.append(none);
  }
  if (selectedDay === today) {
    const later = upcoming(agenda.events, now, 8, 12).filter((e) => e.start >= addDays(today, 1)).slice(0, 5);
    if (later.length) {
      const h = el('div', 'ag-head');
      h.textContent = t('cal.soon');
      box.append(h);
      later.forEach((e) => box.append(eventRow(e, null, now)));
    }
  }
  for (const err of agenda.errors) {
    const p = el('div', 'note warn');
    p.textContent = t('agenda.unreachableItem', err);
    box.append(p);
  }
  return box;
}

function isoWeek(date) {
  const d = new Date(Date.UTC(date.getFullYear(), date.getMonth(), date.getDate()));
  d.setUTCDate(d.getUTCDate() + 4 - (d.getUTCDay() || 7));
  return Math.ceil(((d - Date.UTC(d.getUTCFullYear(), 0, 1)) / 86400000 + 1) / 7);
}

/** Korte namen van maandag t/m zondag: "ma" in het Nederlands, "Mo" in het Engels. */
function weekdayLabels() {
  // 1 januari 2024 was een maandag.
  return Array.from({ length: 7 }, (_, i) => {
    const name = new Date(2024, 0, 1 + i).toLocaleDateString(locale(), { weekday: 'short' }).replace('.', '');
    return language() === 'nl' ? name.slice(0, 2).toLowerCase() : name.slice(0, 2);
  });
}

function renderCalendar() {
  const today = new Date();
  const first = new Date(today.getFullYear(), today.getMonth() + calOffset, 1);
  panel.className = 'calendar';
  panel.replaceChildren();

  const head = el('div', 'cal-head');
  const title = el('strong');
  title.textContent = first.toLocaleDateString(locale(), { month: 'long', year: 'numeric' });
  const nav = el('div');
  for (const [icon, label, step] of [['left', t('cal.prevMonth'), -1], ['right', t('cal.nextMonth'), 1]]) {
    const b = el('button', 'icon-btn', glyph[icon]);
    b.title = label;
    b.addEventListener('click', () => {
      calOffset += step;
      renderCalendar();
    });
    nav.append(b);
  }
  head.append(title, nav);

  const line = el('div', 'cal-today');
  line.textContent = today.toLocaleDateString(locale(), { weekday: 'long', day: 'numeric', month: 'long' });

  const grid = el('div', 'cal-grid');
  for (const d of ['', ...weekdayLabels()]) {
    const s = el('span', 'dow');
    s.textContent = d;
    grid.append(s);
  }
  // Begin op de maandag van de week waarin de 1e valt; altijd 6 weken.
  const cursor = new Date(first);
  cursor.setDate(1 - ((first.getDay() + 6) % 7));
  for (let week = 0; week < 6; week++) {
    const wk = el('span', 'wk');
    wk.textContent = isoWeek(cursor);
    grid.append(wk);
    for (let day = 0; day < 7; day++) {
      const out = cursor.getMonth() !== first.getMonth();
      const isToday = cursor.toDateString() === today.toDateString();
      const dayStart = startOfDay(cursor.getTime());
      const s = el('button', `day${isToday ? ' today' : out ? ' out' : ''}${dayStart === selectedDay ? ' sel' : ''}`);
      s.textContent = cursor.getDate();
      const onDay = eventsOnDay(agenda.events, dayStart);
      if (onDay.length) {
        s.classList.add('has');
        s.style.setProperty('--c', onDay[0].color);
        s.title = onDay.map((e) => e.title).join('\n');
      }
      s.addEventListener('click', () => {
        selectedDay = dayStart;
        renderCalendar();
        ready();
      });
      grid.append(s);
      cursor.setDate(cursor.getDate() + 1);
    }
  }

  const foot = el('div', 'cal-foot');
  const bell = el('button', 'btn', `${glyph.bell}<span></span>`);
  bell.querySelector('span').textContent = t('cal.notifications');
  bell.addEventListener('click', async () => {
    await close();
    invoke('send_keys', { name: 'notifications' });
  });
  foot.append(bell);

  panel.append(head, line, grid, agendaList(), foot);
}

// ---------------------------------------------------------- systeemvak --

let trayItems = [];
let trayEdit = false;
let shown = false; // staat het paneel nu open?

function renderTray() {
  panel.className = 'tray';
  panel.replaceChildren();
  const pinned = config.trayPinned ?? [];
  const items = trayEdit ? trayItems : trayItems.filter((i) => !pinned.includes(i.key));

  if (trayEdit) {
    const hint = el('div', 'note');
    hint.textContent = t('tray.editHint');
    panel.append(hint);
  }
  const grid = el('div', 'tray-grid');
  for (const item of items) {
    const b = el('button', pinned.includes(item.key) && trayEdit ? 'pinned' : '');
    b.title = item.tip;
    const img = el('img');
    img.alt = '';
    img.src = item.icon;
    b.append(img);
    const send = (right) => {
      const r = b.getBoundingClientRect();
      invoke('tray_click', { key: item.key, right, x: r.left + r.width / 2, y: r.top + r.height / 2 });
    };
    b.addEventListener('click', () => {
      if (!trayEdit) return send(false);
      save((c) => {
        const list = c.trayPinned ?? [];
        c.trayPinned = list.includes(item.key) ? list.filter((k) => k !== item.key) : [...list, item.key];
      });
    });
    b.addEventListener('contextmenu', (e) => {
      e.preventDefault();
      if (!trayEdit) send(true);
    });
    grid.append(b);
  }
  if (!items.length) {
    const none = el('div', 'note');
    none.textContent = trayItems.length ? t('tray.allPinned') : t('tray.none');
    panel.append(none);
  } else panel.append(grid);

  const foot = el('div', 'tray-foot');
  const toggle = el('button', 'link');
  toggle.textContent = trayEdit ? t('tray.done') : t('tray.choose');
  toggle.addEventListener('click', () => {
    trayEdit = !trayEdit;
    renderTray();
    ready();
  });
  foot.append(toggle);
  panel.append(foot);
}

listen('tray', (items) => {
  trayItems = items;
  if (shown && current?.kind === 'tray') {
    renderTray();
    ready();
  }
});
listen('popup:closed', () => {
  shown = false;
  trayEdit = false;
});

// ---------------------------------------------------------------- weer --

let wxCache = null; // { at, data } — hooguit eens per 10 minuten opnieuw ophalen

async function renderWeather() {
  panel.className = 'weather';
  const place = config.weather.place;
  if (!wxCache || Date.now() - wxCache.at > 10 * 60 * 1000 || wxCache.place !== place) {
    if (!wxCache || wxCache.place !== place) {
      panel.replaceChildren(el('div', 'note'));
      panel.firstChild.textContent = t('wx.fetching');
      ready();
    }
    try {
      wxCache = { at: Date.now(), place, data: await forecast(config.weather) };
    } catch {
      panel.replaceChildren(el('div', 'note'));
      panel.firstChild.textContent = t('wx.failed');
      return void ready();
    }
  }
  if (current?.kind !== 'weather') return;
  const { current: now, hourly, daily } = wxCache.data;
  const d = describe(now.weather_code, now.is_day);
  panel.replaceChildren();

  const head = el('div', 'wx-head');
  const name = el('strong');
  name.textContent = place;
  const time = el('span');
  time.textContent = t('wx.updated', new Date(wxCache.at).toLocaleTimeString(locale(), { hour: '2-digit', minute: '2-digit' }));
  head.append(name, time);

  const nowBox = el('div', 'wx-now', `<span class="big">${d.emoji}</span><span class="temp">${deg(now.temperature_2m)}</span><div class="what"><span></span><span class="wx-meta"></span></div>`);
  nowBox.querySelector('.what span').textContent = d.text;
  nowBox.querySelector('.wx-meta').textContent = t('wx.meta', deg(now.apparent_temperature), Math.round(now.wind_speed_10m), now.relative_humidity_2m);

  // De komende acht uur, vanaf het eerstvolgende hele uur.
  const hours = el('div', 'wx-hours');
  const from = hourly.time.findIndex((t) => new Date(t).getTime() > Date.now() - 30 * 60 * 1000);
  for (let i = Math.max(0, from); i < Math.min(hourly.time.length, from + 8); i++) {
    const h = describe(hourly.weather_code[i], hourly.is_day[i]);
    const rain = hourly.precipitation_probability[i];
    const cell = el('div', '', `<span class="h">${t('wx.hour', hourly.time[i].slice(11, 13))}</span><span class="e">${h.emoji}</span><span>${deg(hourly.temperature_2m[i])}</span><span class="r">${rain >= 20 ? `${rain}%` : '&nbsp;'}</span>`);
    cell.title = `${h.text}${rain ? `, ${t('wx.rainChance', rain)}` : ''}`;
    hours.append(cell);
  }

  const days = el('div', 'wx-days');
  daily.time.forEach((iso, i) => {
    const day = describe(daily.weather_code[i], true);
    const rain = daily.precipitation_probability_max[i];
    const label = i === 0 ? t('day.today') : i === 1 ? t('day.tomorrow') : new Date(iso).toLocaleDateString(locale(), { weekday: 'long' });
    const row = el('div', 'wx-day', `<span class="d"></span><span class="e">${day.emoji}</span><span class="r">${rain >= 20 ? `💧 ${rain}%` : ''}</span><span class="t">${deg(daily.temperature_2m_max[i])} <span>/ ${deg(daily.temperature_2m_min[i])}</span></span>`);
    row.querySelector('.d').textContent = label;
    row.title = day.text;
    days.append(row);
  });

  const sun = el('div', 'wx-sun');
  const clock = (iso) => iso.slice(11, 16);
  sun.textContent = t('wx.sun', clock(daily.sunrise[0]), clock(daily.sunset[0]));

  panel.append(head, nowBox, hours, days, sun);
  ready();
}

// -------------------------------------------------------------- updates --

let updateNote = '';
let updateBusy = false;
let checkNote = '';

/** Release-tekst (Markdown) als gewone tekst. */
const plainNotes = (text) =>
  text
    .replace(/```[\s\S]*?```/g, '')
    .replace(/^#+\s*/gm, '')
    .replace(/^>\s?/gm, '')
    .replace(/\*\*/g, '')
    .replace(/`/g, '')
    .replace(/\[([^\]]+)\]\([^)]+\)/g, '$1')
    .replace(/\n{3,}/g, '\n\n')
    .trim();

function renderUpdate() {
  panel.className = 'update';
  panel.replaceChildren();
  const u = pendingUpdate;
  const head = el('div', 'upd-head');
  const title = el('strong');
  title.textContent = u ? t('update.title', u.version) : t('update.none');
  const now = el('span');
  now.textContent = t('update.current', version);
  head.append(title, now);
  panel.append(head);
  if (!u) return;

  if (u.notes) {
    const notes = el('div', 'upd-notes');
    notes.textContent = plainNotes(u.notes);
    panel.append(notes);
  }
  const actions = el('div', 'actions');
  const go = el('button', 'btn primary');
  go.textContent = t('update.install');
  go.disabled = updateBusy;
  go.addEventListener('click', async () => {
    updateBusy = true;
    updateNote = t('update.downloading');
    renderUpdate();
    ready();
    try {
      await invoke('install_update');
      updateNote = t('update.started');
    } catch (err) {
      updateNote = t('failed', errorText(err));
      updateBusy = false;
    }
    if (current?.kind === 'update') {
      renderUpdate();
      ready();
    }
  });
  const page = el('button', 'btn');
  page.textContent = t('update.page');
  page.addEventListener('click', () => invoke('launch', { target: u.page }));
  actions.append(go, page);
  const note = el('div', 'note');
  note.textContent = updateNote;
  panel.append(actions, note);
}

/** "Nu controleren" in de instellingen, met de uitkomst ernaast. */
function updateCheckRow() {
  const row = el('div', 'actions');
  const check = el('button', 'btn');
  check.textContent = t('update.check');
  const note = el('div', 'note');
  note.textContent = checkNote;
  check.addEventListener('click', async () => {
    note.textContent = t('update.checking');
    try {
      const result = await invoke('check_update');
      checkNote = result.update ? t('update.found', result.update.version) : t('update.latest', result.current);
    } catch (err) {
      checkNote = t('failed', errorText(err));
    }
    if (current?.kind === 'settings') {
      renderSettings();
      ready();
    }
  });
  row.append(check);
  if (pendingUpdate) {
    const show = el('button', 'btn primary');
    show.textContent = t('update.show');
    show.addEventListener('click', () => emit('open-update'));
    row.append(show);
  }
  const box = el('div');
  box.append(row, note);
  return box;
}

listen('update', (update) => {
  pendingUpdate = update;
  if (current?.kind === 'update' || current?.kind === 'settings') {
    render();
    ready();
  }
});

// ------------------------------------------------------------------ start --

function render() {
  if (!current) return;
  if (current.kind === 'menu') renderMenu(current.payload);
  else if (current.kind === 'settings') renderSettings();
  else if (current.kind === 'calendar') renderCalendar();
  else if (current.kind === 'weather') return void renderWeather();
  else if (current.kind === 'tray') renderTray();
  else if (current.kind === 'update') renderUpdate();
}

listen('popup:render', async (request) => {
  if (request.kind === 'calendar' && current?.kind !== 'calendar') calOffset = 0;
  if (request.kind === 'settings' && current?.kind !== 'settings') {
    placeNote = '';
    customNote = '';
    customForm = null;
    calNote = '';
    calForm = null;
  }
  if (request.kind === 'calendar' && current?.kind !== 'calendar') selectedDay = startOfDay(Date.now());
  current = request;
  shown = true;
  if (request.kind === 'tray') trayItems = await invoke('tray_icons');
  document.documentElement.style.setProperty('--avail', `${request.maxHeight}px`);
  render();
  prepareEnter();
  if (request.kind !== 'weather') ready(); // het weerpaneel meldt zich zelf zodra het weer er is
});

listen('agenda', (data) => {
  agenda = data;
});
emit('agenda:request');

listen('config', (next) => {
  config = next;
  applyLook(next);
  if (current?.kind === 'settings') renderSettings();
});

document.addEventListener('keydown', (e) => {
  if (e.key === 'Escape') close();
});
document.addEventListener('contextmenu', (e) => e.preventDefault());
