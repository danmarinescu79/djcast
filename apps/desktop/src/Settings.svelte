<script>
  import { invoke } from '@tauri-apps/api/core';
  import { onMount } from 'svelte';

  let { config, live, onSaved, onClose } = $props();

  // Edit a deep copy; nothing changes until Save.
  let draft = $state(structuredClone($state.snapshot(config)));
  let tab = $state('server');
  let devices = $state([]);
  let saving = $state(false);
  let message = $state({ text: '', ok: true });
  let testing = $state(false);
  let configPath = $state('');

  const tabs = [
    ['audio', 'Audio input'],
    ['server', 'Server'],
    ['processing', 'Sound processing'],
    ['nowplaying', 'Now playing'],
    ['recording', 'Recording'],
  ];

  onMount(async () => {
    devices = await invoke('list_devices').catch(() => []);
    configPath = await invoke('get_config_path');
  });

  const deviceOptions = $derived(
    devices.filter((d) => (draft.audio.mode === 'loopback' ? d.output : d.input)).map((d) => d.name)
  );

  async function save() {
    saving = true;
    try {
      // Number inputs may hand back strings; normalize before saving.
      draft.server.port = Number(draft.server.port);
      draft.encoder.bitrate = Number(draft.encoder.bitrate);
      draft.nowplaying.confirm_secs = Number(draft.nowplaying.confirm_secs);
      draft.processing.target_lufs = Number(draft.processing.target_lufs);
      await invoke('save_config', { config: draft });
      message = { text: live ? 'Saved. Audio and server changes apply the next time you go live.' : 'Saved.', ok: true };
      onSaved(structuredClone($state.snapshot(draft)));
    } catch (e) {
      message = { text: String(e), ok: false };
    } finally {
      saving = false;
    }
  }

  async function test() {
    testing = true;
    message = { text: 'Connecting…', ok: true };
    try {
      draft.server.port = Number(draft.server.port);
      message = { text: await invoke('test_connection', { config: draft }), ok: true };
    } catch (e) {
      message = { text: String(e), ok: false };
    } finally {
      testing = false;
    }
  }

  const presets = [
    ['off', 'Off — send the mix as it is'],
    ['gentle', 'Gentle — -16 LUFS, very transparent'],
    ['radio', 'Radio — -13 LUFS, typical internet radio'],
    ['loud', 'Loud — -10 LUFS, like heavily processed stations'],
    ['custom', 'Custom target'],
  ];
  const sources = [
    ['auto', 'Automatic — instant, falls back to the history'],
    ['rekordbox-live', 'rekordbox, instant (window + controller MIDI)'],
    ['rekordbox-db', 'rekordbox history (about 1 minute late)'],
    ['none', 'Off — type titles by hand'],
  ];
</script>

