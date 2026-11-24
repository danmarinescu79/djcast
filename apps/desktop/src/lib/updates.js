// Updates through the Tauri updater (GitHub releases, signature-checked), plus the
// "What's new" note shown once after an update installs.
import { check } from '@tauri-apps/plugin-updater';
import { relaunch } from '@tauri-apps/plugin-process';

const WHATS_NEW_KEY = 'djcast.whatsNew';

/** Returns `{ version, notes, update }` when a newer version exists, else null. Throws on errors. */
export async function checkForUpdate() {
  const update = await check();
  if (!update) return null;
  return { version: update.version, notes: update.body || '', update };
}

/** Download, install and restart. `onProgress(percent)` while downloading. */
export async function installUpdate(found, onProgress = () => {}) {
  let total = 0;
  let received = 0;
  try {
    localStorage.setItem(WHATS_NEW_KEY, JSON.stringify({ version: found.version, notes: found.notes }));
  } catch {
    /* only a convenience */
  }
  await found.update.downloadAndInstall((ev) => {
    if (ev.event === 'Started') total = ev.data.contentLength || 0;
    if (ev.event === 'Progress') {
      received += ev.data.chunkLength;
      if (total) onProgress(Math.min(100, Math.round((received / total) * 100)));
    }
  });
  await relaunch();
}

/** The notes of the version that was just installed (once), or null. */
export function takeWhatsNew(currentVersion) {
  try {
    const raw = localStorage.getItem(WHATS_NEW_KEY);
    if (!raw) return null;
    const saved = JSON.parse(raw);
    if (saved.version !== currentVersion) return null;
    localStorage.removeItem(WHATS_NEW_KEY);
    return saved;
  } catch {
    return null;
  }
}
