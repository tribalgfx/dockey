// Weer via Open-Meteo (geen account of sleutel nodig). Gedeeld door balk en paneel.
import { t } from './i18n.js';

// WMO-weercodes → [codes, emoji overdag, emoji 's nachts, tekst (sleutel in i18n.js)]
const WMO = [
  [[0], '☀️', '🌙', 'wx.clear'], [[1], '🌤️', '🌙', 'wx.mainlyClear'], [[2], '⛅', '☁️', 'wx.partlyCloudy'], [[3], '☁️', '☁️', 'wx.cloudy'],
  [[45, 48], '🌫️', '🌫️', 'wx.fog'], [[51, 53, 55, 56, 57], '🌦️', '🌧️', 'wx.drizzle'], [[61, 63, 65, 66, 67], '🌧️', '🌧️', 'wx.rain'],
  [[71, 73, 75, 77], '🌨️', '🌨️', 'wx.snow'], [[80, 81, 82], '🌦️', '🌧️', 'wx.showers'], [[85, 86], '🌨️', '🌨️', 'wx.snowShowers'],
  [[95, 96, 99], '⛈️', '⛈️', 'wx.thunder'],
];

export function describe(code, isDay = true) {
  const row = WMO.find(([codes]) => codes.includes(code)) ?? [[], '🌡️', '🌡️', ''];
  return { emoji: isDay ? row[1] : row[2], text: row[3] ? t(row[3]) : '' };
}

const BASE = 'https://api.open-meteo.com/v1/forecast';

/** Alleen het weer van nu (voor de balk). */
export async function currentWeather({ lat, lon }) {
  const url = `${BASE}?latitude=${lat}&longitude=${lon}&current=temperature_2m,weather_code,is_day&timezone=auto`;
  const { current } = await (await fetch(url)).json();
  return { temp: current.temperature_2m, ...describe(current.weather_code, current.is_day) };
}

/** Nu, de komende uren en de komende zeven dagen (voor het paneel). */
export async function forecast({ lat, lon }) {
  const url =
    `${BASE}?latitude=${lat}&longitude=${lon}&timezone=auto&forecast_days=7` +
    '&current=temperature_2m,apparent_temperature,relative_humidity_2m,wind_speed_10m,weather_code,is_day' +
    '&hourly=temperature_2m,weather_code,precipitation_probability,is_day' +
    '&daily=weather_code,temperature_2m_max,temperature_2m_min,precipitation_probability_max,sunrise,sunset';
  return (await fetch(url)).json();
}

export const deg = (v) => `${Math.round(v)}°`;
