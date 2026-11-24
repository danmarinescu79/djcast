// Minimal i18n: one dictionary per language in src/locales, English as the fallback.
// To add a language, copy locales/en.js, translate the values and register it below.
import de from '../locales/de.js';
import en from '../locales/en.js';
import es from '../locales/es.js';
import fr from '../locales/fr.js';
import it from '../locales/it.js';
import ja from '../locales/ja.js';
import nl from '../locales/nl.js';
import pl from '../locales/pl.js';
import pt from '../locales/pt.js';
import ro from '../locales/ro.js';
import ru from '../locales/ru.js';
import tr from '../locales/tr.js';
import zh from '../locales/zh.js';

// Shown in Settings → General, each in its own language.
export const LANGUAGES = [
  { code: 'de', name: 'Deutsch' },
  { code: 'en', name: 'English' },
  { code: 'es', name: 'Español' },
  { code: 'fr', name: 'Français' },
  { code: 'it', name: 'Italiano' },
  { code: 'nl', name: 'Nederlands' },
  { code: 'pl', name: 'Polski' },
  { code: 'pt', name: 'Português (Brasil)' },
  { code: 'ro', name: 'Română' },
  { code: 'tr', name: 'Türkçe' },
  { code: 'ru', name: 'Русский' },
  { code: 'ja', name: '日本語' },
  { code: 'zh', name: '中文（简体）' },
];
const DICTS = { de, en, es, fr, it, ja, nl, pl, pt, ro, ru, tr, zh };

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
