import { mkdtempSync, mkdirSync, rmSync, statSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { resolveUserDataDirectory } from '../AppIdentity';

let root: string;

beforeEach(() => {
  root = mkdtempSync(join(tmpdir(), 'neohub-identity-'));
});

afterEach(() => rmSync(root, { recursive: true, force: true }));

function existingDatabase(name: string): string {
  const directory = join(root, name);
  mkdirSync(directory, { recursive: true });
  writeFileSync(join(directory, 'devharbor.db'), 'existing projects');
  return directory;
}

describe('NeoHub data directory', () => {
  it('uses NeoHub on a clean installation', () => {
    expect(resolveUserDataDirectory(root)).toBe(join(root, 'NeoHub'));
  });

  it('keeps reading projects from the previous packaged app', () => {
    const legacy = existingDatabase('DevHarbor');
    mkdirSync(join(root, 'NeoHub'));
    expect(statSync(join(resolveUserDataDirectory(root), 'devharbor.db')).ino).toBe(
      statSync(join(legacy, 'devharbor.db')).ino
    );
  });

  it('keeps reading projects from the previous development app', () => {
    const legacy = existingDatabase('devharbor');
    expect(statSync(join(resolveUserDataDirectory(root), 'devharbor.db')).ino).toBe(
      statSync(join(legacy, 'devharbor.db')).ino
    );
  });

  it('prefers an existing NeoHub database over a legacy one', () => {
    existingDatabase('DevHarbor');
    const current = existingDatabase('NeoHub');
    expect(resolveUserDataDirectory(root)).toBe(current);
  });
});
