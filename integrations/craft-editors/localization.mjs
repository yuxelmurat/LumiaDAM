import { readFileSync, writeFileSync, mkdirSync, existsSync } from 'node:fs';
import { resolve, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';

const directory = dirname(fileURLToPath(import.meta.url));
export const catalogFiles = ['localization.mjs', 'tr.tsv'].map(name => resolve(directory, name));
const rows = text => text.split(/\r?\n/).filter(line => line.trim() && !line.startsWith('#')).map(line => line.split('\t'));
export function validateTurkishCatalog(text) {
  const parsed = rows(text), seen = new Set();
  const placeholders = value => (value.match(/\{[^{}]*\}/g) ?? []).sort().join('|');
  for (const row of parsed) {
    const [context, source, translation] = row, key = `${context}\t${source}`;
    if (row.length !== 3 || !source || !translation || translation.includes('\r')) throw new Error(`Geçersiz Türkçe çeviri satırı: ${source}`);
    if (seen.has(key)) throw new Error(`Yinelenen Türkçe çeviri: ${source}`);
    seen.add(key);
    if (context === '@id') continue;
    const forms = context === '@plural' ? source.split('|') : [source];
    if (context === '@plural' && (forms.length !== 2 || translation.includes('|'))) throw new Error(`Türkçe çoğul tek form olmalıdır: ${source}`);
    if (forms.some(form => placeholders(form) !== placeholders(translation))) throw new Error(`Çeviride yer tutucu uyuşmazlığı: ${source}`);
  }
  return parsed;
}
const sourceRows = validateTurkishCatalog(readFileSync(resolve(directory, 'tr.tsv'), 'utf8'));
const translations = new Map(sourceRows.map(([context, source, translation]) => [`${context}\t${source}`, translation]));
// Yalnız derleme anında, bilinen arayüz etiketlerinin noktalama varyantları eşleştirilir.
// Kullanıcı metinleri, belge içeriği veya dosya adları bu tablodan geçirilmez.
const normalize = text => text.replace(/[.…: ]+$/u, '').toLocaleLowerCase('en');
const labels = new Map(sourceRows.filter(([context]) => !context).map(([, source, translation]) => [normalize(source), translation.replace(/[.…: ]+$/u, '')]));
const lookup = (context, source) => translations.get(`${context}\t${source}`) ?? translations.get(`\t${source}`) ??
  (!context && labels.has(normalize(source)) ? labels.get(normalize(source)) + (source.match(/[.…: ]+$/u)?.[0] ?? '') : undefined);

export function prepareTurkishCatalog({ name, source, upstream, writeChanged }) {
  const path = 'crates/ui-egui/src/i18n';
  const reference = rows(upstream(`${path}/es.tsv`));
  const existing = name === 'pdfcraft' ? upstream(`${path}/tr.tsv`) : '';
  const existingKeys = new Set(rows(existing).map(([context, source]) => `${context}\t${source}`));
  const missing = [], additions = [];
  for (const [context, key] of reference) {
    if (context === '@id' || existingKeys.has(`${context}\t${key}`)) continue;
    const translation = lookup(context, key);
    if (translation) additions.push([context, key, translation].join('\t'));
    else missing.push({ context, source: key });
  }
  const result = `${existing || '# LumiaDAM Türkçe arayüz kataloğu. Kullanıcı içeriği çevrilmez.\n'}\n${additions.join('\n')}\n`;
  validateTurkishCatalog(result);
  writeChanged(resolve(source, path, 'tr.tsv'), result);
  const reportDir = resolve(source, '../lumiadam-i18n');
  mkdirSync(reportDir, { recursive: true });
  writeFileSync(resolve(reportDir, 'coverage.json'), JSON.stringify({ name, sourceRows: reference.filter(([context]) => context !== '@id').length,
    commandOverrides: reference.filter(([context]) => context === '@id').length,
    translatedRows: reference.filter(([context, key]) => context !== '@id' && (existingKeys.has(`${context}\t${key}`) || lookup(context, key))).length,
    missingRows: missing.length, missing }, null, 2));
  console.log(`${name}: ${additions.length} Türkçe satır eklendi, ${missing.length} kaynak satır İngilizce yedek dili kullanıyor.`);
  if (name === 'pdfcraft') return;
  const moduleFile = !existsSync(resolve(source, path, 'mod.rs'))
    ? 'crates/ui-egui/src/i18n.rs'
    : `${path}/mod.rs`;
  const module = upstream(moduleFile);
  const registry = /pub static LANGUAGES: \[LangInfo; (\d+)\] = \[/;
  if (!registry.test(module)) throw new Error(`${name}: Dil kayıt yapısı değişmiş.`);
  const catalog = name === 'photocraft' ? 'catalog!("tr")' :
    `include_str!("${moduleFile.endsWith('/i18n.rs') ? 'i18n/' : ''}tr.tsv")`;
  let patched = module.replace(registry, (_, count) => `pub static LANGUAGES: [LangInfo; ${Number(count) + 1}] = [`);
  const end = patched.indexOf('\n];', patched.indexOf('pub static LANGUAGES:'));
  if (end < 0) throw new Error(`${name}: Dil kayıt dizisinin sonu bulunamadı.`);
  // İngilizce sabiti LANGUAGES[0] kullandığından Türkçe kaydı en sona eklenir.
  patched = patched.slice(0, end) + `\n    LangInfo { code: "tr", name: "Türkçe", source: ${catalog}, plural: plural_none, complete_menus: ${missing.length === 0}, catalog: OnceLock::new() },` + patched.slice(end);
  writeChanged(resolve(source, moduleFile), patched);
}
