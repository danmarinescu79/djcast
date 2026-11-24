<script>
  import { invoke } from '@tauri-apps/api/core';
  import { open } from '@tauri-apps/plugin-dialog';
  import { onMount } from 'svelte';
  import { t, LANGUAGES } from './lib/i18n.svelte.js';
  import { getVersion } from '@tauri-apps/api/app';
  import { isEnabled as autostartIsEnabled, enable as autostartEnable, disable as autostartDisable } from '@tauri-apps/plugin-autostart';
  import { checkForUpdate, installUpdate } from './lib/updates.js';

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

  let { config, live, systemLocale = 'en', initialTab = 'general', onSaved, onClose } = $props();

  // Edit a deep copy taken when the screen opens; nothing changes until Save.
  // svelte-ignore state_referenced_locally
  let draft = $state({ language: 'auto', ...structuredClone($state.snapshot(config)) });
  if (!draft.language) draft.language = 'auto';
  draft.app = { start_minimized: true, close_to_tray: false, auto_live: false, check_updates: true, ...(draft.app || {}) };

  // Start with the system is stored by the OS, not in the config file.
  let autostart = $state(false);
  let autostartInitial = false;
  let version = $state('');
  let folders = $state({ config: '', logs: '' });
  // Update check: idle | checking | none | found | installing | error
  let upd = $state({ state: 'idle', found: null, pct: 0, error: '' });
  // svelte-ignore state_referenced_locally
  let tab = $state(new URLSearchParams(globalThis.location?.search || '').get('tab') || initialTab);
  let devices = $state([]);
  let saving = $state(false);
  let message = $state({ text: '', ok: true });
  let testing = $state(false);
  let configPath = $state('');

  const tabs = ['general', 'audio', 'server', 'processing', 'nowplaying', 'recording', 'about'];

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
    const name = uniqueName(t('server.newName'));
    const server = { ...structuredClone($state.snapshot(draft.server)), host: '', password: '', mount: '/live' };
    draft.profiles.push({ name, server, bitrate: Number(draft.encoder.bitrate), live: false });
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
  let confirmingDelete = $state(false);

  function deleteProfile() {
    if (draft.profiles.length < 2) return;
    const removed = current;
    draft.profiles = draft.profiles.filter((p) => p.name !== removed);
    loadProfile(draft.profiles[0].name);
    confirmingDelete = false;
    message = { text: t('server.removed', { name: removed }), ok: true };
  }

  onMount(async () => {
    devices = await invoke('list_devices').catch(() => []);
    midiPorts = await invoke('list_midi_inputs').catch(() => []);
    configPath = await invoke('get_config_path');
    version = await getVersion().catch(() => '');
    folders = await invoke('app_folders').catch(() => folders);
    autostart = autostartInitial = await autostartIsEnabled().catch(() => false);
  });

  async function checkUpdates() {
    upd = { state: 'checking', found: null, pct: 0, error: '' };
    try {
      const found = await checkForUpdate();
      upd = { ...upd, state: found ? 'found' : 'none', found };
    } catch (e) {
      upd = { ...upd, state: 'error', error: String(e) };
    }
  }

  async function install() {
    upd = { ...upd, state: 'installing', pct: 0 };
    try {
      await installUpdate(upd.found, (pct) => (upd.pct = pct));
    } catch (e) {
      upd = { ...upd, state: 'error', error: String(e) };
    }
  }

  const openPage = (path) => invoke('open_project_page', { path }).catch(() => {});

  // Windows can list two devices with the same name (e.g. one per monitor): show each name once.
  const deviceOptions = $derived([
    ...new Set(devices.filter((d) => (draft.audio.mode === 'loopback' ? d.output : d.input)).map((d) => d.name)),
  ]);
  const midiOptions = $derived([...new Set(midiPorts)]);
  const midiSpecial = ['auto', 'none', ''];
  const liveCount = $derived(draft.profiles.filter((p) => p.live).length);
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
      if (autostart !== autostartInitial) {
        await (autostart ? autostartEnable() : autostartDisable());
        autostartInitial = autostart;
      }
      message = {
        text: live ? t('settings.savedLive') : t('settings.saved', { servers: liveNames(saved) }),
        ok: true,
      };
      onSaved(saved);
    } catch (e) {
      message = { text: String(e), ok: false };
    } finally {
      saving = false;
    }
  }

  function liveNames(cfg) {
    const live = cfg.profiles.filter((p) => p.live).map((p) => p.name);
    return (live.length ? live : [cfg.active_profile]).map((n) => `"${n}"`).join(', ');
  }

  async function test() {
    testing = true;
    message = { text: t('settings.connecting'), ok: true };
    try {
      draft.server.port = Number(draft.server.port);
      message = { text: await invoke('test_connection', { config: draft }), ok: true };
    } catch (e) {
      message = { text: String(e), ok: false };
    } finally {
      testing = false;
    }
  }

  const presets = ['off', 'gentle', 'radio', 'loud', 'custom'];
  const sources = ['auto', 'rekordbox-live', 'rekordbox-db', 'none'];
