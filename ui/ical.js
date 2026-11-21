// iCal-lezer (VEVENT → { title, location, start, end, allDay }). Herhalingen worden uitgevouwen binnen een
// venster van een jaar terug tot twee jaar vooruit: RRULE met BYDAY, BYMONTHDAY, BYMONTH,
// BYSETPOS, INTERVAL, COUNT en UNTIL, plus RDATE, EXDATE en losse wijzigingen in een reeks
// (RECURRENCE-ID). Tijden zonder zone gelden in de tijdzone van deze pc.
import { t } from "./i18n.js";

const DAY = 86_400_000;
// Voor tijden zonder zone en voor zones die Intl niet kent (Outlook: "W. Europe Standard Time"):
// de tijdzone van deze pc.
const HOME_ZONE = (() => {
  try {
    return Intl.DateTimeFormat().resolvedOptions().timeZone || "UTC";
  } catch {
    return "UTC";
  }
})();

const zoneFormats = new Map();

function zoneFormat(zone) {
  if (zoneFormats.has(zone)) return zoneFormats.get(zone);
  let format;
  try {
    format = new Intl.DateTimeFormat("en-US", { timeZone: zone, hourCycle: "h23", year: "numeric", month: "numeric", day: "numeric", hour: "numeric", minute: "numeric", second: "numeric" });
  } catch {
    format = zoneFormat(HOME_ZONE);
  }
  if (zoneFormats.size > 200) zoneFormats.clear(); // zonenamen komen uit de feed: niet eindeloos onthouden
  zoneFormats.set(zone, format);
  return format;
}

// Hoeveel de klok in een zone op dat moment voorloopt op UTC.
function zoneOffset(ts, zone) {
  if (zone === "UTC") return 0;
  const parts = Object.fromEntries(zoneFormat(zone).formatToParts(ts).map((p) => [p.type, Number(p.value)]));
  return Date.UTC(parts.year, parts.month - 1, parts.day, parts.hour, parts.minute, parts.second) - Math.floor(ts / 1000) * 1000;
}

// Van een kloktijd (als UTC genoteerd) in een zone naar het echte tijdstip.
function zonedTime(wall, zone) {
  return wall - zoneOffset(wall - zoneOffset(wall, zone), zone);
}

export function parseICalDate(raw, zone = HOME_ZONE) {
  const s = raw?.trim();
  if (!s) return null;
  const y = +s.slice(0, 4), mo = +s.slice(4, 6) - 1, d = +s.slice(6, 8);
  if (s.length === 8) return Number.isFinite(y + mo + d) ? { ts: Date.UTC(y, mo, d), allDay: true } : null;
  const wall = Date.UTC(y, mo, d, +s.slice(9, 11), +s.slice(11, 13), +s.slice(13, 15) || 0);
  if (!Number.isFinite(wall)) return null;
  return { ts: s.endsWith("Z") ? wall : zonedTime(wall, zone), allDay: false };
}

const WEEKDAYS = ["SU", "MO", "TU", "WE", "TH", "FR", "SA"];
const RULE_FREQS = ["DAILY", "WEEKLY", "MONTHLY", "YEARLY"];
const RULE_UNSUPPORTED = ["BYHOUR", "BYMINUTE", "BYSECOND", "BYWEEKNO", "BYYEARDAY"];
const RULE_MAX_STEPS = 20000; // vangnet; normaal stopt de lus bij UNTIL, COUNT of het venster
// Zo lang mag het uitvouwen van één agenda duren. Het rekenwerk houdt de balk op, en de agenda
// komt van buiten: een feed met duizenden zware regels mag hem niet lang vastzetten.
// Daarna houden de resterende reeksen alleen hun eerste afspraak.
const EXPAND_BUDGET_MS = 1500;

const numberList = (text) => (text ? text.split(",").map(Number).filter(Number.isFinite) : null);

