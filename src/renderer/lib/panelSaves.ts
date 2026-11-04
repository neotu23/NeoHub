// Only mounted editors register here; no runtime data or logs are retained by this module.
const saves = new Set<() => Promise<void>>();

export function registerPanelSave(flush: () => Promise<void>): () => void {
  saves.add(flush);
  return () => { saves.delete(flush); };
}

export async function flushPanelSaves(): Promise<void> {
  await Promise.all([...saves].map((flush) => flush()));
}
