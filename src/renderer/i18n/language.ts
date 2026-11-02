/**
 * Pure language-resolution helpers. Kept free of DOM and i18next imports so they can be
 * unit-tested in the node test environment.
 */

/** UI languages shipped with the app. `en` is the fallback and the source of truth for keys. */
export const SUPPORTED_LANGUAGES = ['en', 'zh-CN'] as const;
export type AppLanguage = (typeof SUPPORTED_LANGUAGES)[number];

/** The persisted setting: an explicit language, or `system` to follow the OS locale. */
export type LanguageSetting = 'system' | AppLanguage;

export const FALLBACK_LANGUAGE: AppLanguage = 'en';

/** True when a BCP-47 tag is a Chinese variant (zh, zh-CN, zh-Hans, zh-TW, ...). */
export function isChinese(tag: string | undefined | null): boolean {
  return (tag ?? '').toLowerCase().startsWith('zh');
}

/**
 * Only the primary (most-preferred) locale decides: a machine set to [en-GB, zh-CN] is an
 * English user who also reads Chinese, so it gets English.
 */
export function detectLanguageFrom(locales: readonly (string | undefined | null)[]): AppLanguage {
  const primary = locales.find((tag): tag is string => typeof tag === 'string' && tag.length > 0);
  return isChinese(primary) ? 'zh-CN' : FALLBACK_LANGUAGE;
}

/** Map the persisted setting onto the language to display, using `locales` for `system`. */
export function resolveLanguage(
  setting: LanguageSetting | undefined,
  locales: readonly (string | undefined | null)[]
): AppLanguage {
  if (setting === 'en' || setting === 'zh-CN') return setting;
  return detectLanguageFrom(locales);
}
