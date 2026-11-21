// Gekoppelde agenda's: ophalen, omzetten en wat er vandaag en binnenkort speelt.
// Gedeeld door de balk (widget) en het paneel (kalender, instellingen).
import { parseICal } from './ical.js';
import { t, locale } from './i18n.js';

export const CAL_COLORS = ['#5b9cff', '#3ecf8e', '#ff8a5c', '#c58cff', '#ffcc4d', '#ff6b9a'];

/** webcal:// (iCloud, Outlook) is gewoon https. */
export const icalUrl = (url) => url.trim().replace(/^webcal:\/\//i, 'https://');

export function startOfDay(ts) {
  const d = new Date(ts);
  d.setHours(0, 0, 0, 0);
  return d.getTime();
}

export function addDays(ts, n) {
  const d = new Date(ts);
  d.setDate(d.getDate() + n);
  return d.getTime();
}

// Hele-dagafspraken komen binnen als UTC-middernacht: omzetten naar middernacht hier.
const localDay = (ts) => {
  const d = new Date(ts);
  return new Date(d.getUTCFullYear(), d.getUTCMonth(), d.getUTCDate()).getTime();
};

export function toEvents(text, cal) {
  return parseICal(text).map((e) => {
    const start = e.allDay ? localDay(e.start) : e.start;
    const end = e.allDay ? (e.end ? localDay(e.end) : addDays(start, 1)) : e.end ?? e.start;
    return { title: e.title, location: e.location, start, end: Math.max(end, start), allDay: e.allDay, color: cal.color, calendar: cal.name };
  });
}

export async function loadCalendar(invoke, cal) {
  const text = await invoke('fetch_url', { url: icalUrl(cal.url), insecure: false });
  if (!/BEGIN:VCALENDAR/i.test(text)) throw new Error(t('cal.notIcal'));
  return toEvents(text, cal);
}

/** Afspraken die (deels) op deze dag vallen; `day` is middernacht. */
export function eventsOnDay(events, day) {
  const end = addDays(day, 1);
  return events.filter((e) => e.start < end && Math.max(e.end, e.start + 1) > day);
}

export const fmtTime = (ts) => new Date(ts).toLocaleTimeString(locale(), { hour: '2-digit', minute: '2-digit' });

export function relDay(ts, now) {
  const day = startOfDay(ts);
  const today = startOfDay(now);
  if (day === today) return t('day.today');
  if (day === addDays(today, 1)) return t('day.tomorrow');
  return new Date(ts).toLocaleDateString(locale(), { weekday: 'short', day: 'numeric', month: 'short' });
}

export const fmtWhen = (e, now) => (e.allDay ? relDay(e.start, now) : `${relDay(e.start, now)} ${fmtTime(e.start)}`);

/** De eerstvolgende afspraken (ook wat nu bezig is) in de komende dagen. */
export function upcoming(events, now, days = 7, max = 6) {
  const until = addDays(startOfDay(now), days);
  return events.filter((e) => e.end > now && e.start < until).slice(0, max);
}
