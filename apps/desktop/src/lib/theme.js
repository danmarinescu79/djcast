/**
 * Colour theme: "system" follows Windows (prefers-color-scheme), "light" / "dark" force one.
 * The CSS palettes live in app.css; the window title bar follows through Tauri.
 */
export function applyTheme(theme) {
  const forced = theme === 'light' || theme === 'dark' ? theme : null;
  const root = document.documentElement;
  if (forced) root.dataset.theme = forced;
  else delete root.dataset.theme;
  // Not available in the browser preview; ignore there.
  import('@tauri-apps/api/window')
    .then(({ getCurrentWindow }) => getCurrentWindow().setTheme(forced))
    .catch(() => {});
}
