import { describe, expect, it } from 'vitest';
import { detectLanguageFrom, isChinese, resolveLanguage } from '../language';

describe('isChinese', () => {
  it('matches any zh variant', () => {
    expect(isChinese('zh')).toBe(true);
    expect(isChinese('zh-CN')).toBe(true);
    expect(isChinese('zh-Hans-CN')).toBe(true);
    expect(isChinese('ZH-tw')).toBe(true);
  });

  it('rejects other locales and missing values', () => {
    expect(isChinese('en-US')).toBe(false);
    expect(isChinese('ja-JP')).toBe(false);
    expect(isChinese(undefined)).toBe(false);
    expect(isChinese(null)).toBe(false);
    expect(isChinese('')).toBe(false);
  });
});

describe('detectLanguageFrom', () => {
  it('picks Simplified Chinese for a Chinese system locale', () => {
    expect(detectLanguageFrom(['zh-Hans-CN', 'en-US'])).toBe('zh-CN');
  });

  it('keeps the first preference when it is not Chinese', () => {
    expect(detectLanguageFrom(['en-GB', 'zh-CN'])).toBe('en');
  });

  it('falls back to English for unknown or empty locale lists', () => {
    expect(detectLanguageFrom(['fr-FR'])).toBe('en');
    expect(detectLanguageFrom([])).toBe('en');
    expect(detectLanguageFrom([undefined])).toBe('en');
  });
});

describe('resolveLanguage', () => {
  it('honours an explicit setting over the system locale', () => {
    expect(resolveLanguage('en', ['zh-CN'])).toBe('en');
    expect(resolveLanguage('zh-CN', ['en-US'])).toBe('zh-CN');
  });

  it('follows the system locale for "system" and for an unset/unknown value', () => {
    expect(resolveLanguage('system', ['zh-CN'])).toBe('zh-CN');
    expect(resolveLanguage('system', ['en-US'])).toBe('en');
    expect(resolveLanguage(undefined, ['zh-CN'])).toBe('zh-CN');
  });
});
