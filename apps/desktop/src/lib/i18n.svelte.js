// Minimal i18n: one dictionary per language in src/locales, English as the fallback.
// To add a language, copy locales/en.js, translate the values and register it below.
import en from '../locales/en.js';
import ro from '../locales/ro.js';

export const LANGUAGES = [
  { code: 'en', name: 'English' },
  { code: 'ro', name: 'Română' },
];
const DICTS = { en, ro };

let current = $state('en');

/** Resolve a preference ("auto", "", "ro", …) against the system locale ("ro-RO"). */
export function setLanguage(pref, systemLocale = 'en') {
  const wanted = !pref || pref === 'auto' ? systemLocale : pref;
  const code = (wanted || 'en').toLowerCase().split(/[-_]/)[0];
  current = DICTS[code] ? code : 'en';
  try {
    document.documentElement.lang = current;
  } catch {
    /* not in a browser */
  }
}

export function language() {
  return current;
}

/** Translate `key`; `{name}` placeholders are replaced from `vars`. Reactive in templates. */
export function t(key, vars) {
  const text = DICTS[current][key] ?? en[key] ?? key;
  return vars ? text.replace(/\{(\w+)\}/g, (m, k) => (k in vars ? String(vars[k]) : m)) : text;
}
