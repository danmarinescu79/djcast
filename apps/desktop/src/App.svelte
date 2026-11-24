<script>
  import { invoke } from '@tauri-apps/api/core';
  import { listen } from '@tauri-apps/api/event';
  import { getCurrentWindow } from '@tauri-apps/api/window';
  import { LogicalSize } from '@tauri-apps/api/dpi';
  import { onMount } from 'svelte';
  import Console from './Console.svelte';
  import Decks from './Decks.svelte';
  import Mini from './Mini.svelte';
  import Settings from './Settings.svelte';
  import { clock } from './lib/format.js';
  import { t, setLanguage } from './lib/i18n.svelte.js';
  import { getVersion } from '@tauri-apps/api/app';
  import { checkForUpdate, takeWhatsNew } from './lib/updates.js';

  const stored = (k, d) => {
    try {
      return localStorage.getItem(k) ?? d;
    } catch {
      return d;
    }
  };
  const store = (k, v) => {
    try {
      localStorage.setItem(k, v);
    } catch {
      /* per-viewer convenience only */
    }
  };

  // 'console' | 'decks' (remembered), plus the settings screen and the mini window.
  let layout = $state(stored('djcast.layout', 'console'));
  let showSettings = $state(false);
  let mini = $state(false);
  let status = $state(null);
  let config = $state(null);
  let busy = $state(false);
  let actionError = $state('');
  let systemLocale = $state('en');
  let settingsTab = $state('general');
  // { kind: 'update', version } | { kind: 'whatsnew', version, notes } | null
  let banner = $state(null);

  async function startupChecks(cfg) {
    const version = await getVersion().catch(() => '');
    const news = takeWhatsNew(version);
    if (news) {
      banner = { kind: 'whatsnew', version, notes: news.notes };
      return;
    }
    if (cfg.app?.check_updates !== false) {
      const found = await checkForUpdate().catch(() => null); // offline or no release yet: stay quiet
      if (found) banner = { kind: 'update', version: found.version };
    }
  }

  function openSettings(tab) {
    settingsTab = tab;
    showSettings = true;
  }

  onMount(() => {
    Promise.all([invoke('get_config'), invoke('system_locale').catch(() => 'en')]).then(([c, sys]) => {
      systemLocale = sys;
      setLanguage(c.language, sys);
      config = c;
      startupChecks(c);
    });
    invoke('status').then((s) => (status = s));
    const unlisten = listen('status', (e) => (status = e.payload));
    return () => unlisten.then((f) => f());
  });

  function setLayout(l) {
    layout = l;
    showSettings = false;
    store('djcast.layout', l);
  }

  async function toggleLive() {
    busy = true;
    actionError = '';
    try {
      await invoke(status?.live ? 'stop' : 'go_live');
    } catch (e) {
      actionError = String(e);
    } finally {
      busy = false;
    }
  }

  async function sendTitle(title) {
    if (title.trim()) await invoke('set_title', { title });
  }

  async function setMini(on) {
    const win = getCurrentWindow();
    if (on) {
      await win.setMinSize(new LogicalSize(380, 240));
      await win.setSize(new LogicalSize(440, 300));
      await win.setAlwaysOnTop(true);
    } else {
      await win.setAlwaysOnTop(false);
      await win.setSize(new LogicalSize(1100, 800));
      await win.setMinSize(new LogicalSize(960, 720));
    }
    mini = on;
  }

  const PROTOCOLS = { icecast: 'Icecast', shoutcast1: 'Shoutcast v1', shoutcast2: 'Shoutcast v2' };
  const server = $derived.by(() => {
    if (!config) return '';
    const s = config.server;
    const where =
      s.protocol === 'icecast' ? s.mount : s.protocol === 'shoutcast2' ? ` · stream ${s.stream_id}` : '';
    return `${s.host}:${s.port}${where}`;
  });
  const format = $derived(
    config ? `${config.active_profile} · ${PROTOCOLS[config.server.protocol] ?? config.server.protocol} · MP3 ${config.encoder.bitrate} kbps` : ''
  );
</script>

