/** Four deck slots for the UI, filled from the live source when it knows them. */
export function deckSlots(status) {
  return [1, 2, 3, 4].map((n) => {
    const d = status.decks?.decks?.find((x) => x.deck === n);
    const fader = status.decks?.faders?.[n - 1];
    return {
      n,
      loaded: !!d,
      title: d?.title || '',
      artist: d?.artist || '',
      elapsed: d?.elapsed || '--:--',
      state: d ? (d.on_air ? 'ON AIR' : d.playing ? 'PLAYING' : 'STOPPED') : 'EMPTY',
      onAir: !!d?.on_air,
      playing: !!d?.playing,
      hidden: d ? !d.visible : false,
      // Fader position when known, else the mixer gain (1 = unknown/open).
      fader: Math.round(100 * (fader ?? (d ? d.gain : 0))),
    };
  });
}

export function sourceLabel(status) {
  if (status.nowplaying_source === 'none') return 'manual titles';
  if (status.decks?.using_fallback) return 'rekordbox history';
  if (status.decks) return 'live detection';
  return status.nowplaying_source;
}
