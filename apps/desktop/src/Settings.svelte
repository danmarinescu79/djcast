<script>
  import { invoke } from '@tauri-apps/api/core';
  // The webview's own clipboard API is blocked on macOS; the plugin writes natively.
  import { writeText } from '@tauri-apps/plugin-clipboard-manager';
  import { open } from '@tauri-apps/plugin-dialog';
  import { onMount } from 'svelte';
  import { t, LANGUAGES } from './lib/i18n.svelte.js';
  import { getVersion } from '@tauri-apps/api/app';
  import { isEnabled as autostartIsEnabled, enable as autostartEnable, disable as autostartDisable } from '@tauri-apps/plugin-autostart';
  import { checkForUpdate, installUpdate } from './lib/updates.js';
  import { BITRATES, nearestBitrate } from './lib/format.js';

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
  draft.app = { start_minimized: true, close_to_tray: false, auto_live: false, check_updates: true, theme: 'system', ...(draft.app || {}) };
  draft.processing = { eq_bass_db: 0, eq_presence_db: 0, ...draft.processing };
  draft.nowplaying = {
    midi_mapping: 'pioneer-ddj',
    crossfader_assign: ['thru', 'thru', 'thru', 'thru'],
    ...draft.nowplaying,
  };
  draft.nowplaying.midi_custom = {
    faders: [0, 1, 2, 3].map(() => ({ channel: 0, cc: 0 })), crossfader: { channel: 0, cc: 0 }, crossfader_reversed: false,
    ...(draft.nowplaying.midi_custom || {}),
  };

  // The device field means a device name, or a program name in "process" mode: keep one per mode.
  const lastDevice = {};
  function setMode(mode) {
    lastDevice[draft.audio.mode] = draft.audio.device;
    draft.audio.mode = mode;
    draft.audio.device = lastDevice[mode] ?? '';
  }
  const signed = (v) => (Number(v) > 0 ? `+${Number(v)}` : `${Number(v)}`);

  // ---- MIDI learn: slot 0..3 = channel faders, 4 = crossfader ----
  let learning = $state(-1);
  let learnMsg = $state({ text: '', ok: true });
  const unset = { channel: 0, cc: 0 };
  const mapped = (c) => c && c.channel > 0;
  const controlLabel = (c) =>
    !mapped(c) ? t('np.notMapped') : c.lsb != null ? t('np.control14', { ch: c.channel, cc: c.cc, lsb: c.lsb }) : t('np.control', { ch: c.channel, cc: c.cc });
  const slotControl = (slot) => (slot < 4 ? draft.nowplaying.midi_custom.faders[slot] : draft.nowplaying.midi_custom.crossfader);
  function setSlot(slot, value) {
    if (slot < 4) draft.nowplaying.midi_custom.faders[slot] = value;
    else draft.nowplaying.midi_custom.crossfader = value;
  }
  async function learn(slot) {
    learning = slot;
    learnMsg = { text: t('np.learning'), ok: true };
    try {
      setSlot(slot, await invoke('midi_learn', { port: draft.nowplaying.midi_input }));
      learnMsg = { text: t('np.learned'), ok: true };
    } catch (e) {
      learnMsg = { text: String(e), ok: false };
    } finally {
      learning = -1;
    }
  }
  draft.share = {
    discord_webhook: '', listen_url: '', on_live: true, on_track: false, on_stop: true,
    live_message: '🔴 **{station}** is live! {track}\n{url}', track_message: '🎵 {track}',
    stop_message: '⏹ **{station}** is off air. Tracklist:\n{tracklist}', ...(draft.share || {}),
  };
  let showHook = $state(false);
  let discordMsg = $state({ text: '', ok: true });
  let sendingTest = $state(false);
  async function testDiscord() {
    sendingTest = true;
    discordMsg = { text: t('settings.connecting'), ok: true };
    try {
      await invoke('test_discord', { share: $state.snapshot(draft.share) });
      discordMsg = { text: t('share.testSent'), ok: true };
    } catch (e) {
      discordMsg = { text: String(e), ok: false };
    } finally {
      sendingTest = false;
    }
  }

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

  const tabs = ['general', 'audio', 'server', 'processing', 'nowplaying', 'recording', 'share', 'about'];

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
      p.codec = draft.encoder.codec;
      p.bitrate = Number(draft.encoder.bitrate);
    }
  }
  function loadProfile(name) {
    const p = draft.profiles.find((x) => x.name === name);
    if (!p) return;
    current = name;
    draft.server = structuredClone($state.snapshot(p.server));
    draft.encoder.codec = p.codec || 'mp3';
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
    draft.profiles.push({ name, server, codec: draft.encoder.codec, bitrate: Number(draft.encoder.bitrate), live: false });
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

  // Setup report for testers: collected by the app (listens to the input for a few seconds).
  let report = $state({ busy: false, text: '', copied: false });
  async function collectReport() {
    report = { busy: true, text: '', copied: false };
    try {
      report.text = await invoke('setup_report');
    } catch (e) {
      report.text = String(e);
    } finally {
      report.busy = false;
    }
  }
  async function copyReport() {
    await writeText(report.text);
    report.copied = true;
    setTimeout(() => (report.copied = false), 2000);
  }

  // Windows can list two devices with the same name (e.g. one per monitor): show each name once.
  const deviceOptions = $derived([
    ...new Set(devices.filter((d) => (draft.audio.mode === 'loopback' ? d.output : d.input)).map((d) => d.name)),
  ]);
  const isMac = navigator.userAgent.includes('Mac');
  // Inputs with more than two channels (e.g. a DJ controller): which stereo pair to capture.
  const inputDevice = $derived(devices.find((d) => d.input && d.name === draft.audio.device));
  const channelPairs = $derived(
    Array.from({ length: Math.floor((inputDevice?.channels ?? 0) / 2) }, (_, i) => 2 * i + 1),
  );
  const pairLabel = (first) => `${first}–${first + 1}`;

  // "Detect": listen to every pair for a few seconds while music plays, select the one with sound.
  let detect = $state({ busy: false, result: '' });
  async function detectChannels() {
    detect = { busy: true, result: '' };
    try {
      const r = await invoke('detect_channels', { device: draft.audio.device });
      if (r.best) {
        draft.audio.first_channel = r.best;
        const peak = r.levels.find((l) => l.first === r.best).peak_db;
        detect.result = t('audio.detectFound', { pair: pairLabel(r.best), db: Math.round(peak) });
      } else {
        detect.result = t('audio.detectNone');
      }
    } catch (e) {
      detect.result = String(e);
    } finally {
      detect.busy = false;
    }
  }
  const midiOptions = $derived([...new Set(midiPorts)]);
  const midiSpecial = ['auto', 'none', ''];
  const liveCount = $derived(draft.profiles.filter((p) => p.live).length);

  function setCodec(codec) {
    draft.encoder.codec = codec;
    draft.encoder.bitrate = nearestBitrate(codec, Number(draft.encoder.bitrate));
  }
  // Opus needs Icecast; AAC and Opus are not for every player.
  const opusOnShoutcast = $derived(draft.encoder.codec === 'opus' && draft.server.protocol !== 'icecast');
  const formatHint = $derived(
    opusOnShoutcast ? t('format.opusShoutcast') : draft.encoder.codec === 'mp3' ? '' : t(`format.${draft.encoder.codec}Hint`)
  );
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
      draft.processing.eq_bass_db = Number(draft.processing.eq_bass_db);
      draft.processing.eq_presence_db = Number(draft.processing.eq_presence_db);
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
  const sources = ['rekordbox-live', 'serato', 'text-file', 'none'];
  // Faders, crossfader and the switch delay only matter for the sources that follow the decks.
  const usesMidi = $derived(['rekordbox-live', 'serato'].includes(draft.nowplaying.source));

  async function pickTextFile() {
    const chosen = await open({
      title: t('np.textFile'),
      defaultPath: draft.nowplaying.text_file || undefined,
      filters: [{ name: t('np.textFiles'), extensions: ['txt', 'log', 'm3u', 'm3u8', 'csv'] }],
    });
    if (typeof chosen === 'string') draft.nowplaying.text_file = chosen;
  }
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
          <select id="mode" value={draft.audio.mode} onchange={(e) => setMode(e.currentTarget.value)}>
            <option value="loopback">{t('audio.loopback')}</option>
            {#if !isMac || draft.audio.mode === 'process'}<option value="process">{t('audio.process')}</option>{/if}
            <option value="input">{t('audio.input')}</option>
          </select>
        </div>
        {#if draft.audio.mode === 'process'}
          <div class="field">
            <label for="program">{t('audio.program')}</label>
            <input id="program" bind:value={draft.audio.device} placeholder="rekordbox" spellcheck="false" />
            <span class="hint">{t('audio.programHint')}</span>
          </div>
        {:else}
          <div class="field">
            <label for="device">{t('audio.device')}</label>
            <select id="device" bind:value={draft.audio.device} onchange={() => (draft.audio.first_channel = 0)}>
              <option value="">{t('common.systemDefault')}</option>
              {#each deviceOptions as name (name)}<option value={name}>{name}</option>{/each}
              {#if draft.audio.device && !deviceOptions.includes(draft.audio.device)}
                <option value={draft.audio.device}>{draft.audio.device}</option>
              {/if}
            </select>
          </div>
          {#if draft.audio.mode === 'input' && inputDevice?.channels > 2}
            <div class="field">
              <label for="channels">{t('audio.channels')}</label>
              <div class="folder">
                <select id="channels" bind:value={draft.audio.first_channel}>
                  <option value={0}>{t('audio.channelsAuto', { pair: pairLabel(inputDevice.auto_channel) })}</option>
                  {#each channelPairs as first (first)}<option value={first}>{pairLabel(first)}</option>{/each}
                </select>
                <button type="button" class="btn big" onclick={detectChannels} disabled={detect.busy}>
                  {detect.busy ? t('audio.detecting') : t('audio.detect')}
                </button>
              </div>
              {#if detect.result}
                <span class="hint" role="status">{detect.result}</span>
              {:else}
                <span class="hint">{t('audio.detectHint')}</span>
              {/if}
            </div>
          {/if}
        {/if}
        <div class="field">
          <label for="gain">{t('audio.gain', { v: Number(draft.audio.gain).toFixed(2) })}</label>
          <input id="gain" type="range" min="0.25" max="4" step="0.05" bind:value={draft.audio.gain} />
        </div>
      </div>
      <p class="note">{draft.audio.mode === 'process' ? t('audio.processNote') : isMac ? t('audio.macNote') : t('audio.asioNote')}</p>
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
          <label for="codec">{t('server.format')}</label>
          <div class="pair">
            <select id="codec" value={draft.encoder.codec || 'mp3'} onchange={(e) => setCodec(e.currentTarget.value)}>
              <option value="mp3">MP3</option>
              <option value="aac">AAC</option>
              <option value="opus" disabled={draft.server.protocol !== 'icecast'}>Opus</option>
            </select>
            <select id="br" aria-label={t('server.bitrate')} bind:value={draft.encoder.bitrate}>
              {#each BITRATES[draft.encoder.codec] ?? BITRATES.mp3 as b (b)}<option value={b}>{b} kbps</option>{/each}
            </select>
          </div>
          {#if formatHint}<span class="hint" class:warn={opusOnShoutcast}>{formatHint}</span>{/if}
          {#if draft.encoder.codec === 'opus' && !opusOnShoutcast}<span class="hint">{t('format.opusTitles')}</span>{/if}
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
        {#if draft.server.protocol === 'icecast'}
          <div class="field">
            <label class="check"><input type="checkbox" bind:checked={draft.server.tls} /> {t('server.tls')}</label>
            <span class="hint">{t('server.tlsHint')}</span>
          </div>
        {/if}
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
      <h2 class="sub sep-top">{t('processing.tone')}</h2>
      <div class="grid">
        <div class="field">
          <label for="bass">{t('processing.bass', { v: signed(draft.processing.eq_bass_db) })}</label>
          <input id="bass" type="range" min="-6" max="6" step="0.5" bind:value={draft.processing.eq_bass_db} />
        </div>
        <div class="field">
          <label for="presence">{t('processing.presence', { v: signed(draft.processing.eq_presence_db) })}</label>
          <input id="presence" type="range" min="-6" max="6" step="0.5" bind:value={draft.processing.eq_presence_db} />
        </div>
      </div>
      <p class="note">{t('processing.toneNote')}</p>
    {:else if tab === 'nowplaying'}
      <h1>{t('tab.nowplaying')}</h1>
      <div class="radios" role="radiogroup" aria-label={t('np.source')}>
        {#each sources as id (id)}
          <label class="radio"><input type="radio" name="source" value={id} bind:group={draft.nowplaying.source} /> {t(`np.source.${id}`)}</label>
        {/each}
      </div>
      {#if draft.nowplaying.source === 'text-file'}
        <div class="field sep">
          <label for="textfile">{t('np.textFile')}</label>
          <div class="folder">
            <input id="textfile" bind:value={draft.nowplaying.text_file} placeholder="C:\…\tracklist.txt" />
            <button type="button" class="btn big" onclick={pickTextFile}>{t('common.browse')}</button>
          </div>
          <span class="hint">{t('np.textFileHint')}</span>
        </div>
      {:else if draft.nowplaying.source === 'serato'}
        <p class="note">{t('np.seratoHint')}</p>
      {/if}
      <div class="grid sep">
        {#if usesMidi}
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
          <label for="mapping">{t('np.mapping')}</label>
          <select id="mapping" bind:value={draft.nowplaying.midi_mapping} disabled={draft.nowplaying.midi_input === 'none'}>
            <option value="pioneer-ddj">{t('np.mappingDdj')}</option>
            <option value="custom">{t('np.mappingCustom')}</option>
          </select>
        </div>
        {/if}
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
      {#if usesMidi && draft.nowplaying.midi_input !== 'none'}
        {#if draft.nowplaying.midi_mapping === 'custom'}
          <div class="sep-top block">
            <p class="note">{t('np.mappingIntro')}</p>
            <div class="maps">
              {#each [0, 1, 2, 3, 4] as slot (slot)}
                <div class="maprow">
                  <span class="mapname">{slot < 4 ? t('np.fader', { n: slot + 1 }) : t('np.crossfader')}</span>
                  <span class="mapval" class:unset={!mapped(slotControl(slot))}>{controlLabel(slotControl(slot))}</span>
                  <button type="button" class="btn" disabled={learning >= 0} onclick={() => learn(slot)}>
                    {learning === slot ? t('np.learning') : t('np.learn')}
                  </button>
                  <button type="button" class="btn" disabled={learning >= 0 || !mapped(slotControl(slot))} onclick={() => setSlot(slot, { ...unset })}>{t('np.clear')}</button>
                </div>
              {/each}
            </div>
            <span class="msg" class:bad={!learnMsg.ok} role="status">{learnMsg.text}</span>
            <label class="check"><input type="checkbox" bind:checked={draft.nowplaying.midi_custom.crossfader_reversed} /> {t('np.reversed')}</label>
          </div>
        {/if}
        <div class="sep-top block">
          <h2 class="sub">{t('np.sides')}</h2>
          <div class="sides">
            {#each [0, 1, 2, 3] as i (i)}
              <div class="field">
                <label for={`side${i}`}>{t('np.side', { n: i + 1 })}</label>
                <select id={`side${i}`} bind:value={draft.nowplaying.crossfader_assign[i]}>
                  <option value="a">A</option>
                  <option value="thru">THRU</option>
                  <option value="b">B</option>
                </select>
              </div>
            {/each}
          </div>
          <p class="note">{draft.nowplaying.midi_mapping === 'custom' ? t('np.sideHintCustom') : t('np.sideHintDdj')}</p>
        </div>
      {/if}
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
    {:else if tab === 'share'}
      <h1>{t('tab.share')}</h1>
      <p class="note">{t('share.intro')}</p>
      <div class="grid">
        <div class="field">
          <label for="hook">{t('share.webhook')}</label>
          <div class="pass">
            <input id="hook" type={showHook ? 'text' : 'password'} bind:value={draft.share.discord_webhook} placeholder="https://discord.com/api/webhooks/…" autocomplete="off" spellcheck="false" />
            <button type="button" class="eye" aria-label={showHook ? t('server.hidePassword') : t('server.showPassword')} aria-pressed={showHook} onclick={() => (showHook = !showHook)}>
              <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M1 12s4-8 11-8 11 8 11 8-4 8-11 8-11-8-11-8z"></path><circle cx="12" cy="12" r="3"></circle></svg>
            </button>
          </div>
          <span class="hint">{t('share.webhookHint')}</span>
        </div>
        <div class="field">
          <label for="listen">{t('share.listenUrl')}</label>
          <input id="listen" bind:value={draft.share.listen_url} placeholder={t('share.listenUrlAuto')} />
          <span class="hint">{t('share.listenUrlHint')}</span>
        </div>
      </div>
      <div class="checks">
        <label class="check"><input type="checkbox" bind:checked={draft.share.on_live} /> {t('share.onLive')}</label>
        {#if draft.share.on_live}<textarea class="tpl" rows="2" aria-label={t('share.onLive')} bind:value={draft.share.live_message}></textarea>{/if}
        <label class="check"><input type="checkbox" bind:checked={draft.share.on_track} /> {t('share.onTrack')}</label>
        {#if draft.share.on_track}<textarea class="tpl" rows="1" aria-label={t('share.onTrack')} bind:value={draft.share.track_message}></textarea>{/if}
        <label class="check"><input type="checkbox" bind:checked={draft.share.on_stop} /> {t('share.onStop')}</label>
        {#if draft.share.on_stop}<textarea class="tpl" rows="2" aria-label={t('share.onStop')} bind:value={draft.share.stop_message}></textarea>{/if}
        <span class="hint">{t('share.placeholders')}</span>
      </div>
      <div class="update-row">
        <button type="button" class="btn big" disabled={sendingTest} onclick={testDiscord}>{t('share.test')}</button>
        <span class="msg" class:bad={!discordMsg.ok} role="status">{discordMsg.text}</span>
      </div>
      <p class="note">{t('share.cueNote')}</p>
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
        <div class="field">
          <label for="theme">{t('general.theme')}</label>
          <select id="theme" bind:value={draft.app.theme}>
            <option value="system">{t('general.themeSystem')}</option>
            <option value="dark">{t('general.themeDark')}</option>
            <option value="light">{t('general.themeLight')}</option>
          </select>
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
      <div class="report">
        <div class="about-buttons">
          <button type="button" class="btn big" onclick={collectReport} disabled={report.busy}>
            {report.busy ? t('report.collecting') : t('report.button')}
          </button>
          {#if report.text}
            <button type="button" class="btn big" onclick={copyReport}>{report.copied ? t('report.copied') : t('report.copy')}</button>
            <button type="button" class="btn big" onclick={() => invoke('open_setup_report_form', { report: report.text })}>{t('report.open')}</button>
          {/if}
        </div>
        <span class="hint">{t('report.hint')}</span>
        {#if report.text}<textarea class="report-text" readonly rows="14" aria-label={t('report.button')}>{report.text}</textarea>{/if}
      </div>
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
  .tab.active { background: var(--raised); color: var(--text); }
  .path { font-size: 11px; color: var(--muted); padding: 0 10px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; user-select: text; }
  .body { flex-grow: 1; min-width: 0; padding: 24px 32px; display: flex; flex-direction: column; gap: 18px; overflow-y: auto; }
  h1 { margin: 0; font-size: 22px; }
  .grid { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 16px 20px; }
  .grid.sep { border-top: 1px solid var(--line); padding-top: 18px; }
  h2.sub { font-size: 15px; margin: 4px 0 0; }
  .maps { display: flex; flex-direction: column; gap: 8px; margin: 12px 0; max-width: 640px; }
  .maprow { display: grid; grid-template-columns: 150px 1fr auto auto; gap: 10px; align-items: center; }
  .mapname { font-weight: 600; }
  .mapval { font-family: var(--mono, ui-monospace, monospace); font-size: 13px; color: var(--text-2); }
  .mapval.unset { color: var(--muted); }
  .sides { display: grid; grid-template-columns: repeat(4, minmax(0, 140px)); gap: 12px; margin-bottom: 8px; }
  .block { display: flex; flex-direction: column; gap: 10px; }
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
  .eye:hover { color: var(--text); background: var(--raised); }
  .sep-top { border-top: 1px solid var(--line); padding-top: 18px; }
  h2 { margin: 8px 0 0; font-size: 15px; font-weight: 600; color: var(--text-2); border-top: 1px solid var(--line); padding-top: 18px; }
  .checks { display: flex; flex-direction: column; gap: 10px; }
  .tpl {
    margin-left: 28px; max-width: 640px; padding: 8px 10px; resize: vertical;
    background: var(--panel); color: var(--text); border: 1px solid var(--line-strong); border-radius: 8px;
    font: inherit; font-size: 13px;
  }
  .checks .sub { margin-left: 28px; }
  .dim { opacity: 0.55; }
  .update-row { display: flex; align-items: center; gap: 12px; }
  .notes { background: var(--panel); border: 1px solid var(--line); border-radius: 10px; padding: 14px; display: flex; flex-direction: column; gap: 10px; max-width: 640px; }
  .notes-title { font-weight: 600; }
  .notes pre { margin: 0; white-space: pre-wrap; font: inherit; font-size: 13px; color: var(--text-2); max-height: 180px; overflow-y: auto; user-select: text; }
  .notes .primary { align-self: flex-start; }
  .about-version { margin: -8px 0 0; color: var(--muted); }
  .about-buttons { display: flex; flex-wrap: wrap; gap: 10px; }
  .report { display: flex; flex-direction: column; gap: 8px; margin-top: 8px; }
  .report-text { width: 100%; font-family: ui-monospace, Menlo, Consolas, monospace; font-size: 12px; resize: vertical; user-select: text; }
  .disclaimer { font-size: 12px; margin-top: 8px; }
  .copyright { font-size: 12px; margin-top: -8px; }
  .folder { display: flex; gap: 8px; }
  .folder input, .folder select { flex: 1 1 auto; min-width: 0; }
  .folder .btn { flex-shrink: 0; }
  .hint { font-size: 12px; color: var(--muted); user-select: text; overflow-wrap: anywhere; }
  .hint.warn { color: var(--on-air-text); }
  .note { margin: 0; font-size: 13px; color: var(--muted); max-width: 640px; }
  footer { display: flex; align-items: center; gap: 12px; }
  .msg { font-size: 13px; color: var(--ok); user-select: text; }
  .msg.bad { color: var(--on-air-text); }
  .grow { flex-grow: 1; }
  .btn.big { height: 40px; padding: 0 16px; white-space: nowrap; flex-shrink: 0; font-size: 14px; border-radius: 8px; }
  .primary { height: 40px; padding: 0 18px; background: var(--text); color: var(--bg); border: none; border-radius: 8px; font-size: 14px; font-weight: 600; }
</style>