<div class="settings">
  <nav aria-label="Settings sections">
    <div class="nav-title">Settings</div>
    {#each tabs as [id, label] (id)}
      <button type="button" class="tab" class:active={tab === id} onclick={() => (tab = id)}>{label}</button>
    {/each}
    <div class="grow"></div>
    <div class="path" title={configPath}>{configPath}</div>
  </nav>

  <section class="body">
    {#if tab === 'audio'}
      <h1>Audio input</h1>
      <div class="grid">
        <div class="field">
          <label for="mode">Capture</label>
          <select id="mode" bind:value={draft.audio.mode}>
            <option value="loopback">What an output plays (loopback)</option>
            <option value="input">An input (line-in, mixer USB, virtual cable)</option>
          </select>
        </div>
        <div class="field">
          <label for="device">Device</label>
          <select id="device" bind:value={draft.audio.device}>
            <option value="">System default</option>
            {#each deviceOptions as name (name)}<option value={name}>{name}</option>{/each}
            {#if draft.audio.device && !deviceOptions.includes(draft.audio.device)}
              <option value={draft.audio.device}>{draft.audio.device}</option>
            {/if}
          </select>
        </div>
        <div class="field">
          <label for="gain">Input gain ({draft.audio.gain.toFixed(2)}×)</label>
          <input id="gain" type="range" min="0.25" max="4" step="0.05" bind:value={draft.audio.gain} />
        </div>
      </div>
      <p class="note">
        With rekordbox on ASIO, turn on PC MASTER OUT in rekordbox's audio preferences, send it to a spare
        output (for example Realtek Digital Output) and capture that output here.
      </p>
    {:else if tab === 'server'}
      <h1>Server</h1>
      <div class="grid">
        <div class="field">
          <label for="proto">Server type</label>
          <select id="proto" bind:value={draft.server.protocol}><option value="icecast">Icecast 2</option></select>
        </div>
        <div class="field">
          <label for="br">Format</label>
          <select id="br" bind:value={draft.encoder.bitrate}>
            {#each [128, 160, 192, 256, 320] as b (b)}<option value={b}>MP3 · {b} kbps</option>{/each}
          </select>
        </div>
        <div class="field"><label for="host">Address</label><input id="host" bind:value={draft.server.host} /></div>
        <div class="pair">
          <div class="field"><label for="port">Port</label><input id="port" type="number" bind:value={draft.server.port} /></div>
          <div class="field"><label for="mount">Mount point</label><input id="mount" bind:value={draft.server.mount} /></div>
        </div>
        <div class="field"><label for="user">User</label><input id="user" bind:value={draft.server.user} /></div>
        <div class="field"><label for="pass">Password</label><input id="pass" type="password" bind:value={draft.server.password} /></div>
      </div>
      <div class="grid sep">
        <div class="field"><label for="sname">Station name</label><input id="sname" bind:value={draft.server.name} /></div>
        <div class="field"><label for="genre">Genre</label><input id="genre" bind:value={draft.server.genre} /></div>
        <div class="field"><label for="desc">Description</label><input id="desc" bind:value={draft.server.description} /></div>
        <div class="field"><label for="surl">Website</label><input id="surl" bind:value={draft.server.url} /></div>
        <label class="check"><input type="checkbox" bind:checked={draft.server.public} /> List in the public directory</label>
      </div>
    {:else if tab === 'processing'}
      <h1>Sound processing</h1>
      <p class="note">
        Makes the stream as loud and clear as other stations in players that don't adjust volume
        (Second Life, web players). A slow automatic gain brings the mix to the target loudness;
        a limiter keeps peaks below the ceiling.
      </p>
      <div class="radios" role="radiogroup" aria-label="Preset">
        {#each presets as [id, label] (id)}
          <label class="radio"><input type="radio" name="preset" value={id} bind:group={draft.processing.preset} /> {label}</label>
        {/each}
      </div>
      {#if draft.processing.preset === 'custom'}
        <div class="grid">
          <div class="field">
            <label for="target">Target loudness ({draft.processing.target_lufs} LUFS)</label>
            <input id="target" type="range" min="-23" max="-8" step="0.5" bind:value={draft.processing.target_lufs} />
          </div>
        </div>
      {/if}
    {:else if tab === 'nowplaying'}
      <h1>Now playing</h1>
      <div class="radios" role="radiogroup" aria-label="Source">
        {#each sources as [id, label] (id)}
          <label class="radio"><input type="radio" name="source" value={id} bind:group={draft.nowplaying.source} /> {label}</label>
        {/each}
      </div>
      <div class="grid sep">
        <div class="field">
          <label for="confirm">Switch the title after ({draft.nowplaying.confirm_secs} s)</label>
          <input id="confirm" type="range" min="0" max="15" step="0.5" bind:value={draft.nowplaying.confirm_secs} />
        </div>
        <div class="field">
          <label for="midi">Controller MIDI input</label>
          <input id="midi" bind:value={draft.nowplaying.midi_input} placeholder="auto" />
        </div>
        <div class="field">
          <label for="out">Folder for OBS text files and tracklists</label>
          <input id="out" bind:value={draft.nowplaying.output_dir} placeholder="empty = off" />
        </div>
      </div>
    {:else if tab === 'recording'}
      <h1>Recording</h1>
      <label class="check"><input type="checkbox" bind:checked={draft.record.enabled} /> Record every session to an MP3 file</label>
      <div class="grid">
        <div class="field"><label for="rdir">Folder</label><input id="rdir" bind:value={draft.record.dir} placeholder="recordings" /></div>
      </div>
    {/if}

    <div class="grow"></div>
    <footer>
      <span class="msg" class:bad={!message.ok} role="status">{message.text}</span>
      <span class="grow"></span>
      {#if tab === 'server'}
        <button type="button" class="btn big" disabled={testing || live} title={live ? 'Stop streaming to test' : ''} onclick={test}>Test connection</button>
      {/if}
      <button type="button" class="btn big" onclick={onClose}>Close</button>
      <button type="button" class="primary" disabled={saving} onclick={save}>Save</button>
    </footer>
  </section>
</div>

<style>
  .settings { flex-grow: 1; min-height: 0; display: flex; }
  nav { width: 220px; padding: 20px 12px; border-right: 1px solid var(--line); background: var(--bar); display: flex; flex-direction: column; gap: 4px; }
  .nav-title { font-weight: 700; font-size: 17px; padding: 0 10px 16px; }
  .tab { text-align: left; padding: 10px 12px; border-radius: 8px; border: none; background: transparent; color: var(--muted); font-size: 14px; }
  .tab:hover { color: var(--text); }
  .tab.active { background: #22252c; color: var(--text); }
  .path { font-size: 11px; color: var(--muted); padding: 0 10px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; user-select: text; }
  .body { flex-grow: 1; min-width: 0; padding: 24px 32px; display: flex; flex-direction: column; gap: 18px; overflow-y: auto; }
  h1 { margin: 0; font-size: 22px; }
  .grid { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 16px 20px; }
  .grid.sep { border-top: 1px solid var(--line); padding-top: 18px; }
  .pair { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 12px; }
  .field { display: flex; flex-direction: column; gap: 6px; }
  label { font-size: 12px; color: var(--muted); }
  input:not([type]), input[type='number'], input[type='password'], select {
    height: 40px; padding: 0 12px; background: var(--panel); color: var(--text);
    border: 1px solid var(--line-strong); border-radius: 8px; font-size: 14px; width: 100%;
  }
  input[type='range'] { width: 100%; accent-color: var(--info); }
  .check, .radio { display: flex; align-items: center; gap: 10px; font-size: 14px; color: var(--text); }
  .radios { display: flex; flex-direction: column; gap: 12px; }
  input[type='checkbox'], input[type='radio'] { width: 18px; height: 18px; accent-color: var(--info); }
  .note { margin: 0; font-size: 13px; color: var(--muted); max-width: 640px; }
  footer { display: flex; align-items: center; gap: 12px; }
  .msg { font-size: 13px; color: var(--ok); user-select: text; }
  .msg.bad { color: var(--on-air-text); }
  .grow { flex-grow: 1; }
  .btn.big { height: 40px; padding: 0 16px; font-size: 14px; border-radius: 8px; }
  .primary { height: 40px; padding: 0 18px; background: var(--text); color: var(--bg); border: none; border-radius: 8px; font-size: 14px; font-weight: 600; }
</style>
