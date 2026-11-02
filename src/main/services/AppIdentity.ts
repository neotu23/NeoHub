import { existsSync } from 'node:fs';
import { join } from 'node:path';

/** Keep the selected database, WAL, settings, and Chromium storage together on rebrand. */
export function resolveUserDataDirectory(appDataDirectory: string): string {
  for (const name of ['NeoHub', 'DevHarbor', 'devharbor']) {
    const directory = join(appDataDirectory, name);
    if (existsSync(join(directory, 'devharbor.db'))) return directory;
  }
  return join(appDataDirectory, 'NeoHub');
}
