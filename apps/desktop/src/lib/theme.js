import { getCurrentWindow } from '@tauri-apps/api/window';

/**
 * Colour theme: "system" follows Windows (prefers-color-scheme), "light" / "dark" force one.
 * The CSS palettes live in app.css; the window title bar follows through Tauri.
 */
export function applyTheme(theme) {
  const forced = theme === 'light' || theme === 'dark' ? theme : null;
  const root = document.documentElement;
  if (forced) root.dataset.theme = forced;
  else delete root.dataset.theme;
  try {
    getCurrentWindow().setTheme(forced).catch(() => {});
  } catch {
    // Browser preview: there is no window to theme.
  }
}
