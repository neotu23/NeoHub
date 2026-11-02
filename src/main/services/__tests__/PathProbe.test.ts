import { mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { PathProbe } from '../PathProbe';

let root: string;
let shell: string;
const loginPath = '/custom/node/bin:/usr/bin:/bin';

function workingShell(): void {
  writeFileSync(
    shell,
    `#!/bin/sh\nprintf 'Welcome\\n__DH_PATH_BEGIN__%s__DH_PATH_END__\\nGoodbye\\n' '${loginPath}'\n`,
    { mode: 0o755 }
  );
}

beforeEach(() => {
  root = mkdtempSync(join(tmpdir(), 'path-probe-'));
  shell = join(root, 'login-shell');
  vi.stubEnv('SHELL', shell);
  vi.stubEnv('PATH', '/usr/bin:/bin');
});

afterEach(() => {
  vi.unstubAllEnvs();
  rmSync(root, { recursive: true, force: true });
});

describe('PathProbe', () => {
  it('gives concurrent callers the completed login-shell PATH', async () => {
    workingShell();
    const probe = new PathProbe();
    expect(await Promise.all([probe.get(), probe.get()])).toEqual([loginPath, loginPath]);
  });

  it('retries a failed shell probe on the next call', async () => {
    writeFileSync(shell, '#!/bin/sh\nexit 1\n', { mode: 0o755 });
    const probe = new PathProbe();
    expect(await probe.get()).toBe('/usr/bin:/bin');
    workingShell();
    expect(await probe.get()).toBe(loginPath);
  });

  it('keeps a successful PATH cached and strips shell banners', async () => {
    workingShell();
    const probe = new PathProbe();
    expect(await probe.get()).toBe(loginPath);
    rmSync(shell);
    expect(await probe.get()).toBe(loginPath);
  });
});
