import { useCallback, useEffect, useRef, useState } from 'react';
import type { SettingsState } from '@shared/ipc';
import i18n, {
  resolveCurrentLanguage,
  type AppLanguage,
  type LanguageSetting
} from '../i18n';

/**
 * Keep i18next in sync with the persisted `language` setting.
 *
 * - Mount fetches the setting and applies it (the OS locale is only the initial guess).
 * - `settings-changed` (dispatched by SettingsDrawer) re-reads it.
 * - When the setting is `system`, an OS locale change re-resolves it live.
 *
 * Components don't need this hook to re-render on a language change: `useTranslation`
 * subscribes to i18next and re-renders them. It exists so the app shell owns the
 * settings round-trip and SettingsDrawer can flip the language.
 */
export function useLanguage(): {
  language: LanguageSetting;
  resolved: AppLanguage;
  setLanguage: (language: LanguageSetting) => Promise<void>;
} {
  const [language, setLocalLanguage] = useState<LanguageSetting>('system');
  const [resolved, setResolved] = useState<AppLanguage>(() => resolveCurrentLanguage('system'));
  // Latest setting, for the locale-change listener without re-subscribing on every change.
  const languageRef = useRef<LanguageSetting>('system');

  const apply = useCallback((setting: LanguageSetting): void => {
    languageRef.current = setting;
    const next = resolveCurrentLanguage(setting);
    setResolved(next);
    if (i18n.language !== next) void i18n.changeLanguage(next);
    document.documentElement.lang = next;
  }, []);

  useEffect(() => {
    let cancelled = false;
    const refresh = (): void => {
      void window.api.invoke('settings:get', undefined).then((s: SettingsState) => {
        if (cancelled) return;
        setLocalLanguage(s.language);
        apply(s.language);
      });
    };
    refresh();

    const onSettingsChanged = (): void => refresh();
    const onLocaleChanged = (): void => {
      if (languageRef.current === 'system') apply('system');
    };
    window.addEventListener('settings-changed', onSettingsChanged);
    window.addEventListener('languagechange', onLocaleChanged);
    return () => {
      cancelled = true;
      window.removeEventListener('settings-changed', onSettingsChanged);
      window.removeEventListener('languagechange', onLocaleChanged);
    };
  }, [apply]);

  const setLanguage = async (next: LanguageSetting): Promise<void> => {
    const settings = await window.api.invoke('settings:set', { patch: { language: next } });
    setLocalLanguage(settings.language);
    apply(settings.language);
  };

  return { language, resolved, setLanguage };
}