function parseRule(text) {
  const parts = Object.fromEntries(text.split(";").map((part) => part.split("=")));
  if (!RULE_FREQS.includes(parts.FREQ) || RULE_UNSUPPORTED.some((key) => key in parts)) return null;
  const byDay = (parts.BYDAY ?? "").split(",").filter(Boolean).map((item) => {
    const [, nth, day] = item.match(/^([+-]?\d+)?([A-Z]{2})$/) ?? [];
    return { nth: nth ? Number(nth) : null, day: WEEKDAYS.indexOf(day) };
  });
  if (byDay.some((b) => b.day < 0)) return null;
  // Een heel getal vanaf 1. Met -1 loopt de reeks achteruit en haalt hij het venster nooit: dan
  // rekent de lus tot het vangnet, seconden lang per afspraak.
  const interval = parts.INTERVAL === undefined ? 1 : Number(parts.INTERVAL);
  if (!Number.isInteger(interval) || interval < 1) return null;
  return {
    freq: parts.FREQ,
    interval,
    count: parts.COUNT ? Number(parts.COUNT) : Infinity,
    until: parts.UNTIL ?? null,
    weekStart: Math.max(0, WEEKDAYS.indexOf(parts.WKST ?? "MO")),
    byDay,
    byMonth: numberList(parts.BYMONTH)?.map((month) => month - 1) ?? null,
    byMonthDay: numberList(parts.BYMONTHDAY),
    bySetPos: numberList(parts.BYSETPOS)
  };
}

// Kalenderdagen als dagnummers (dagen sinds 1970): daar valt geen zomertijd op te tellen.
const dayNumber = (y, mo, d) => Date.UTC(y, mo, d) / DAY;
const weekdayOf = (day) => new Date(day * DAY).getUTCDay();
const isByDay = (rule, day) => rule.byDay.some((b) => b.day === weekdayOf(day));

// De dagen uit een reeks die bij BYDAY passen; "2TU" telt binnen die reeks, "-1FR" van achteren.
function matchingWeekdays(firstDay, length, byDay) {
  const days = [];
  for (const { day, nth } of byDay) {
    const matches = [];
    for (let i = 0; i < length; i++) if (weekdayOf(firstDay + i) === day) matches.push(firstDay + i);
    if (nth === null) days.push(...matches);
    else days.push(matches.at(nth > 0 ? nth - 1 : nth));
  }
  return days.filter((day) => day !== undefined);
}

function monthDays(y, mo, rule, first) {
  const firstDay = dayNumber(y, mo, 1);
  const length = dayNumber(y, mo + 1, 1) - firstDay;
  if (rule.byMonthDay) {
    const days = rule.byMonthDay.map((n) => (n > 0 ? n : length + 1 + n)).filter((n) => n >= 1 && n <= length).map((n) => firstDay + n - 1);
    return rule.byDay.length ? days.filter((day) => isByDay(rule, day)) : days;
  }
  if (rule.byDay.length) return matchingWeekdays(firstDay, length, rule.byDay);
  // 31 april of 29 februari in een gewoon jaar bestaat niet: die slaat de iCal-standaard over.
  return first.date <= length ? [firstDay + first.date - 1] : [];
}

function yearDays(y, rule, first) {
  if (rule.byDay.length && !rule.byMonth && !rule.byMonthDay) {
    return matchingWeekdays(dayNumber(y, 0, 1), dayNumber(y + 1, 0, 1) - dayNumber(y, 0, 1), rule.byDay);
  }
  const allMonths = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11];
  const months = rule.byMonth ?? (rule.byMonthDay ? allMonths : [first.month]);
  return months.flatMap((mo) => monthDays(y, mo, rule, first));
}

// Periode nummer `step` van de regel: waar die begint en welke dagen erin vallen, op volgorde.
function rulePeriod(rule, first, step) {
  const n = step * rule.interval;
  let begin, days;
  if (rule.freq === "DAILY") {
    begin = first.day + n;
    const date = new Date(begin * DAY);
    const monthDay = date.getUTCDate();
    const length = dayNumber(date.getUTCFullYear(), date.getUTCMonth() + 1, 1) - dayNumber(date.getUTCFullYear(), date.getUTCMonth(), 1);
    const fits = (!rule.byDay.length || isByDay(rule, begin))
      && (!rule.byMonthDay || rule.byMonthDay.some((d) => (d > 0 ? d : length + 1 + d) === monthDay));
    days = fits ? [begin] : [];
  } else if (rule.freq === "WEEKLY") {
    begin = first.day - ((first.weekday - rule.weekStart + 7) % 7) + n * 7;
    const weekdays = rule.byDay.length ? rule.byDay.map((b) => b.day) : [first.weekday];
    days = weekdays.map((weekday) => begin + ((weekday - rule.weekStart + 7) % 7));
  } else if (rule.freq === "MONTHLY") {
    const month = new Date(Date.UTC(first.year, first.month + n, 1));
    begin = month.getTime() / DAY;
    days = monthDays(month.getUTCFullYear(), month.getUTCMonth(), rule, first);
  } else {
    begin = dayNumber(first.year + n, 0, 1);
    days = yearDays(first.year + n, rule, first);
  }
  if (rule.byMonth) days = days.filter((day) => rule.byMonth.includes(new Date(day * DAY).getUTCMonth()));
  days = [...new Set(days)].sort((a, b) => a - b);
  if (rule.bySetPos) days = rule.bySetPos.map((pos) => days.at(pos > 0 ? pos - 1 : pos)).filter((day) => day !== undefined).sort((a, b) => a - b);
  return { begin, days };
}

