<script>
  import { invoke } from '@tauri-apps/api/core';
  import { listen } from '@tauri-apps/api/event';
  import { onMount } from 'svelte';
  import Console from './Console.svelte';
  import Settings from './Settings.svelte';

  let view = $state('console');
  let status = $state(null);
  let config = $state(null);
  let busy = $state(false);
  let actionError = $state('');

  onMount(() => {
    invoke('get_config').then((c) => (config = c));
    invoke('status').then((s) => (status = s));
    const unlisten = listen('status', (e) => (status = e.payload));
    return () => unlisten.then((f) => f());
  });

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

  const server = $derived(
    config ? `${config.server.host}:${config.server.port}${config.server.mount}` : ''
  );
  const format = $derived(
    config ? `${config.server.protocol === 'icecast' ? 'Icecast' : config.server.protocol} · MP3 ${config.encoder.bitrate} kbps` : ''
  );
</script>

<div class="app">
  <header>
    <div class="brand">djcast</div>
    <div class="chip mono">{server}</div>
    <div class="sub">{format}{status?.sample_rate ? ` · ${status.sample_rate / 1000} kHz` : ''}</div>
    <div class="grow"></div>
    {#if status?.live}<span class="live-pill">LIVE</span>{/if}
    <button class="btn" type="button" onclick={() => (view = view === 'settings' ? 'console' : 'settings')}>
      {view === 'settings' ? 'Back' : 'Settings'}
    </button>
  </header>

  {#if status && config && view === 'settings'}
    <Settings {config} live={status.live} onSaved={(c) => (config = c)} onClose={() => (view = 'console')} />
  {:else if status && config}
    <Console {status} {busy} {actionError} onToggleLive={toggleLive} onSendTitle={sendTitle} />
  {:else}
    <div class="loading">Starting…</div>
  {/if}
</div>

<style>
  .app { height: 100%; display: flex; flex-direction: column; }
  header {
    height: 52px; padding: 0 20px; display: flex; align-items: center; gap: 16px;
    border-bottom: 1px solid var(--line); background: var(--bar);
  }
  .brand { font-weight: 700; font-size: 17px; letter-spacing: -0.2px; }
  .chip { font-size: 12px; color: var(--muted); padding: 5px 10px; border: 1px solid var(--line-strong); border-radius: 6px; }
  .sub { font-size: 12px; color: var(--muted); }
  .grow { flex-grow: 1; }
  .loading { margin: auto; color: var(--muted); }
  .live-pill { font-size: 11px; font-weight: 700; letter-spacing: 1px; padding: 4px 10px; border-radius: 999px; background: var(--on-air); color: #fff; }
</style>
