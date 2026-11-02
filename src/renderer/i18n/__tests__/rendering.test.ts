import { describe, expect, it, beforeAll } from 'vitest';
import i18next, { type i18n } from 'i18next';
import { en } from '../locales/en';
import { zhCN } from '../locales/zh-CN';
import { STATE_LABEL_KEY } from '../../lib/processState';

/**
 * Resolves keys through the real i18next engine, so interpolation and plural rules are
 * exercised the way the renderer uses them (not just compared as raw dictionaries).
 */
let zh: i18n;

beforeAll(async () => {
  zh = i18next.createInstance();
  await zh.init({
    resources: { en: { translation: en }, 'zh-CN': { translation: zhCN } },
    lng: 'zh-CN',
    fallbackLng: 'en',
    interpolation: { escapeValue: false }
  });
});

describe('zh-CN rendering', () => {
  it('interpolates named values', () => {
    expect(zh.t('sidebar.prompt.removeAppTitle', { name: 'api' })).toBe(
      '从 NeoHub 移除“api”？'
    );
    expect(zh.t('common.sortTooltip', { mode: '名称（A-Z）' })).toBe('排序应用 - 名称（A-Z）');
  });

  it('picks the plural form from the count', () => {
    expect(zh.t('dashboard.appCount', { count: 1 })).toBe('1 个应用');
    expect(zh.t('dashboard.appCount', { count: 5 })).toBe('5 个应用');
    expect(zh.t('appDetail.envFilesChanged', { count: 3 })).toBe('3 个 env 文件已在磁盘上被修改。');
    // A key with `count` but no plural suffix must still resolve to the base string.
    expect(zh.t('sidebar.prompt.deleteFolderDesc', { count: 3 })).toBe(
      '3 个应用将移动到“（未分组）”。'
    );
  });

  it('translates every process state the StatusDot / header can show', () => {
    for (const state of Object.keys(STATE_LABEL_KEY) as (keyof typeof STATE_LABEL_KEY)[]) {
      const label = zh.t(STATE_LABEL_KEY[state]);
      expect(label).not.toBe(`state.${state}`);
      expect(label).not.toBe(en.state[state as keyof typeof en.state]);
    }
  });

  it('falls back to English for a key missing from the active locale', () => {
    const enOnly = i18next.createInstance();
    return enOnly
      .init({
        resources: { en: { translation: en }, 'zh-CN': { translation: {} } },
        lng: 'zh-CN',
        fallbackLng: 'en'
      })
      .then(() => {
        expect(enOnly.t('common.close')).toBe('Close');
      });
  });
});
