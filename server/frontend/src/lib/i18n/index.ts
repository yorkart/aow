import i18next from 'i18next';
import en from './locales/en-US.json';
import zh from './locales/zh-CN.json';

export const defaultLanguage = 'en-US';
export const languages = [
  { id: 'en-US', name: 'English', dir: 'ltr', aliases: ['en'], resource: en },
  { id: 'zh-CN', name: '简体中文', dir: 'ltr', aliases: ['zh', 'zh-hans', 'zh-hans-cn'], resource: zh },
] as const;
export type Language = typeof languages[number]['id'];
const storageKey = 'aow.language';

export function resolveLanguage(value: unknown): Language {
  if (typeof value !== 'string') return defaultLanguage;
  const normalized = value.toLowerCase().replaceAll('_', '-');
  return languages.find(language => language.id.toLowerCase() === normalized
    || (language.aliases as readonly string[]).includes(normalized))?.id ?? defaultLanguage;
}

function savedLanguage(): Language {
  try { return resolveLanguage(globalThis.localStorage?.getItem(storageKey)); }
  catch { return defaultLanguage; }
}

// Initialize synchronously before modules create translated labels and formatters.
// A language change is explicitly applied with a page reload to refresh those values.
void i18next.init({
  lng: savedLanguage(), fallbackLng: defaultLanguage,
  supportedLngs: languages.map(language => language.id),
  load: 'currentOnly', initAsync: false,
  resources: Object.fromEntries(languages.map(language => [language.id, { translation: language.resource }])),
  interpolation: { escapeValue: false },
  returnNull: false, returnEmptyString: true,
});

export function getLanguage(): Language { return resolveLanguage(i18next.resolvedLanguage); }
export function saveLanguage(language: Language): boolean {
  try { globalThis.localStorage.setItem(storageKey, language); return true; }
  catch { return false; }
}
export function languageHeaders(init?: HeadersInit): Headers {
  const headers = new Headers(init);
  headers.set('Accept-Language', getLanguage());
  return headers;
}
export function t(key: string, values?: Record<string, unknown>): string {
  return i18next.t(key, values ?? {});
}
export function initializeLanguage(): void {
  document.documentElement.lang = getLanguage();
  document.documentElement.dir = languages.find(language => language.id === getLanguage())?.dir ?? 'ltr';
}
