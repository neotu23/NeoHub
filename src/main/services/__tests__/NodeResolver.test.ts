import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { NodeResolver } from '../NodeResolver';

let root: string;
let bin: string;

function installNode(version: string): void {
  writeFileSync(join(bin, 'node'), `#!/bin/sh\nprintf 'v${version}\\n'\n`, { mode: 0o755 });
}

beforeEach(() => {
  root = mkdtempSync(join(tmpdir(), 'node-resolver-'));
  // Executable paths must be passed literally, including shell metacharacters.
  bin = join(root, 'local $HOME `node` "bin"');
  mkdirSync(bin);
  const shell = join(root, 'login-shell');
  writeFileSync(
    shell,
    `#!/bin/sh\nprintf '__DH_PATH_BEGIN__%s__DH_PATH_END__' '${bin}:/usr/bin:/bin'\n`,
    { mode: 0o755 }
  );
  vi.stubEnv('HOME', root);
  vi.stubEnv('NVM_DIR', join(root, '.nvm'));
  vi.stubEnv('SHELL', shell);
  vi.stubEnv('PATH', '/usr/bin:/bin');
  writeFileSync(join(root, 'package.json'), JSON.stringify({ engines: { node: '>=22' } }));
});

afterEach(() => {
  vi.unstubAllEnvs();
  rmSync(root, { recursive: true, force: true });
});

describe('NodeResolver in a Finder-style environment', () => {
  it('resolves a compatible Node from the login-shell PATH, including literal special characters', async () => {
    installNode('22.22.3');
    expect(await new NodeResolver().resolve({ kind: 'auto' }, root)).toEqual({
      source: 'system',
      version: '22.22.3',
      binDir: bin
    });
  });

  it('discovers an installation after an initially empty scan without restarting', async () => {
    const resolver = new NodeResolver();
    expect(await resolver.list()).toEqual([]);
    installNode('22.22.3');
    expect(await resolver.list()).toEqual([{ source: 'system', version: '22.22.3', binDir: bin }]);
  });

  it('refreshes a previously detected version when starting again after a Node upgrade', async () => {
    const resolver = new NodeResolver();
    installNode('20.0.0');
    expect(await resolver.list()).toEqual([{ source: 'system', version: '20.0.0', binDir: bin }]);
    installNode('22.22.3');
    expect(await resolver.resolve({ kind: 'auto' }, root)).toMatchObject({ version: '22.22.3' });
  });

  it('still rejects a Node version that does not satisfy the project engines', async () => {
    installNode('20.0.0');
    await expect(new NodeResolver().resolve({ kind: 'auto' }, root)).rejects.toThrow(
      'No installed Node version satisfies engines.node ">=22". Installed: 20.0.0'
    );
  });

  it('preserves project version pins and explicit/system preferences', async () => {
    installNode('22.22.3');
    writeFileSync(join(root, '.nvmrc'), '22');
    const resolver = new NodeResolver();
    expect(await resolver.resolve({ kind: 'auto' }, root)).toMatchObject({ version: '22.22.3' });
    expect(await resolver.resolve({ kind: 'system' }, root)).toMatchObject({ binDir: bin });
    expect(await resolver.resolve({ kind: 'explicit', version: '22.22.3' }, root)).toMatchObject({
      version: '22.22.3'
    });
    writeFileSync(join(root, '.nvmrc'), '24');
    await expect(resolver.resolve({ kind: 'auto' }, root)).rejects.toThrow(
      'Project requires Node v24'
    );
  });
});
