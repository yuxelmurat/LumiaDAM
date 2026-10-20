import { useSyncExternalStore } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { appTr } from './app.tr';
import { aiTr } from './ai.tr';
import { editorsTr } from './editors.tr';

export type Language = 'en' | 'tr';
const translations: Record<string, string> = { ...appTr, ...aiTr, ...editorsTr };
let language: Language = 'en';
const listeners = new Set<() => void>();
const native = () => typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;
export const getLanguage = () => language;
export function subscribeLanguage(listener: () => void) {
  listeners.add(listener);
  return () => { listeners.delete(listener); };
}
function applyLanguage(value: Language) {
  const changed = language !== value;
  language = value;
  if (typeof document !== 'undefined') document.documentElement.lang = value;
  try { localStorage.setItem('lumiadam.language', value); } catch { /* Depolama kapalıysa oturum dili kullanılabilir. */ }
  if (changed) listeners.forEach(listener => listener());
}
export async function initializeLanguage() {
  let saved: unknown;
  try { saved = native() ? await invoke('get_app_language') : localStorage.getItem('lumiadam.language'); }
  catch (error) { console.error('Language preferences could not be read', error); }
  applyLanguage(saved === 'tr' ? 'tr' : 'en');
}
export async function setLanguage(value: Language) {
  if (value !== 'en' && value !== 'tr') throw new Error('Unsupported language');
  // Kalıcı kayıt başarısızsa ekranda kaydedilmiş gibi göstermeyiz.
  if (native()) await invoke('set_app_language', { language: value });
  applyLanguage(value);
}
export function t(key: string, values: Record<string, string | number> = {}): string {
  const text = language === 'tr' ? translations[key] ?? key : key;
  return text.replace(/\{([\w]+)\}/g, (placeholder, name: string) =>
    Object.prototype.hasOwnProperty.call(values, name) ? String(values[name]) : placeholder);
}
const escape = (value: string) => value.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
const messagePatterns = Object.entries(translations).filter(([key]) => /\{\w+\}/.test(key)).map(([key, value]) => {
  const names: string[] = [];
  const chunks = key.split(/(\{\w+\})/g).map(chunk => {
    if (/^\{\w+\}$/.test(chunk)) { names.push(chunk.slice(1, -1)); return '([\\s\\S]*?)'; }
    return escape(chunk);
  });
  return { pattern: new RegExp(`^${chunks.join('')}$`), names, value };
});
export function translateMessage(message: string): string {
  if (language !== 'tr' || !message) return message;
  if (Object.prototype.hasOwnProperty.call(translations, message)) return translations[message];
  const trimmed = message.trim();
  if (Object.prototype.hasOwnProperty.call(translations, trimmed)) return message.replace(trimmed, translations[trimmed]);
  for (const { pattern, names, value } of messagePatterns) {
    const match = message.match(pattern);
    if (match) return value.replace(/\{(\w+)\}/g, (placeholder, name: string) => {
      const index = names.indexOf(name);
      return index < 0 ? placeholder : match[index + 1];
    });
  }
  // Ham sistem hatası, kullanıcı adı veya dosya yolu tahminle çevrilmez.
  return message;
}
export const formatNumber = (value: number) => value.toLocaleString(language === 'tr' ? 'tr-TR' : 'en-US');
export const formatDate = (value: string | number | Date) => new Date(value).toLocaleString(language === 'tr' ? 'tr-TR' : 'en-US');
export function useTranslation() {
  const current = useSyncExternalStore(subscribeLanguage, getLanguage, getLanguage);
  return { t, language: current };
}
