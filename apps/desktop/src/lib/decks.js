import { t } from './i18n.svelte.js';

/** "mm:ss" or "-mm:ss" → seconds (null if absent). */
function secs(t) {
  const m = /^-?(\d{1,3}):(\d{2})$/.exec((t || '').trim());
  return m ? Number(m[1]) * 60 + Number(m[2]) : null;
}

/** Four deck slots for the UI, filled from the live source when it knows them. */
export function deckSlots(status) {
  return [1, 2, 3, 4].map((n) => {
    const d = status.decks?.decks?.find((x) => x.deck === n);
    const fader = status.decks?.faders?.[n - 1];
    const el = secs(d?.elapsed);
    const rem = secs(d?.remaining);
    const total = el != null && rem != null ? el + rem : null;
    return {
      n,
      loaded: !!d,
      remaining: d?.remaining || '',
      // Track position 0..1 when both times are known.
      progress: total ? el / total : 0,
      // Last 30 seconds of a playing track: time to mix.
      ending: !!d?.playing && rem != null && rem <= 30,
      title: d?.title || '',
      artist: d?.artist || '',
      elapsed: d?.elapsed || '--:--',
      // 'onair' / 'playing' / 'stopped' / 'empty', shown with the deck.* translation keys
      stateKey: d ? (d.on_air ? 'onair' : d.playing ? 'playing' : 'stopped') : 'empty',
      onAir: !!d?.on_air,
      playing: !!d?.playing,
      hidden: d ? !d.visible : false,
      // Fader position when known, else the mixer gain (1 = unknown/open).
      fader: Math.round(100 * (fader ?? (d ? d.gain : 0))),
    };
  });
}

export function sourceLabel(status) {
  if (status.nowplaying_source === 'none') return t('source.manual');
  if (status.decks?.using_fallback) return t('source.history');
  if (status.decks) return t('source.live');
  return status.nowplaying_source;
}