{#if mini && status}
  <Mini {status} {busy} onToggleLive={toggleLive} onExpand={() => setMini(false)} />
{:else}
  <div class="app">
    <header>
      <div class="brand">djcast</div>
      <div class="chip mono">{server}</div>
      <div class="sub">{format}{status?.sample_rate ? ` · ${status.sample_rate / 1000} kHz` : ''}</div>
      <div class="grow"></div>

      {#if !showSettings}
        <div class="seg" role="group" aria-label={t('app.layout')}>
          <button type="button" class:active={layout === 'console'} aria-pressed={layout === 'console'} onclick={() => setLayout('console')}>{t('app.console')}</button>
          <button type="button" class:active={layout === 'decks'} aria-pressed={layout === 'decks'} onclick={() => setLayout('decks')}>{t('app.decks')}</button>
        </div>
      {/if}

      {#if layout === 'decks' && !showSettings && status}
        <button type="button" class="live-pill" class:on={status.live} disabled={busy} onclick={toggleLive}>
          <span class="dot"></span>
          {status.live ? (status.connected ? t('live.liveTime', { time: clock(status.connected_secs) }) : t('live.connecting')) : t('live.goLive')}
        </button>
      {:else if status?.live}
        <span class="live-tag">{t('live.live')}</span>
      {/if}

      <button class="btn" type="button" aria-label={t('app.miniTitle')} title={t('app.miniTitle')} onclick={() => setMini(true)}>{t('app.mini')}</button>
      <button class="btn" type="button" onclick={() => (showSettings ? (showSettings = false) : openSettings('general'))}>
        {showSettings ? t('app.back') : t('app.settings')}
      </button>
    </header>

    {#if banner && !showSettings}
      <div class="top-banner" role="status">
        {#if banner.kind === 'update'}
          <span>{t('banner.update', { v: banner.version })}</span>
          <button type="button" class="link" onclick={() => { openSettings('general'); banner = null; }}>{t('banner.view')}</button>
        {:else}
          <div class="news">
            <strong>{t('banner.whatsNew', { v: banner.version })}</strong>
            <pre>{banner.notes}</pre>
          </div>
        {/if}
        <span class="grow"></span>
        <button type="button" class="link" onclick={() => (banner = null)}>{t('banner.dismiss')}</button>
      </div>
    {/if}

    {#if status && config && showSettings}
      <Settings {config} live={status.live} {systemLocale} initialTab={settingsTab} onSaved={(c) => { config = c; setLanguage(c.language, systemLocale); }} onClose={() => (showSettings = false)} />
    {:else if status && config && layout === 'decks'}
      {#if actionError}<p class="banner" role="alert">{actionError}</p>{/if}
      <Decks {status} />
    {:else if status && config}
      <Console {status} {busy} {actionError} onToggleLive={toggleLive} onSendTitle={sendTitle} />
    {:else}
      <div class="loading">{t('app.starting')}</div>
    {/if}
  </div>
{/if}

<style>
  .app { height: 100%; display: flex; flex-direction: column; }
  header {
    height: 52px; padding: 0 20px; display: flex; align-items: center; gap: 12px;
    border-bottom: 1px solid var(--line); background: var(--bar);
  }
  .brand { font-weight: 700; font-size: 17px; letter-spacing: -0.2px; }
  .chip { font-size: 12px; color: var(--muted); padding: 5px 10px; border: 1px solid var(--line-strong); border-radius: 6px; white-space: nowrap; }
  .sub { font-size: 12px; color: var(--muted); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; min-width: 0; }
  .grow { flex-grow: 1; }
  .loading { margin: auto; color: var(--muted); }
  .seg { display: flex; border: 1px solid var(--line-strong); border-radius: 8px; overflow: hidden; flex-shrink: 0; }
  .seg button { height: 30px; padding: 0 12px; border: none; background: transparent; color: var(--muted); font-size: 13px; }
  .seg button.active { background: #22252c; color: var(--text); }
  .live-tag { font-size: 11px; font-weight: 700; letter-spacing: 1px; padding: 4px 10px; border-radius: 999px; background: var(--on-air); color: #fff; }
  .live-pill {
    height: 32px; padding: 0 14px; border: 1px solid var(--line-strong); border-radius: 999px; flex-shrink: 0;
    background: var(--panel); color: var(--text); font-weight: 700; font-size: 13px;
    display: flex; align-items: center; gap: 8px;
  }
  .live-pill.on { background: var(--on-air); border-color: var(--on-air); color: #fff; }
  .dot { width: 8px; height: 8px; border-radius: 50%; background: currentColor; }
  .btn { flex-shrink: 0; }
  .top-banner {
    display: flex; align-items: flex-start; gap: 12px; padding: 10px 20px;
    background: var(--info-soft); color: var(--text); border-bottom: 1px solid var(--line); font-size: 13px;
  }
  .top-banner .link { background: none; border: none; color: var(--info); padding: 0; font-size: 13px; }
  .news { display: flex; flex-direction: column; gap: 4px; min-width: 0; }
  .news pre { margin: 0; white-space: pre-wrap; font: inherit; color: var(--text-2); max-height: 120px; overflow-y: auto; user-select: text; }
  .banner { margin: 12px 20px 0; font-size: 12px; color: var(--on-air-text); }
</style>
