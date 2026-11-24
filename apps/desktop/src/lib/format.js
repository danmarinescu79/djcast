import { t } from './i18n.svelte.js';

export function clock(secs) {
  const s = Math.max(0, Math.floor(secs || 0));
  const pad = (n) => String(n).padStart(2, '0');
  return `${pad(Math.floor(s / 3600))}:${pad(Math.floor(s / 60) % 60)}:${pad(s % 60)}`;
}

export function bytes(n) {
  if (!n) return '0 MB';
  if (n < 1024 * 1024) return `${Math.round(n / 1024)} KB`;
  return `${(n / 1024 / 1024).toFixed(n < 100 * 1024 * 1024 ? 1 : 0)} MB`;
}

/** Peak (0..1) → bar fraction on a -48..0 dBFS scale. */
export function meterFraction(peak) {
  const db = 20 * Math.log10(Math.max(peak || 0, 1e-6));
  return Math.min(1, Math.max(0, (db + 48) / 48));
}

export function lufs(v) {
  return v == null ? '—' : v.toFixed(1);
}

export function db(v, signed = true) {
  if (v == null) return '—';
  const s = v.toFixed(1);
  return signed && v > 0 ? `+${s}` : s;
}

export const CODECS = { mp3: 'MP3', aac: 'AAC', opus: 'Opus' };

/** Bitrates offered per codec (kbps). The Windows AAC encoder takes 96–192 only. */
export const BITRATES = {
  mp3: [128, 160, 192, 256, 320],
  aac: [96, 128, 160, 192],
  opus: [64, 96, 128, 160, 192, 256],
};

/** The offered bitrate of `codec` closest to `kbps`. */
export function nearestBitrate(codec, kbps) {
  const list = BITRATES[codec] ?? BITRATES.mp3;
  return list.reduce((best, b) => (Math.abs(b - kbps) < Math.abs(best - kbps) ? b : best), list[0]);
}

/** "MP3 192 kbps" */
export function formatLabel(codec, kbps) {
  return `${CODECS[codec] ?? 'MP3'} ${kbps} kbps`;
}

/** "Loud", "Radio + multiband", "Off" … from the loudness status. */
export function presetLabel(loudness) {
  const name = t(`preset.${loudness?.preset || 'off'}`);
  return loudness?.bands_db ? t('preset.withMultiband', { name }) : name;
}
