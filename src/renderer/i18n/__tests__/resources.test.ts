import { describe, expect, it } from 'vitest';
import { en } from '../locales/en';
import { zhCN } from '../locales/zh-CN';

type Dict = { [key: string]: unknown };

/** Flatten to `a.b.c` -> leaf value so key sets can be compared across locales. */
function flatten(dict: Dict, prefix = ''): Record<string, string> {
  const out: Record<string, string> = {};
  for (const [key, value] of Object.entries(dict)) {
    const path = prefix ? `${prefix}.${key}` : key;
    if (typeof value === 'string') out[path] = value;
    else Object.assign(out, flatten(value as Dict, path));
  }
  return out;
}

/** `{{token}}` placeholders a string interpolates. */
function placeholders(value: string): string[] {
  return [...value.matchAll(/\{\{(\w+)\}\}/g)].map((m) => m[1]!).sort();
}

const enKeys = flatten(en as unknown as Dict);
const zhKeys = flatten(zhCN as unknown as Dict);

describe('zh-CN dictionary', () => {
  it('covers every English key and adds none', () => {
    expect(Object.keys(zhKeys).sort()).toEqual(Object.keys(enKeys).sort());
  });

  it('interpolates the same placeholders as English', () => {
    const mismatched = Object.keys(enKeys).filter(
      (key) =>
        JSON.stringify(placeholders(enKeys[key]!)) !== JSON.stringify(placeholders(zhKeys[key]!))
    );
    expect(mismatched).toEqual([]);
  });

  it('translates every string (empty values only where English is empty too)', () => {
    // A few values are intentionally identical: `ms`/`CPU` are universal, the folder-move
    // menu label is just an arrow plus the folder name, and the delete-folder prefix is
    // empty when no apps move.
    const identicalByDesign = new Set([
      'settings.unitMs',
      'dashboard.statCpu',
      'sidebar.menu.moveToFolder'
    ]);
    const empty = Object.keys(zhKeys).filter((key) => zhKeys[key] === '' && enKeys[key] !== '');
    expect(empty).toEqual([]);

    const untranslated = Object.keys(zhKeys).filter(
      (key) => zhKeys[key] === enKeys[key] && !identicalByDesign.has(key)
    );
    expect(untranslated).toEqual([]);
  });

  it('has one/other forms for every pluralised key', () => {
    const bases = new Set(
      Object.keys(enKeys)
        .filter((key) => /_(one|other)$/.test(key))
        .map((key) => key.replace(/_(one|other)$/, ''))
    );
    expect(bases.size).toBeGreaterThan(0);
    for (const base of bases) {
      for (const suffix of ['_one', '_other']) {
        expect(enKeys[`${base}${suffix}`], `${base}${suffix} (en)`).toBeDefined();
        expect(zhKeys[`${base}${suffix}`], `${base}${suffix} (zh)`).toBeDefined();
      }
    }
  });
});
