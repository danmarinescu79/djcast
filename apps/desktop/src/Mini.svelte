<script>
  import { clock, meterFraction, lufs, db, presetLabel } from './lib/format.js';
  import BandMeters from './lib/BandMeters.svelte';

  let { status, busy, onToggleLive, onExpand } = $props();
</script>

<div class="mini">
  <div class="top">
    <button type="button" class="live" class:on={status.live} disabled={busy} onclick={onToggleLive}>
      <span class="dot"></span>
      <span>{status.live ? (status.connected ? `LIVE ${clock(status.connected_secs)}` : 'CONNECTING') : 'GO LIVE'}</span>
    </button>
    <span class="muted small">{presetLabel(status.loudness)}</span>
    <span class="grow"></span>
    <button type="button" class="icon" aria-label="Open full window" title="Full window" onclick={onExpand}>
      <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M15 3h6v6"></path><path d="M9 21H3v-6"></path><path d="M21 3l-7 7"></path><path d="M3 21l7-7"></path></svg>
    </button>
  </div>

  <div class="now">
    <span class="deck">{status.track?.deck ? `DECK ${status.track.deck} · ON AIR` : 'NOW PLAYING'}</span>
    <span class="title">{status.track?.title || 'Waiting for a track…'}</span>
    <span class="artist">{status.track?.artist || ''}</span>
  </div>

  <div class="grow"></div>

  <div class="bottom" class:with-bands={!!status.loudness.bands_db}>
    <div class="row l"><span>L</span><div class="track"><div style:width="{meterFraction(status.level_l) * 100}%"></div></div></div>
    <div class="row r"><span>R</span><div class="track"><div style:width="{meterFraction(status.level_r) * 100}%"></div></div></div>
    <div class="nums mono">
      <span>{lufs(status.loudness.out_lufs)} LUFS</span>
      <span>AGC {db(status.loudness.agc_gain_db)}</span>
      <span>LIM {db(status.loudness.limiter_db, false)}</span>
    </div>
    {#if status.loudness.bands_db}
      <div class="bands"><BandMeters bands={status.loudness.bands_db} part="bars" /></div>
      <div class="band-labels"><BandMeters bands={status.loudness.bands_db} part="labels" /></div>
    {/if}
  </div>
</div>

<style>
  .mini { height: 100%; padding: 16px; display: flex; flex-direction: column; gap: 12px; background: var(--bar); }
  .top { display: flex; align-items: center; gap: 10px; }
  .live {
    height: 34px; padding: 0 14px; border: 1px solid var(--line-strong); border-radius: 999px;
    background: var(--panel); color: var(--text); font-weight: 700; font-size: 13px;
    display: flex; align-items: center; gap: 8px;
  }
  .live.on { background: var(--on-air); border-color: var(--on-air); color: #fff; }
  .dot { width: 8px; height: 8px; border-radius: 50%; background: currentColor; }
  .icon { width: 34px; height: 34px; border: 1px solid var(--line-strong); border-radius: 8px; background: transparent; color: var(--text-2); display: flex; align-items: center; justify-content: center; }
  .now { display: flex; flex-direction: column; gap: 4px; min-width: 0; }
  .deck { font-size: 11px; color: var(--on-air-text); font-weight: 600; letter-spacing: 1px; }
  .title { font-size: 18px; font-weight: 700; line-height: 1.25; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; user-select: text; }
  .artist { font-size: 13px; color: var(--text-2); min-height: 18px; }
  .grow { flex-grow: 1; }
  /* Rows: L, R, numbers. Band bars span the L and R rows; their labels sit on the numbers row. */
  .bottom {
    display: grid; grid-template-columns: minmax(0, 1fr); grid-template-rows: 14px 14px 18px;
    row-gap: 6px; column-gap: 16px; align-items: center;
  }
  .bottom.with-bands { grid-template-columns: minmax(0, 1fr) auto; }
  .l { grid-row: 1; grid-column: 1; }
  .r { grid-row: 2; grid-column: 1; }
  .nums { grid-row: 3; grid-column: 1; line-height: 18px; }
  /* 3px in from the row edges = the top of the L bar to the bottom of the R bar (8px bars in 14px rows). */
  .bands { grid-row: 1 / 3; grid-column: 2; align-self: stretch; padding: 3px 0; }
  .band-labels { grid-row: 3; grid-column: 2; line-height: 18px; }
  .row { display: flex; align-items: center; gap: 8px; font-size: 11px; color: var(--muted); }
  .row span { width: 10px; }
  .track { flex-grow: 1; height: 8px; background: var(--line); border-radius: 4px; overflow: hidden; }
  .track > div { height: 8px; background: var(--ok); border-radius: 4px; transition: width 50ms linear; }
  .nums { display: flex; justify-content: space-between; font-size: 12px; color: var(--muted); }
  .muted { color: var(--muted); }
  .small { font-size: 12px; }
</style>
