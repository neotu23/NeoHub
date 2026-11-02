import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

/**
 * Boots the real i18n module (the one `main.tsx` imports for its side effect) against a
 * stubbed browser environment, to prove the renderer start-up path is usable in a
 * Chinese-locale session without a flash of English.
 */
describe('i18n boot', () => {
  beforeEach(() => {
    vi.resetModules();
    vi.stubGlobal('navigator', { language: 'zh-CN', languages: ['zh-CN', 'en-US'] });
    vi.stubGlobal('document', { documentElement: { lang: '' } });
  });

  afterEach(() => {
    vi.unstubAllGlobals();
    vi.resetModules();
  });

  it('infers Simplified Chinese from the OS locale', async () => {
    const mod = await import('../index');
    expect(mod.detectSystemLanguage()).toBe('zh-CN');
    expect(mod.default.language).toBe('zh-CN');
    expect(mod.default.t('sidebar.dashboard')).toBe('仪表盘');
  });

  it('mirrors the resolved language onto <html lang>', async () => {
    await import('../index');
    expect(document.documentElement.lang).toBe('zh-CN');
  });

  it('switches language at runtime and updates <html lang>', async () => {
    const mod = await import('../index');
    await mod.default.changeLanguage('en');
    expect(mod.default.t('sidebar.dashboard')).toBe('Dashboard');
    expect(document.documentElement.lang).toBe('en');
  });
});
