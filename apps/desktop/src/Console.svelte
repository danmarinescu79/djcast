<script>
  import { clock, bytes, meterFraction, lufs, db, presetLabel } from './lib/format.js';
  import BandMeters from './lib/BandMeters.svelte';
  import { deckSlots, sourceLabel as srcLabel } from './lib/decks.js';
  import { t } from './lib/i18n.svelte.js';

  let { status, busy, actionError, onToggleLive, onSendTitle } = $props();

  let manualTitle = $state('');
  let copied = $state(false);

  // Always show four deck slots, filled from the live source when it knows them.
  const decks = $derived(
    deckSlots(status).map((d) => ({ ...d, title: d.title || '—' }))
  );

  const sourceLabel = $derived(srcLabel(status));

  const liveLabel = $derived(!status.live ? t('live.goLive') : status.connected ? t('live.onAir') : t('live.connecting'));

  async function saveTracklist() {
    const { save } = await import('@tauri-apps/plugin-dialog');
    const stamp = new Date().toISOString().slice(0, 16).replace('T', ' ').replace(':', '-');
    const path = await save({ defaultPath: `djcast tracklist ${stamp}.txt`, filters: [{ name: 'Text', extensions: ['txt'] }] });
    if (typeof path === 'string') {
      const { invoke } = await import('@tauri-apps/api/core');
      await invoke('save_tracklist', { path });
    }
  }

  async function copyTracklist() {
    const text = status.tracklist.map((t) => `${t.time} ${t.title}`).join('\n');
    await navigator.clipboard.writeText(text);
    copied = true;
    setTimeout(() => (copied = false), 1500);
  }

  function submitTitle(e) {
    e.preventDefault();
    onSendTitle(manualTitle);
    manualTitle = '';
  }
</script>

