<script>
  import { lufs, db, meterFraction, presetLabel } from './lib/format.js';
  import BandMeters from './lib/BandMeters.svelte';
  import { deckSlots, sourceLabel } from './lib/decks.js';
  import { t } from './lib/i18n.svelte.js';

  let { status } = $props();

  const decks = $derived(deckSlots(status));
  const xfader = $derived(status.decks?.crossfader);
</script>

<main>
  <section class="grid">
    {#each decks as d (d.n)}
      <div class="deck" class:onair={d.onAir}>
        <div class="head">
          <span class="num">{d.n}</span>
          {#if d.playing}<span class="pulse" class:red-dot={d.onAir} aria-hidden="true"></span>{/if}
          <span class="state" class:red={d.onAir} class:ok={!d.onAir && d.playing}>{t(`deck.${d.stateKey}`)}</span>
          {#if d.hidden}<span class="hidden">{t('deck.hidden')}</span>{/if}
        </div>
        <div class="title">{d.loaded ? d.title : t('deck.noTrack')}</div>
        <div class="artist">{d.artist}</div>
        <div class="grow"></div>
        {#if d.loaded}
          <div class="pos-row">
            <span class="mono muted">{d.elapsed}</span>
            <div class="pos" role="progressbar" aria-label={t('deck.position', { n: d.n })} aria-valuemin="0" aria-valuemax="100" aria-valuenow={Math.round(d.progress * 100)}>
              <div style:width="{d.progress * 100}%" class:ending={d.ending}></div>
            </div>
            <span class="mono rem" class:ending-text={d.ending}>{d.remaining}</span>
          </div>
        {/if}
        <div class="fader-row">
          <span class="muted">{t('deck.fader')}</span>
          <div class="fader"><div style:width="{d.fader}%" class:hot={d.onAir}></div></div>
          <span class="mono muted pct">{d.fader}%</span>
        </div>
      </div>
    {/each}
  </section>

  <aside>
    <div class="panel card">
      <span class="label">{t('decks.sent')}</span>
      <span class="sent">{status.track?.display || t('now.waiting')}</span>
      <span class="muted small">{sourceLabel(status)}{status.track?.deck ? ` · ${t('deck.n', { n: status.track.deck })}` : ''}</span>
    </div>

    <div class="panel card">
      <span class="label">{t('decks.mixer')}{status.decks?.midi ? ' · MIDI' : ''}</span>
      <div class="faders">
        {#each decks as d (d.n)}
          <div class="vfader">
            <div class="vtrack"><div style:height="{d.fader}%" class:hot={d.onAir}></div></div>
            <span class="muted small">{d.n}</span>
          </div>
        {/each}
      </div>
      <div class="xfader" aria-label={t('decks.crossfader')}>
        <span class="muted small">A</span>
        <div class="xtrack">
          {#if xfader != null}<div class="knob" style:left="calc({xfader * 100}% - 5px)"></div>{/if}
        </div>
        <span class="muted small">B</span>
      </div>
      {#if !status.decks?.midi}<span class="muted small">{t('decks.noMidi')}</span>{/if}
    </div>

    <div class="panel card rows">
      <span class="label">{t('out.title')} · {presetLabel(status.loudness)}</span>
      <div class="hmeter"><span class="muted">L</span><div class="htrack"><div style:width="{meterFraction(status.level_l) * 100}%"></div></div></div>
      <div class="hmeter"><span class="muted">R</span><div class="htrack"><div style:width="{meterFraction(status.level_r) * 100}%"></div></div></div>
      <div class="loud">
        <span class="big mono">{lufs(status.loudness.out_lufs)}</span>
        <span class="muted small">LUFS{status.loudness.target_lufs != null ? ` · ${t('out.target', { v: status.loudness.target_lufs })}` : ''}</span>
      </div>
      <div><span class="muted">AGC</span><span class="mono blue">{db(status.loudness.agc_gain_db)} dB</span></div>
      <div><span class="muted">{t('out.limiter')}</span><span class="mono warn">{db(status.loudness.limiter_db, false)} dB</span></div>
      {#if status.loudness.bands_db}
        <div class="bands"><span class="muted">{t('out.bands')}</span><BandMeters bands={status.loudness.bands_db} /></div>
      {/if}
    </div>
  </aside>
</main>

<style>
  main { flex-grow: 1; min-height: 0; display: flex; gap: 16px; padding: 16px 20px; }
  .grid { flex-grow: 1; min-width: 0; display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); grid-template-rows: repeat(2, minmax(0, 1fr)); gap: 12px; }
  .deck {
    background: var(--panel); border: 2px solid var(--line); border-radius: 14px;
    padding: 16px 18px; display: flex; flex-direction: column; gap: 8px; min-width: 0;
  }
  .deck.onair { background: #1d1416; border-color: var(--on-air); }
  .head { display: flex; align-items: center; gap: 10px; }
  .num { width: 30px; height: 30px; border-radius: 8px; background: #22252c; display: flex; align-items: center; justify-content: center; font-weight: 700; }
  .state { font-size: 12px; font-weight: 600; letter-spacing: 1px; color: var(--muted); text-transform: uppercase; }
  .red { color: var(--on-air-text); }
  .ok { color: var(--ok); }
  .hidden { font-size: 11px; color: var(--muted); border: 1px solid var(--line-strong); border-radius: 999px; padding: 1px 8px; }
  .title { font-size: 19px; font-weight: 700; line-height: 1.25; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; user-select: text; }
  .artist { font-size: 14px; color: var(--text-2); min-height: 20px; user-select: text; }
  .grow { flex-grow: 1; }
  .pulse { width: 8px; height: 8px; border-radius: 50%; background: var(--ok); animation: pulse 1.2s ease-in-out infinite; }
  .pulse.red-dot { background: var(--on-air-text); }
  @keyframes pulse { 0%, 100% { opacity: 1; } 50% { opacity: 0.25; } }
  @media (prefers-reduced-motion: reduce) { .pulse { animation: none; } }
  .pos-row { display: flex; align-items: center; gap: 10px; font-size: 12px; }
  .pos { flex-grow: 1; height: 6px; background: var(--line); border-radius: 3px; overflow: hidden; }
  .pos > div { height: 6px; background: var(--text-2); border-radius: 3px; transition: width 400ms linear; }
  .pos > div.ending { background: var(--warn); }
  .rem { color: var(--text-2); }
  .ending-text { color: var(--warn); font-weight: 600; }
  .fader-row { display: flex; align-items: center; gap: 10px; font-size: 11px; }
  .fader { flex-grow: 1; height: 8px; background: var(--line); border-radius: 4px; }
  .fader > div { height: 8px; background: #5b6470; border-radius: 4px; transition: width 80ms linear; }
  .hot { background: #ff6b6b !important; }
  .pct { width: 36px; text-align: right; }

  aside { width: 250px; display: flex; flex-direction: column; gap: 12px; overflow-y: auto; min-height: 0; }
  .card { padding: 16px; display: flex; flex-direction: column; gap: 10px; }
  .sent { font-size: 16px; font-weight: 700; line-height: 1.3; user-select: text; }
  .faders { display: flex; justify-content: space-between; height: 110px; }
  .vfader { display: flex; flex-direction: column; align-items: center; gap: 6px; }
  .vtrack { width: 14px; flex-grow: 1; background: var(--line); border-radius: 3px; display: flex; flex-direction: column; justify-content: flex-end; overflow: hidden; }
  .vtrack > div { background: #5b6470; border-radius: 3px; transition: height 80ms linear; }
  .xfader { display: flex; align-items: center; gap: 8px; }
  .xtrack { flex-grow: 1; height: 6px; background: var(--line); border-radius: 3px; position: relative; }
  .knob { position: absolute; top: -5px; width: 10px; height: 16px; background: var(--text-2); border-radius: 2px; transition: left 80ms linear; }
  .rows { font-size: 13px; }
  .hmeter { display: flex; align-items: center; gap: 8px; }
  .hmeter span { width: 10px; font-size: 11px; }
  .htrack { flex-grow: 1; height: 8px; background: var(--line); border-radius: 4px; overflow: hidden; }
  .htrack > div { height: 8px; background: var(--ok); border-radius: 4px; transition: width 50ms linear; }
  .loud { display: flex; align-items: baseline; gap: 8px; }
  .big { font-size: 24px; font-weight: 700; }
  .bands { align-items: center; }
  .rows > div { display: flex; justify-content: space-between; }
  .muted { color: var(--muted); }
  .small { font-size: 12px; }
  .blue { color: var(--info); }
  .warn { color: var(--warn); }
</style>
