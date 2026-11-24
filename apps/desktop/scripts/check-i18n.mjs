// Checks that every translation key used in src/ exists in English, and that every
// language has exactly the English keys. Run: node scripts/check-i18n.mjs
import { readdirSync, readFileSync, statSync } from 'node:fs';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import en from '../src/locales/en.js';

// Every locale file next to en.js is checked.
const localeDir = fileURLToPath(new URL('../src/locales', import.meta.url));
const languages = {};
for (const f of readdirSync(localeDir)) {
  const code = f.replace(/\.js$/, '');
  if (code !== 'en' && f.endsWith('.js')) languages[code] = (await import(`../src/locales/${f}`)).default;
}
const files = [];
(function walk(dir) {
  for (const f of readdirSync(dir)) {
    const p = join(dir, f);
    if (statSync(p).isDirectory()) walk(p);
    else if (/\.(svelte|js)$/.test(f) && !p.includes('locales')) files.push(p);
  }
})(fileURLToPath(new URL('../src', import.meta.url)));

const used = new Set();
const prefixes = new Set();
for (const f of files) {
  const src = readFileSync(f, 'utf8');
  for (const m of src.matchAll(/\bt\(\s*'([^']+)'/g)) used.add(m[1]);
  // t(`deck.${x}`) style: remember the static prefix.
  for (const m of src.matchAll(/\bt\(\s*`([^`$]+)\$\{/g)) prefixes.add(m[1]);
  // Keys stored in arrays and passed to t() later (e.g. 'band.bass').
  for (const m of src.matchAll(/'((?:band|tab|deck|np|processing|preset)\.[A-Za-z0-9.-]+)'/g)) used.add(m[1]);
}

let problems = 0;
for (const k of used) {
  if (!(k in en)) {
    console.error(`missing in en: ${k}`);
    problems++;
  }
}
for (const p of prefixes) {
  if (!Object.keys(en).some((k) => k.startsWith(p))) {
    console.error(`no keys for dynamic prefix: ${p}`);
    problems++;
  }
}
for (const [code, dict] of Object.entries(languages)) {
  for (const k of Object.keys(en)) if (!(k in dict)) (console.error(`${code}: missing ${k}`), problems++);
  for (const k of Object.keys(dict)) if (!(k in en)) (console.error(`${code}: extra ${k}`), problems++);
  for (const [k, v] of Object.entries(en)) {
    const vars = (s) => [...String(s).matchAll(/\{(\w+)\}/g)].map((m) => m[1]).sort().join(',');
    if (k in dict && vars(v) !== vars(dict[k])) (console.error(`${code}: placeholders differ in ${k}`), problems++);
  }
}
console.log(problems ? `${problems} problem(s)` : `ok: ${used.size} keys used, ${Object.keys(en).length} in en, languages: en, ${Object.keys(languages).join(', ')}`);
process.exit(problems ? 1 : 0);
