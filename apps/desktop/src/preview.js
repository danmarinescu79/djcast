// Developer preview: render a view with sample data, without Tauri (for screenshots).
// Open /preview.html?view=mini (or console / decks) on the Vite dev server.
import { mount } from 'svelte';
import './app.css';
import Mini from './Mini.svelte';
import Console from './Console.svelte';
import Decks from './Decks.svelte';

const deck = (deck, title, artist, elapsed, remaining, playing, on_air, gain) => ({
  deck, title, artist, elapsed, remaining, playing, visible: true, gain, on_air,
});
const status = {
  live: true, connected: true, connected_secs: 221, bytes_sent: 5_400_000, device: 'Realtek Digital Output',
  sample_rate: 48000, recording: false, level_l: 0.42, level_r: 0.47,
  loudness: { preset: 'loud', target_lufs: -10, in_lufs: -22.1, out_lufs: -12.4, agc_gain_db: 11.0, limiter_db: -0.2, bands_db: [-4.5, -1.2, -0.2, -2.5] },
  track: { artist: 'Digital Underground', title: 'The Humpty Dance [xtendz] - Clean', deck: 1, display: 'Digital Underground - The Humpty Dance [xtendz] - Clean' },
  tracklist: [{ time: '00:00:00', title: 'Digital Underground - The Humpty Dance [xtendz] - Clean' }],
  decks: {
    decks: [
      deck(1, 'The Humpty Dance [xtendz] - Clean', 'Digital Underground', '01:12', '-02:26', true, true, 1),
      deck(2, 'Salt Shaker (Quick Hit Clean)', 'Ying Yang Twins Ft Lil Jon', '03:01', '-00:21', true, false, 0.4),
    ],
    on_air: 1, faders: [1, 0.4, null, null], crossfader: 0.5, midi: true, using_fallback: false, error: null,
  },
  outputs: [{ name: 'Default', host: 'my.powerstream.live:8828', bitrate: 192, connected: true, connected_secs: 221, bytes_sent: 5_400_000, error: null }],
  nowplaying_source: 'auto', nowplaying_error: null, error: null,
};
const view = new URLSearchParams(location.search).get('view') || 'mini';
const props = { status, busy: false, actionError: '', onToggleLive() {}, onExpand() {}, onSendTitle() {} };
const C = { mini: Mini, console: Console, decks: Decks }[view];
mount(C, { target: document.getElementById('app'), props });