<main>
  <section class="col">
    <button
      type="button"
      class="live"
      class:on={status.live}
      class:waiting={status.live && !status.connected}
      disabled={busy}
      onclick={onToggleLive}
    >
      <span class="live-label">{liveLabel}</span>
      <span class="live-time mono">{status.live ? clock(status.connected_secs) : '00:00:00'}</span>
      <span class="live-hint">{status.live ? t('live.clickStop') : t('live.clickStart')}</span>
    </button>
    {#if actionError}<p class="error" role="alert">{actionError}</p>{/if}
    {#if status.error}<p class="error" role="status">{status.error}</p>{/if}

    <div class="panel stats">
      <div><div class="k">{t('stats.sent')}</div><div class="v">{bytes(status.bytes_sent)}</div></div>
      <div><div class="k">{t('stats.status')}</div><div class="v" class:ok={status.connected}>{status.connected ? t('stats.connected') : status.live ? t('stats.retrying') : t('stats.offline')}</div></div>
      <div><div class="k">{t('stats.recording')}</div><div class="v" class:ok={status.recording}>{status.recording ? t('common.on') : t('common.off')}</div></div>
      <div><div class="k">{t('stats.preset')}</div><div class="v preset">{presetLabel(status.loudness)}</div></div>
    </div>

    {#if status.outputs.length > 0}
      <div class="panel servers">
        <div class="k">{t('stats.servers')}</div>
        {#each status.outputs as o (o.name)}
          <div class="srv" title={o.error || o.host}>
            <span class="led" class:up={o.connected} class:down={!o.connected && o.error}></span>
            <span class="srv-name">{o.name}</span>
            <span class="mono muted small">{o.connected ? bytes(o.bytes_sent) : o.error ? t('stats.retryingShort') : t('stats.connectingShort')}</span>
          </div>
        {/each}
      </div>
    {/if}

    <div class="panel input">
      <div class="k">{t('stats.input')}</div>
      <div>{status.device || t('common.systemDefault')}</div>
      <div class="k">{t('stats.loopback')}</div>
    </div>
  </section>

  <section class="col center">
    <div class="panel now">
      <div class="badges">
        <span class="label">{t('now.title')}</span>
        {#if status.track?.deck}<span class="badge red">{t('now.deckOnAir', { n: status.track.deck })}</span>{/if}
        <span class="badge blue">{sourceLabel}</span>
      </div>
      <div class="title">{status.track?.title || t('now.waiting')}</div>
      <div class="artist">{status.track?.artist || ''}</div>
      {#if status.nowplaying_error && !status.decks?.using_fallback}
        <div class="hint">{status.nowplaying_error}</div>
      {/if}
      <form class="manual" onsubmit={submitTitle}>
        <label for="manual-title" class="sr">{t('now.manualLabel')}</label>
        <input id="manual-title" placeholder={t('now.manualPlaceholder')} bind:value={manualTitle} />
      </form>
    </div>

    <div class="decks">
      {#each decks as d (d.n)}
        <div class="panel deck" class:onair={d.onAir}>
          <div class="deck-head">
            <span>{t('deck.n', { n: d.n }).toUpperCase()}</span>
            <span class:red-text={d.onAir} class:ok={!d.onAir && d.playing}>{t(`deck.${d.stateKey}`)}</span>
          </div>
          <div class="deck-title">{d.title}</div>
          <div class="fader"><div style:width="{d.fader}%" class:hot={d.onAir}></div></div>
          {#if d.loaded}
            <div class="pos" title="{d.elapsed} / {d.remaining}">
              <div style:width="{d.progress * 100}%" class:ending={d.ending}></div>
            </div>
          {/if}
        </div>
      {/each}
    </div>

    <div class="panel tracklist">
      <div class="tl-head">
        <span class="label">{t('tracklist.title')}</span>
        <span class="tl-actions">
          <button type="button" class="link" disabled={!status.tracklist.length} onclick={saveTracklist}>{t('tracklist.save')}</button>
          <button type="button" class="link" disabled={!status.tracklist.length} onclick={copyTracklist}>
            {copied ? t('tracklist.copied') : t('tracklist.copy')}
          </button>
        </span>
      </div>
      <div class="tl-rows">
        {#each status.tracklist as t, i (i)}
          <div class="tl-row"><span class="mono muted">{t.time}</span><span class="ellipsis">{t.title}</span></div>
        {:else}
          <div class="muted small">{t('tracklist.empty')}</div>
        {/each}
      </div>
    </div>
  </section>

  <section class="panel col out">
    <span class="label">{t('out.title')}</span>
    <div class="meters">
      <div class="meter"><div class="track"><div class="fill" style:height="{meterFraction(status.level_l) * 100}%"></div></div><span>L</span></div>
      <div class="meter"><div class="track"><div class="fill" style:height="{meterFraction(status.level_r) * 100}%"></div></div><span>R</span></div>
      <div class="lufs">
        <span class="big mono">{lufs(status.loudness.out_lufs)}</span>
        <span class="muted small">LUFS{status.loudness.target_lufs != null ? ` · ${t('out.target', { v: status.loudness.target_lufs })}` : ''}</span>
      </div>
    </div>
    <div class="rows">
      <div><span class="muted">{t('out.mixIn')}</span><span class="mono">{lufs(status.loudness.in_lufs)} LUFS</span></div>
      <div><span class="muted">AGC</span><span class="mono blue-text">{db(status.loudness.agc_gain_db)} dB</span></div>
      <div><span class="muted">{t('out.limiter')}</span><span class="mono warn-text">{db(status.loudness.limiter_db, false)} dB</span></div>
      {#if status.loudness.bands_db}
        <div class="mb">
          <span class="muted">{t('out.bands')}</span>
          <BandMeters bands={status.loudness.bands_db} />
        </div>
      {/if}
      <div><span class="muted">{t('out.midi')}</span><span>{status.decks?.midi ? t('stats.connected') : '—'}</span></div>
    </div>
  </section>
</main>

<style>
  main {
    flex-grow: 1; min-height: 0;
    display: grid; grid-template-columns: 260px minmax(0, 1fr) 230px;
    gap: 16px; padding: 16px 20px;
  }
  .col { display: flex; flex-direction: column; gap: 12px; min-width: 0; min-height: 0; }

  .live {
    height: 150px; border: 1px solid var(--line-strong); border-radius: 14px;
    background: var(--panel); color: var(--text);
    display: flex; flex-direction: column; align-items: center; justify-content: center; gap: 6px;
  }
  .live:hover { border-color: #4a505a; }
  .live.on { background: var(--on-air); border-color: var(--on-air); color: #fff; }
  .live.waiting { background: #8a3030; }
  .live-label { font-size: 13px; font-weight: 600; letter-spacing: 2px; }
  .live-time { font-size: 34px; font-weight: 700; }
  .live-hint { font-size: 12px; opacity: 0.85; }

  .error { margin: 0; font-size: 12px; color: var(--on-air-text); user-select: text; }
  .stats { padding: 14px; display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 12px; }
  .k { font-size: 11px; color: var(--muted); }
  .v { font-size: 18px; font-weight: 600; }
  .preset { font-size: 15px; line-height: 1.3; }
  .ok { color: var(--ok); }
  .servers { padding: 12px 14px; display: flex; flex-direction: column; gap: 8px; }
  .srv { display: flex; align-items: center; gap: 8px; font-size: 13px; }
  .srv-name { flex-grow: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .led { width: 8px; height: 8px; border-radius: 50%; background: var(--muted); flex-shrink: 0; }
  .led.up { background: var(--ok); }
  .led.down { background: var(--on-air); }
  .input { padding: 14px; display: flex; flex-direction: column; gap: 6px; font-size: 13px; }

  .now { padding: 18px 20px; display: flex; flex-direction: column; gap: 8px; }
  .badges { display: flex; align-items: center; gap: 8px; }
  .badge { font-size: 11px; padding: 3px 8px; border-radius: 999px; }
  .red { background: var(--on-air-soft); color: var(--on-air-text); }
  .blue { background: var(--info-soft); color: var(--info); }
  .title { font-size: 26px; font-weight: 700; line-height: 1.2; user-select: text; }
  .artist { font-size: 16px; color: var(--text-2); min-height: 22px; user-select: text; }
  .hint { font-size: 12px; color: var(--muted); }
  .manual input {
    width: 100%; height: 34px; padding: 0 10px; margin-top: 4px;
    background: var(--bg); color: var(--text); border: 1px solid var(--line-strong); border-radius: 8px; font-size: 13px;
  }
  .sr { position: absolute; width: 1px; height: 1px; overflow: hidden; clip: rect(0 0 0 0); }

  .decks { display: grid; grid-template-columns: repeat(4, minmax(0, 1fr)); gap: 10px; }
  .deck { padding: 10px; display: flex; flex-direction: column; gap: 6px; min-width: 0; border-radius: 10px; }
  .deck.onair { border-color: var(--on-air); }
  .deck-head { display: flex; justify-content: space-between; font-size: 11px; color: var(--muted); }
  .red-text { color: var(--on-air-text); }
  .deck-title { font-size: 12px; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .fader { height: 6px; background: var(--line); border-radius: 3px; }
  .fader > div { height: 6px; background: #5b6470; border-radius: 3px; transition: width 80ms linear; }
  .fader > div.hot { background: #ff6b6b; }
  .pos { height: 3px; background: var(--line); border-radius: 2px; overflow: hidden; }
  .pos > div { height: 3px; background: var(--text-2); transition: width 400ms linear; }
  .pos > div.ending { background: var(--warn); }

  .tracklist { flex-grow: 1; min-height: 0; padding: 14px 18px; display: flex; flex-direction: column; gap: 8px; }
  .tl-head { display: flex; justify-content: space-between; align-items: center; }
  .tl-actions { display: flex; gap: 16px; }
  .link { background: none; border: none; color: var(--info); font-size: 12px; padding: 0; }
  .tl-rows { overflow-y: auto; min-height: 0; }
  .tl-row { display: flex; gap: 14px; font-size: 13px; padding: 5px 0; border-top: 1px solid #22252c; user-select: text; }
  .tl-row .mono { width: 64px; flex-shrink: 0; }
  .ellipsis { min-width: 0; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .muted { color: var(--muted); }
  .small { font-size: 12px; }

  .out { padding: 14px; gap: 14px; }
  .meters { display: flex; gap: 14px; height: 230px; align-items: stretch; }
  .meter { display: flex; flex-direction: column; align-items: center; gap: 6px; font-size: 11px; color: var(--muted); }
  .track { width: 22px; flex-grow: 1; background: var(--line); border-radius: 4px; display: flex; flex-direction: column; justify-content: flex-end; overflow: hidden; }
  .fill { background: var(--ok); border-radius: 4px; transition: height 50ms linear; }
  .lufs { display: flex; flex-direction: column; justify-content: flex-end; gap: 4px; padding-bottom: 22px; }
  .big { font-size: 30px; font-weight: 700; }
  .rows { display: flex; flex-direction: column; gap: 10px; font-size: 13px; }
  .rows > div { display: flex; justify-content: space-between; }
  .blue-text { color: var(--info); }
  .mb { align-items: center; }
  .warn-text { color: var(--warn); }
</style>
