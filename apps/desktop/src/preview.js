// Developer preview: render a view with sample data, without Tauri (for screenshots).
// Open /preview.html?view=mini (or console / decks) on the Vite dev server.
import { mount } from 'svelte';
import './app.css';
import { setLanguage } from './lib/i18n.svelte.js';
import Mini from './Mini.svelte';
import Console from './Console.svelte';
import Decks from './Decks.svelte';
import Settings from './Settings.svelte';

const deck = (deck, title, artist, elapsed, remaining, playing, on_air, gain) => ({
  deck, title, artist, elapsed, remaining, playing, visible: true, gain, on_air,
});
const status = {
  live: true, connected: true, connected_secs: 221, bytes_sent: 5_400_000, device: 'Realtek Digital Output',
  sample_rate: 48000, recording: false, level_l: 0.42, level_r: 0.47,
  loudness: { preset: 'loud', target_lufs: -10, in_lufs: -22.1, out_lufs: -12.4, agc_gain_db: 11.0, limiter_db: -0.2, bands_db: [-4.5, -1.2, -0.2, -2.5] },
  track: { artist: 'Nova Tide', title: 'Midnight Signal (Extended Mix)', deck: 1, display: 'Nova Tide - Midnight Signal (Extended Mix)' },
  tracklist: [
    { time: '00:00:00', title: 'Kairo Lane - First Light' },
    { time: '00:02:58', title: 'Nova Tide - Midnight Signal (Extended Mix)' },
  ],
  decks: {
    decks: [
      deck(1, 'Midnight Signal (Extended Mix)', 'Nova Tide', '01:12', '-02:26', true, true, 1),
      deck(2, 'Golden Hour', 'The Sundays Collective', '03:01', '-00:21', true, false, 0.4),
    ],
    on_air: 1, faders: [1, 0.4, null, null], crossfader: 0.5, midi: true, using_fallback: false, error: null,
  },
  outputs: [{ name: 'Default', host: 'radio.example.com:8000', bitrate: 192, connected: true, connected_secs: 221, bytes_sent: 5_400_000, error: null }],
  nowplaying_source: 'auto', nowplaying_error: null, error: null,
};
const params = new URLSearchParams(location.search);
const view = params.get('view') || 'mini';
setLanguage(params.get('lang') || 'en');
const props = { status, busy: false, actionError: '', onToggleLive() {}, onExpand() {}, onSendTitle() {} };
const server = { protocol: 'icecast', host: 'radio.example.com', port: 8000, mount: '/live', user: 'source', password: 'example', stream_id: 1, name: 'My DJ Stream', description: 'Live DJ sets', genre: 'Electronic', url: 'https://radio.example.com', public: false, reconnect_delay_secs: 3 };
const config = {
  audio: { device: 'Realtek Digital Output', mode: 'loopback', gain: 1 },
  processing: { preset: 'loud', target_lufs: -14, ceiling_db: -1, max_gain_db: 15, multiband: true },
  encoder: { codec: params.get('codec') || 'mp3', bitrate: 192 }, server, record: { enabled: false, dir: 'recordings' },
  nowplaying: { source: 'auto', poll_interval_secs: 1, rekordbox_key: '', rekordbox_db: '', midi_input: 'auto', confirm_secs: 4, crossfader_assign: ['thru', 'thru', 'thru', 'thru'], output_dir: 'nowplaying' },
  active_profile: 'Default', profiles: [{ name: 'Default', server, codec: params.get('codec') || 'mp3', bitrate: 192, live: true }], language: params.get('lang') || 'auto',
};
Object.assign(props, { config, live: false, systemLocale: 'ro-RO', onSaved() {}, onClose() {} });
const C = { mini: Mini, console: Console, decks: Decks, settings: Settings }[view];
mount(C, { target: document.getElementById('app'), props });