function untilTime(text, event, zone) {
  const until = parseICalDate(text, zone);
  if (!until) return Infinity;
  // Een datum zonder tijd bij een afspraak met tijd: die hele dag telt nog mee.
  return until.allDay && !event.allDay ? zonedTime(until.ts + DAY - 1, zone) : until.ts;
}

// De momenten na de start die de regel oplevert, binnen het venster. Na deadline (een tijdstip)
// stopt het rekenen en blijft het bij wat er al is.
function ruleMoments(rule, first, event, zone, windowStart, windowEnd, deadline) {
  const last = Math.min(rule.until ? untilTime(rule.until, event, zone) : Infinity, windowEnd);
  const moments = [];
  let count = 1; // de start is de eerste
  for (let step = 0; step < RULE_MAX_STEPS; step++) {
    if (Date.now() > deadline) return moments;
    const { begin, days } = rulePeriod(rule, first, step);
    if ((begin - 1) * DAY > last) return moments;
    for (const day of days) {
      if (day <= first.day) continue;
      if (count >= rule.count) return moments;
      const wall = day * DAY + first.clock;
      // Ver voor het venster telt alleen dat hij meetelt voor COUNT; dat scheelt rekenwerk.
      const ts = wall < windowStart - 2 * DAY ? wall : zonedTime(wall, zone);
      if (ts > last) return moments;
      count++;
      if (ts >= windowStart) moments.push(ts);
    }
  }
  return moments;
}

export function expandRrule(event, windowStart, windowEnd, deadline = Infinity) {
  if (!event.RRULE || event.start === null) return [];
  const occurrence = (ts) => ({ ...event, start: ts, end: event.end ? ts + (event.end - event.start) : null });
  const first = event.start >= windowStart && event.start <= windowEnd ? [occurrence(event.start)] : [];
  const rule = parseRule(event.RRULE);
  // Een regel die we niet begrijpen: alleen de eerste afspraak, die klopt in elk geval.
  if (!rule) return first;
  const zone = event.zone ?? (event.allDay ? "UTC" : HOME_ZONE);
  const wall = event.start + zoneOffset(event.start, zone);
  const date = new Date(wall);
  const day = Math.floor(wall / DAY);
  const start = { day, clock: wall - day * DAY, year: date.getUTCFullYear(), month: date.getUTCMonth(), date: date.getUTCDate(), weekday: date.getUTCDay() };
  return [...first, ...ruleMoments(rule, start, event, zone, windowStart, windowEnd, deadline).map(occurrence)];
}

