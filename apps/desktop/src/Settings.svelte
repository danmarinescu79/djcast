<script>
  import { invoke } from '@tauri-apps/api/core';
  import { open } from '@tauri-apps/plugin-dialog';
  import { onMount } from 'svelte';

  /** Folder picker; returns the chosen absolute path or null. */
  async function pickFolder(current, title) {
    const isAbs = /^([a-zA-Z]:[\\/]|\/)/.test(current || '');
    const chosen = await open({ directory: true, title, defaultPath: isAbs ? current : baseDir });
    return typeof chosen === 'string' ? chosen : null;
  }

  async function openFolder(path) {
    try {
      await invoke('open_folder', { path: path || '.' });
    } catch (e) {
      message = { text: String(e), ok: false };
    }
  }

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

  let midiPorts = $state([]);

  // ---- server profiles: the form edits `draft.server`; profiles keep their own copy ----
  let current = $state(draft.active_profile || draft.profiles?.[0]?.name || 'Default');
  let renaming = $state(false);
  let renameValue = $state('');
  let showPass = $state(false);

  function storeCurrent() {
    const p = draft.profiles.find((x) => x.name === current);
    if (p) {
      p.server = structuredClone($state.snapshot(draft.server));
      p.bitrate = Number(draft.encoder.bitrate);
    }
  }
  function loadProfile(name) {
    const p = draft.profiles.find((x) => x.name === name);
    if (!p) return;
    current = name;
    draft.server = structuredClone($state.snapshot(p.server));
    draft.encoder.bitrate = p.bitrate;
    showPass = false;
  }
  function selectProfile(name) {
    storeCurrent();
    loadProfile(name);
  }
  function uniqueName(base) {
    let n = 1;
    let name = base;
    while (draft.profiles.some((p) => p.name === name)) name = `${base} ${++n}`;
    return name;
  }
  function newProfile() {
    storeCurrent();
    const name = uniqueName('New server');
    const server = { ...structuredClone($state.snapshot(draft.server)), host: '', password: '', mount: '/live' };
    draft.profiles.push({ name, server, bitrate: Number(draft.encoder.bitrate) });
    loadProfile(name);
    startRename();
  }
  function startRename() {
    renameValue = current;
    renaming = true;
  }
  function finishRename(e) {
    e?.preventDefault();
    const name = renameValue.trim();
    if (name && name !== current && !draft.profiles.some((p) => p.name === name)) {
      const p = draft.profiles.find((x) => x.name === current);
      p.name = name;
      current = name;
    }
    renaming = false;
  }
  function deleteProfile() {
    if (draft.profiles.length < 2) return;
    draft.profiles = draft.profiles.filter((p) => p.name !== current);
    loadProfile(draft.profiles[0].name);
  }

  onMount(async () => {
    devices = await invoke('list_devices').catch(() => []);
    midiPorts = await invoke('list_midi_inputs').catch(() => []);
    configPath = await invoke('get_config_path');
  });

  // Windows can list two devices with the same name (e.g. one per monitor): show each name once.
  const deviceOptions = $derived([
    ...new Set(devices.filter((d) => (draft.audio.mode === 'loopback' ? d.output : d.input)).map((d) => d.name)),
  ]);
  const midiOptions = $derived([...new Set(midiPorts)]);
  const midiSpecial = ['auto', 'none', ''];
  const baseDir = $derived(configPath.replace(/[\\/][^\\/]*$/, ''));

  async function save() {
    saving = true;
    try {
      // Number inputs may hand back strings; normalize before saving.
      draft.server.port = Number(draft.server.port);
      draft.server.stream_id = Number(draft.server.stream_id) || 1;
      draft.encoder.bitrate = Number(draft.encoder.bitrate);
      draft.nowplaying.confirm_secs = Number(draft.nowplaying.confirm_secs);
      draft.processing.target_lufs = Number(draft.processing.target_lufs);
      if (renaming) finishRename();
      storeCurrent();
      draft.active_profile = current;
      const saved = await invoke('save_config', { config: draft });
      message = {
        text: live
          ? `Saved. "${current}" and audio changes apply the next time you go live.`
          : `Saved. Going live uses "${current}".`,
        ok: true,
      };
      onSaved(saved);
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
      <div class="profile-bar">
        <h1>Server</h1>
        <span class="grow"></span>
        {#if renaming}
          <form class="rename" onsubmit={finishRename}>
            <label for="pname" class="sr">Profile name</label>
            <input id="pname" bind:value={renameValue} />
            <button type="submit" class="btn big">OK</button>
          </form>
        {:else}
          <label for="profile">Profile</label>
          <select id="profile" class="profile-select" value={current} onchange={(e) => selectProfile(e.currentTarget.value)}>
            {#each draft.profiles as p (p.name)}
              <option value={p.name}>{p.name}{p.name === draft.active_profile ? ' (in use)' : ''}</option>
            {/each}
          </select>
          <button type="button" class="btn big" onclick={newProfile}>New</button>
          <button type="button" class="btn big" onclick={startRename}>Rename</button>
          <button type="button" class="btn big" disabled={draft.profiles.length < 2} onclick={deleteProfile}>Delete</button>
        {/if}
      </div>
      <div class="grid">
        <div class="field">
          <label for="proto">Server type</label>
          <select id="proto" bind:value={draft.server.protocol}>
            <option value="icecast">Icecast 2</option>
            <option value="shoutcast2">Shoutcast v2 (DNAS 2)</option>
            <option value="shoutcast1">Shoutcast v1</option>
          </select>
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
          {#if draft.server.protocol === 'icecast'}
            <div class="field"><label for="mount">Mount point</label><input id="mount" bind:value={draft.server.mount} /></div>
          {:else if draft.server.protocol === 'shoutcast2'}
            <div class="field"><label for="sid">Stream ID</label><input id="sid" type="number" min="1" bind:value={draft.server.stream_id} /></div>
          {/if}
        </div>
        {#if draft.server.protocol === 'icecast'}
          <div class="field"><label for="user">User</label><input id="user" bind:value={draft.server.user} /></div>
        {:else}
          <p class="note login-note">
            Shoutcast sends audio to port {Number(draft.server.port) + 1} and logs in with the password only{draft.server.protocol === 'shoutcast2' ? `, as password:#${draft.server.stream_id || 1}` : ''}.
          </p>
        {/if}
        <div class="field">
          <label for="pass">Password</label>
          <div class="pass">
            <input id="pass" type={showPass ? 'text' : 'password'} bind:value={draft.server.password} autocomplete="off" spellcheck="false" />
            <button
              type="button"
              class="eye"
              aria-label={showPass ? 'Hide password' : 'Show password'}
              aria-pressed={showPass}
              onclick={() => (showPass = !showPass)}
            >
              {#if showPass}
                <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M17.94 17.94A10.07 10.07 0 0 1 12 20c-7 0-11-8-11-8a18.45 18.45 0 0 1 5.06-5.94"></path><path d="M9.9 4.24A9.12 9.12 0 0 1 12 4c7 0 11 8 11 8a18.5 18.5 0 0 1-2.16 3.19"></path><path d="M14.12 14.12a3 3 0 1 1-4.24-4.24"></path><path d="M1 1l22 22"></path></svg>
              {:else}
                <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M1 12s4-8 11-8 11 8 11 8-4 8-11 8-11-8-11-8z"></path><circle cx="12" cy="12" r="3"></circle></svg>
              {/if}
            </button>
          </div>
        </div>
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
          <label for="midi">Controller MIDI input (faders and crossfader)</label>
          <select id="midi" bind:value={draft.nowplaying.midi_input}>
            <option value="auto">Automatic (first Pioneer DDJ found)</option>
            <option value="none">Off — use deck play state only</option>
            {#each midiOptions as name (name)}<option value={name}>{name}</option>{/each}
            {#if !midiSpecial.includes(draft.nowplaying.midi_input) && !midiOptions.includes(draft.nowplaying.midi_input)}
              <option value={draft.nowplaying.midi_input}>{draft.nowplaying.midi_input} (not connected)</option>
            {/if}
          </select>
        </div>
        <div class="field">
          <label for="out">Folder for OBS text files and tracklists</label>
          <div class="folder">
            <input id="out" bind:value={draft.nowplaying.output_dir} placeholder="empty = off" />
            <button type="button" class="btn big" onclick={async () => {
              const p = await pickFolder(draft.nowplaying.output_dir, 'Folder for OBS text files and tracklists');
              if (p) draft.nowplaying.output_dir = p;
            }}>Browse…</button>
            <button type="button" class="btn big" disabled={!draft.nowplaying.output_dir} onclick={() => openFolder(draft.nowplaying.output_dir)}>Open</button>
          </div>
          <span class="hint">A name without a drive letter is created next to the settings file: {baseDir}</span>
        </div>
      </div>
    {:else if tab === 'recording'}
      <h1>Recording</h1>
      <label class="check"><input type="checkbox" bind:checked={draft.record.enabled} /> Record every session to an MP3 file</label>
      <div class="grid">
        <div class="field"><label for="rdir">Folder</label><div class="folder">
            <input id="rdir" bind:value={draft.record.dir} placeholder="recordings" />
            <button type="button" class="btn big" onclick={async () => {
              const p = await pickFolder(draft.record.dir, 'Folder for recordings');
              if (p) draft.record.dir = p;
            }}>Browse…</button>
            <button type="button" class="btn big" onclick={() => openFolder(draft.record.dir || 'recordings')}>Open</button>
          </div><span class="hint">A name without a drive letter is created in {baseDir}</span></div>
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
  input:not([type]), input[type='text'], input[type='number'], input[type='password'], select {
    height: 40px; padding: 0 12px; background: var(--panel); color: var(--text);
    border: 1px solid var(--line-strong); border-radius: 8px; font-size: 14px; width: 100%;
  }
  input[type='range'] { width: 100%; accent-color: var(--info); }
  .check, .radio { display: flex; align-items: center; gap: 10px; font-size: 14px; color: var(--text); }
  .radios { display: flex; flex-direction: column; gap: 12px; }
  input[type='checkbox'], input[type='radio'] { width: 18px; height: 18px; accent-color: var(--info); }
  .profile-bar { display: flex; align-items: center; gap: 10px; }
  .profile-select { width: 220px !important; }
  .rename { display: flex; gap: 8px; align-items: center; }
  .rename input { width: 220px; }
  .sr { position: absolute; width: 1px; height: 1px; overflow: hidden; clip: rect(0 0 0 0); }
  .login-note { align-self: end; padding-bottom: 10px; }
  .pass { position: relative; }
  .pass input { padding-right: 44px; }
  .eye {
    position: absolute; right: 4px; top: 4px; width: 32px; height: 32px;
    display: flex; align-items: center; justify-content: center;
    background: transparent; border: none; border-radius: 6px; color: var(--muted);
  }
  .eye:hover { color: var(--text); background: #22252c; }
  .folder { display: flex; gap: 8px; }
  .folder input { flex: 1 1 auto; min-width: 0; }
  .folder .btn { flex-shrink: 0; }
  .hint { font-size: 12px; color: var(--muted); user-select: text; overflow-wrap: anywhere; }
  .note { margin: 0; font-size: 13px; color: var(--muted); max-width: 640px; }
  footer { display: flex; align-items: center; gap: 12px; }
  .msg { font-size: 13px; color: var(--ok); user-select: text; }
  .msg.bad { color: var(--on-air-text); }
  .grow { flex-grow: 1; }
  .btn.big { height: 40px; padding: 0 16px; white-space: nowrap; flex-shrink: 0; font-size: 14px; border-radius: 8px; }
  .primary { height: 40px; padding: 0 18px; background: var(--text); color: var(--bg); border: none; border-radius: 8px; font-size: 14px; font-weight: 600; }
</style>
