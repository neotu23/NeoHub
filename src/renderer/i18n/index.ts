import i18n from 'i18next';
import { initReactI18next } from 'react-i18next';
import {
  detectLanguageFrom,
  resolveLanguage,
  FALLBACK_LANGUAGE,
  SUPPORTED_LANGUAGES,
  type AppLanguage,
  type LanguageSetting
} from './language';
import { en } from './locales/en';
import { zhCN } from './locales/zh-CN';

/**
 * Type-level wiring: every `t('...')` call is checked against the English dictionary, so a
 * renamed or mistyped key is a compile error rather than a raw key rendered in the UI.
 */
declare module 'i18next' {
  interface CustomTypeOptions {
    defaultNS: 'translation';
    resources: { translation: typeof en };
    returnNull: false;
  }
}

export {
  FALLBACK_LANGUAGE,
  SUPPORTED_LANGUAGES,
  isChinese,
  resolveLanguage,
  type AppLanguage,
  type LanguageSetting
} from './language';

/** The OS/browser locale list, most-preferred first. */
function systemLocales(): string[] {
  const langs = navigator.languages;
  return langs && langs.length > 0 ? [...langs] : [navigator.language];
}

/** Resolve `system` against the live OS locale list. */
export function detectSystemLanguage(): AppLanguage {
  return detectLanguageFrom(systemLocales());
}

/** Map the persisted setting onto the language to show right now. */
export function resolveCurrentLanguage(setting: LanguageSetting | undefined): AppLanguage {
  return resolveLanguage(setting, systemLocales());
}

void i18n.use(initReactI18next).init({
  resources: {
    en: { translation: en },
    'zh-CN': { translation: zhCN }
  },
  lng: detectSystemLanguage(),
  fallbackLng: FALLBACK_LANGUAGE,
  supportedLngs: [...SUPPORTED_LANGUAGES],
  // Resources ship inline, so initialise synchronously - no flash of untranslated UI.
  initAsync: false,
  interpolation: {
    // React escapes interpolated values already.
    escapeValue: false
  },
  returnNull: false
});

/** Keep `<html lang>` in sync for screen readers and hyphenation. */
i18n.on('languageChanged', (lng: string) => {
  document.documentElement.lang = lng;
});
document.documentElement.lang = i18n.language;

export default i18n;