// NAAM;TZID="zone: met dubbele punt";VALUE=DATE:inhoud
function parseProperty(line) {
  const match = line.match(/^((?:[^:"]|"[^"]*")*):(.*)$/);
  if (!match) return null;
  const [, head, value] = match;
  const tzid = head.match(/;TZID=(?:"([^"]*)"|([^;]*))/);
  return { name: head.split(";")[0], value: value.trim(), zone: tzid ? tzid[1] ?? tzid[2] : HOME_ZONE };
}

function parseVevents(text) {
  const lines = text.replace(/\r\n/g, "\n").replace(/\n[ \t]/g, "").split("\n");
  const events = [];
  let cur = null;
  let nested = 0;
  for (const line of lines) {
    if (line === "BEGIN:VEVENT") { cur = {}; nested = 0; continue; }
    if (line === "END:VEVENT") { if (cur) events.push(cur); cur = null; continue; }
    if (!cur) continue;
    // Een herinnering (VALARM) binnen de afspraak heeft zijn eigen SUMMARY.
    if (line.startsWith("BEGIN:")) { nested++; continue; }
    if (line.startsWith("END:")) { nested--; continue; }
    if (nested) continue;
    const prop = parseProperty(line);
    if (!prop) continue;
    if (prop.name === "EXDATE" || prop.name === "RDATE") (cur[prop.name] ??= []).push(prop);
    else cur[prop.name] ??= prop;
  }
  return events;
}

const dateOf = (prop) => (prop ? parseICalDate(prop.value, prop.zone) : null);
const dateList = (props = []) => props.flatMap((prop) => prop.value.split(",")
  .filter((value) => !value.includes("/")) // een periode (RDATE;VALUE=PERIOD) slaan we over
  .map((value) => parseICalDate(value, prop.zone))).filter(Boolean);

const unescapeText = (value) => (value ?? "").replace(/\\n/gi, " ").replace(/\\([,;\\])/g, "$1").trim();

function toEvent(raw) {
  const start = dateOf(raw.DTSTART);
  return {
    title: unescapeText(raw.SUMMARY?.value) || t("cal.untitled"),
    location: unescapeText(raw.LOCATION?.value),
    start: start?.ts ?? null,
    end: dateOf(raw.DTEND)?.ts ?? null,
    allDay: start?.allDay ?? false,
    RRULE: raw.RRULE?.value ?? null,
    zone: start?.allDay || raw.DTSTART?.value.endsWith("Z") ? "UTC" : raw.DTSTART?.zone
  };
}

// Vervallen momenten: EXDATE en momenten die als losse afspraak terugkomen (RECURRENCE-ID).
// Een datum zonder tijd schrapt de hele dag. Eén keer opzoeken per moment, hoe lang de lijst
// ook is: een feed met tienduizenden EXDATE's kost anders minuten.
function dropFilter(dropped, zone) {
  if (!dropped.length) return () => false;
  const times = new Set(dropped.map((d) => d.ts));
  const days = new Set(dropped.filter((d) => d.allDay).map((d) => d.ts / DAY));
  return (ts) => times.has(ts) || (days.size > 0 && days.has(Math.floor((ts + zoneOffset(ts, zone)) / DAY)));
}

export function parseICal(text) {
  const raws = parseVevents(text);
  const now = Date.now();
  const year = 365 * DAY;
  const windowStart = now - year;
  const windowEnd = now + 2 * year;
  const deadline = now + EXPAND_BUDGET_MS;

  const changed = new Map(); // UID → momenten uit de reeks die apart staan
  for (const raw of raws) {
    if (!raw["RECURRENCE-ID"]) continue;
    const uid = raw.UID?.value;
    changed.set(uid, [...(changed.get(uid) ?? []), dateOf(raw["RECURRENCE-ID"])].filter(Boolean));
  }

  const expanded = [];
  for (const raw of raws) {
    if (raw.STATUS?.value === "CANCELLED") continue;
    const event = toEvent(raw);
    if (event.start === null) continue;
    const occurrence = (ts) => ({ ...event, start: ts, end: event.end ? ts + (event.end - event.start) : null });
    const moments = event.RRULE && !raw["RECURRENCE-ID"]
      ? expandRrule(event, windowStart, windowEnd, deadline)
      : event.start >= windowStart && event.start <= windowEnd ? [event] : [];
    const known = new Set(moments.map((e) => e.start));
    const extra = [...new Set(dateList(raw.RDATE).map((d) => d.ts))]
      .filter((ts) => ts >= windowStart && ts <= windowEnd && !known.has(ts)).map(occurrence);
    const dropped = raw["RECURRENCE-ID"] ? [] : [...dateList(raw.EXDATE), ...(changed.get(raw.UID?.value) ?? [])];
    const isDropped = dropFilter(dropped, event.zone);
    for (const e of [...moments, ...extra]) if (!isDropped(e.start)) expanded.push(e);
  }
  return expanded
    .sort((a, b) => a.start - b.start)
    .map(({ title, location, start, end, allDay }) => ({ title, location, start, end, allDay }));
}