</script>

<div class="settings">
  <nav aria-label={t('settings.sections')}>
    <div class="nav-title">{t('app.settings')}</div>
    {#each tabs as id (id)}
      <button type="button" class="tab" class:active={tab === id} onclick={() => (tab = id)}>{t(`tab.${id}`)}</button>
    {/each}
    <div class="grow"></div>
    <div class="path" title={configPath}>{configPath}</div>
  </nav>

  <section class="body">
    {#if tab === 'audio'}
      <h1>{t('tab.audio')}</h1>
      <div class="grid">
        <div class="field">
          <label for="mode">{t('audio.capture')}</label>
          <select id="mode" bind:value={draft.audio.mode}>
            <option value="loopback">{t('audio.loopback')}</option>
            <option value="input">{t('audio.input')}</option>
          </select>
        </div>
        <div class="field">
          <label for="device">{t('audio.device')}</label>
          <select id="device" bind:value={draft.audio.device}>
            <option value="">{t('common.systemDefault')}</option>
            {#each deviceOptions as name (name)}<option value={name}>{name}</option>{/each}
            {#if draft.audio.device && !deviceOptions.includes(draft.audio.device)}
              <option value={draft.audio.device}>{draft.audio.device}</option>
            {/if}
          </select>
        </div>
        <div class="field">
          <label for="gain">{t('audio.gain', { v: Number(draft.audio.gain).toFixed(2) })}</label>
          <input id="gain" type="range" min="0.25" max="4" step="0.05" bind:value={draft.audio.gain} />
        </div>
      </div>
      <p class="note">{t('audio.asioNote')}</p>
    {:else if tab === 'server'}
      <div class="profile-bar">
        <h1>{t('tab.server')}</h1>
        <span class="grow"></span>
        {#if confirmingDelete}
          <span class="confirm" role="alert">{t('server.confirmDelete', { name: current })}</span>
          <button type="button" class="btn big danger" onclick={deleteProfile}>{t('common.delete')}</button>
          <button type="button" class="btn big" onclick={() => (confirmingDelete = false)}>{t('common.cancel')}</button>
        {:else if renaming}
          <form class="rename" onsubmit={finishRename}>
            <label for="pname" class="sr">{t('server.profileName')}</label>
            <input id="pname" bind:value={renameValue} />
            <button type="submit" class="btn big">OK</button>
          </form>
        {:else}
          <label for="profile">{t('server.profile')}</label>
          <select id="profile" class="profile-select" value={current} onchange={(e) => selectProfile(e.currentTarget.value)}>
            {#each draft.profiles as p (p.name)}
              <option value={p.name}>{p.name}{p.live ? ` · ${t('server.liveTag')}` : ''}</option>
            {/each}
          </select>
          <button type="button" class="btn big" onclick={newProfile}>{t('common.new')}</button>
          <button type="button" class="btn big" onclick={startRename}>{t('common.rename')}</button>
          <button
            type="button"
            class="btn big"
            disabled={draft.profiles.length < 2}
            title={draft.profiles.length < 2 ? t('server.lastProfile') : ''}
            onclick={() => (confirmingDelete = true)}>{t('common.delete')}</button>
        {/if}
      </div>
      <label class="check live-check">
        <input
          type="checkbox"
          checked={!!draft.profiles.find((p) => p.name === current)?.live}
          onchange={(e) => {
            const p = draft.profiles.find((x) => x.name === current);
            if (p) p.live = e.currentTarget.checked;
          }}
        />
        {t('server.streamHere')}
        <span class="hint">
          {liveCount > 1 ? t('server.liveCount', { n: liveCount }) : liveCount === 0 ? t('server.noneTicked') : ''}
        </span>
      </label>
      <div class="grid">
        <div class="field">
          <label for="proto">{t('server.type')}</label>
          <select id="proto" bind:value={draft.server.protocol}>
            <option value="icecast">Icecast 2</option>
            <option value="shoutcast2">Shoutcast v2 (DNAS 2)</option>
            <option value="shoutcast1">Shoutcast v1</option>
          </select>
        </div>
        <div class="field">
          <label for="br">{t('server.format')}</label>
          <select id="br" bind:value={draft.encoder.bitrate}>
            {#each [128, 160, 192, 256, 320] as b (b)}<option value={b}>MP3 · {b} kbps</option>{/each}
          </select>
        </div>
        <div class="field"><label for="host">{t('server.address')}</label><input id="host" bind:value={draft.server.host} /></div>
        <div class="pair">
          <div class="field"><label for="port">{t('server.port')}</label><input id="port" type="number" bind:value={draft.server.port} /></div>
          {#if draft.server.protocol === 'icecast'}
            <div class="field"><label for="mount">{t('server.mount')}</label><input id="mount" bind:value={draft.server.mount} /></div>
          {:else if draft.server.protocol === 'shoutcast2'}
            <div class="field"><label for="sid">{t('server.streamId')}</label><input id="sid" type="number" min="1" bind:value={draft.server.stream_id} /></div>
          {/if}
        </div>
        {#if draft.server.protocol === 'icecast'}
          <div class="field"><label for="user">{t('server.user')}</label><input id="user" bind:value={draft.server.user} /></div>
        {:else}
          <p class="note login-note">
            {draft.server.protocol === 'shoutcast2'
              ? t('server.shoutcast2Note', { port: Number(draft.server.port) + 1, sid: draft.server.stream_id || 1 })
              : t('server.shoutcast1Note', { port: Number(draft.server.port) + 1 })}
          </p>
        {/if}
        <div class="field">
          <label for="pass">{t('server.password')}</label>
          <div class="pass">
            <input id="pass" type={showPass ? 'text' : 'password'} bind:value={draft.server.password} autocomplete="off" spellcheck="false" />
            <button
              type="button"
              class="eye"
              aria-label={showPass ? t('server.hidePassword') : t('server.showPassword')}
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
        <div class="field"><label for="sname">{t('server.stationName')}</label><input id="sname" bind:value={draft.server.name} /></div>
        <div class="field"><label for="genre">{t('server.genre')}</label><input id="genre" bind:value={draft.server.genre} /></div>
        <div class="field"><label for="desc">{t('server.description')}</label><input id="desc" bind:value={draft.server.description} /></div>
        <div class="field"><label for="surl">{t('server.website')}</label><input id="surl" bind:value={draft.server.url} /></div>
        <label class="check"><input type="checkbox" bind:checked={draft.server.public} /> {t('server.public')}</label>
      </div>
    {:else if tab === 'processing'}
      <h1>{t('tab.processing')}</h1>
      <p class="note">{t('processing.intro')}</p>
      <div class="radios" role="radiogroup" aria-label={t('stats.preset')}>
        {#each presets as id (id)}
          <label class="radio"><input type="radio" name="preset" value={id} bind:group={draft.processing.preset} /> {t(`processing.preset.${id}`)}</label>
        {/each}
      </div>
      <label class="check sep-top">
        <input type="checkbox" bind:checked={draft.processing.multiband} disabled={draft.processing.preset === 'off'} />
        {t('processing.multiband')}
      </label>
      <p class="note">{t('processing.multibandNote')}</p>
      {#if draft.processing.preset === 'custom'}
        <div class="grid">
          <div class="field">
            <label for="target">{t('processing.target', { v: draft.processing.target_lufs })}</label>
            <input id="target" type="range" min="-23" max="-8" step="0.5" bind:value={draft.processing.target_lufs} />
          </div>
        </div>
      {/if}
    {:else if tab === 'nowplaying'}
      <h1>{t('tab.nowplaying')}</h1>
      <div class="radios" role="radiogroup" aria-label={t('np.source')}>
        {#each sources as id (id)}
          <label class="radio"><input type="radio" name="source" value={id} bind:group={draft.nowplaying.source} /> {t(`np.source.${id}`)}</label>
        {/each}
      </div>
      <div class="grid sep">
        <div class="field">
          <label for="confirm">{t('np.confirm', { v: draft.nowplaying.confirm_secs })}</label>
          <input id="confirm" type="range" min="0" max="15" step="0.5" bind:value={draft.nowplaying.confirm_secs} />
        </div>
        <div class="field">
          <label for="midi">{t('np.midi')}</label>
          <select id="midi" bind:value={draft.nowplaying.midi_input}>
            <option value="auto">{t('np.midiAuto')}</option>
            <option value="none">{t('np.midiOff')}</option>
            {#each midiOptions as name (name)}<option value={name}>{name}</option>{/each}
            {#if !midiSpecial.includes(draft.nowplaying.midi_input) && !midiOptions.includes(draft.nowplaying.midi_input)}
              <option value={draft.nowplaying.midi_input}>{draft.nowplaying.midi_input} ({t('np.notConnected')})</option>
            {/if}
          </select>
        </div>
        <div class="field">
          <label for="out">{t('np.folder')}</label>
          <div class="folder">
            <input id="out" bind:value={draft.nowplaying.output_dir} placeholder={t('np.folderOff')} />
            <button type="button" class="btn big" onclick={async () => {
              const p = await pickFolder(draft.nowplaying.output_dir, t('np.folder'));
              if (p) draft.nowplaying.output_dir = p;
            }}>{t('common.browse')}</button>
            <button type="button" class="btn big" disabled={!draft.nowplaying.output_dir} onclick={() => openFolder(draft.nowplaying.output_dir)}>{t('common.open')}</button>
          </div>
          <span class="hint">{t('common.relativeHint', { dir: baseDir })}</span>
        </div>
      </div>
    {:else if tab === 'recording'}
      <h1>{t('tab.recording')}</h1>
      <label class="check"><input type="checkbox" bind:checked={draft.record.enabled} /> {t('rec.enable')}</label>
      <div class="grid">
        <div class="field"><label for="rdir">{t('rec.folder')}</label><div class="folder">
            <input id="rdir" bind:value={draft.record.dir} placeholder="recordings" />
            <button type="button" class="btn big" onclick={async () => {
              const p = await pickFolder(draft.record.dir, t('rec.pickTitle'));
              if (p) draft.record.dir = p;
            }}>{t('common.browse')}</button>
            <button type="button" class="btn big" onclick={() => openFolder(draft.record.dir || 'recordings')}>{t('common.open')}</button>
          </div><span class="hint">{t('common.relativeHint', { dir: baseDir })}</span></div>
      </div>
    {:else if tab === 'general'}
      <h1>{t('tab.general')}</h1>
      <div class="grid">
        <div class="field">
          <label for="lang">{t('general.language')}</label>
          <select id="lang" bind:value={draft.language}>
            <option value="auto">{t('general.languageAuto', { lang: systemLocale })}</option>
            {#each LANGUAGES as l (l.code)}<option value={l.code}>{l.name}</option>{/each}
          </select>
          <span class="hint">{t('general.languageNote')}</span>
        </div>
      </div>

      <h2>{t('general.startup')}</h2>
      <div class="checks">
        <label class="check"><input type="checkbox" bind:checked={autostart} /> {t('general.autostart')}</label>
        <label class="check sub" class:dim={!autostart}>
          <input type="checkbox" bind:checked={draft.app.start_minimized} disabled={!autostart} /> {t('general.startMinimized')}
        </label>
        <label class="check"><input type="checkbox" bind:checked={draft.app.close_to_tray} /> {t('general.closeToTray')}</label>
        <span class="hint sub">{t('general.closeNote')}</span>
        <label class="check"><input type="checkbox" bind:checked={draft.app.auto_live} /> {t('general.autoLive')}</label>
        <span class="hint sub">{t('general.autoLiveNote')}</span>
      </div>

      <h2>{t('general.updates')}</h2>
      <div class="checks">
        <label class="check"><input type="checkbox" bind:checked={draft.app.check_updates} /> {t('general.checkUpdates')}</label>
        <div class="update-row">
          <button type="button" class="btn big" disabled={upd.state === 'checking' || upd.state === 'installing'} onclick={checkUpdates}>
            {t('general.checkNow')}
          </button>
          <span class="msg" class:bad={upd.state === 'error'} role="status">
            {#if upd.state === 'checking'}{t('general.checking')}
            {:else if upd.state === 'none'}{t('general.upToDate', { v: version })}
            {:else if upd.state === 'found'}{t('general.updateAvailable', { v: upd.found.version })}
            {:else if upd.state === 'installing'}{t('general.installing', { pct: upd.pct })}
            {:else if upd.state === 'error'}{t('general.updateError', { error: upd.error })}
            {/if}
          </span>
        </div>
        {#if upd.state === 'found' || upd.state === 'installing'}
          <div class="notes">
            <div class="notes-title">{t('general.whatsNew')} · {upd.found.version}</div>
            <pre>{upd.found.notes || '—'}</pre>
            <button type="button" class="primary" disabled={upd.state === 'installing' || live} title={live ? t('server.stopToTest') : ''} onclick={install}>
              {t('general.install')}
            </button>
          </div>
        {/if}
        <span class="hint sub">{t('general.updatesNote')}</span>
      </div>
    {:else if tab === 'about'}
      <h1>djcast</h1>
      {#if version}<p class="about-version">{t('about.version', { v: version })}</p>{/if}
      <p class="note">{t('about.tagline')}</p>
      <p class="note">{t('about.license')}</p>
      <div class="about-buttons">
        <button type="button" class="btn big" onclick={() => openPage('')}>{t('about.github')}</button>
        <button type="button" class="btn big" onclick={() => openPage('releases')}>{t('about.releases')}</button>
        <button type="button" class="btn big" onclick={() => openPage('issues/new')}>{t('about.issue')}</button>
      </div>
      <div class="about-buttons">
        <button type="button" class="btn big" onclick={() => openFolder(folders.logs)}>{t('about.logs')}</button>
        <button type="button" class="btn big" onclick={() => openFolder(folders.config)}>{t('about.settingsFolder')}</button>
      </div>
      <span class="hint">{t('about.logsNote')}</span>
      <p class="note disclaimer">{t('about.disclaimer')}</p>
      <p class="note copyright">© 2026 Dan Marinescu</p>
    {/if}

    <div class="grow"></div>
    <footer>
      <span class="msg" class:bad={!message.ok} role="status">{message.text}</span>
      <span class="grow"></span>
      {#if tab === 'server'}
        <button type="button" class="btn big" disabled={testing || live} title={live ? t('server.stopToTest') : ''} onclick={test}>{t('server.test')}</button>
      {/if}
      <button type="button" class="btn big" onclick={onClose}>{t('common.close')}</button>
      {#if tab !== 'about'}
        <button type="button" class="primary" disabled={saving} onclick={save}>{t('common.save')}</button>
      {/if}
    </footer>
  </section>
</div>

<style>
  .settings { flex-grow: 1; min-height: 0; display: flex; }
  nav { width: 220px; flex-shrink: 0; padding: 20px 12px; border-right: 1px solid var(--line); background: var(--bar); display: flex; flex-direction: column; gap: 4px; }
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
  .live-check { gap: 10px; }
  .live-check .hint { margin-left: 8px; }
  .confirm { font-size: 14px; color: var(--on-air-text); }
  .danger { border-color: var(--on-air) !important; color: var(--on-air-text) !important; }
  .danger:hover { background: var(--on-air-soft); }
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
  .sep-top { border-top: 1px solid var(--line); padding-top: 18px; }
  h2 { margin: 8px 0 0; font-size: 15px; font-weight: 600; color: var(--text-2); border-top: 1px solid var(--line); padding-top: 18px; }
  .checks { display: flex; flex-direction: column; gap: 10px; }
  .checks .sub { margin-left: 28px; }
  .dim { opacity: 0.55; }
  .update-row { display: flex; align-items: center; gap: 12px; }
  .notes { background: var(--panel); border: 1px solid var(--line); border-radius: 10px; padding: 14px; display: flex; flex-direction: column; gap: 10px; max-width: 640px; }
  .notes-title { font-weight: 600; }
  .notes pre { margin: 0; white-space: pre-wrap; font: inherit; font-size: 13px; color: var(--text-2); max-height: 180px; overflow-y: auto; user-select: text; }
  .notes .primary { align-self: flex-start; }
  .about-version { margin: -8px 0 0; color: var(--muted); }
  .about-buttons { display: flex; flex-wrap: wrap; gap: 10px; }
  .disclaimer { font-size: 12px; margin-top: 8px; }
  .copyright { font-size: 12px; margin-top: -8px; }
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
