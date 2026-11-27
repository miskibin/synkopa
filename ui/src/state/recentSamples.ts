import type { LibFile } from '../api/types';
import { createSignal } from '../lib/reactive';
import { addRecent, parseRecent, RECENT_KEY } from '../lib/recentSamples';

function load(): LibFile[] {
  try { return parseRecent(localStorage.getItem(RECENT_KEY)); } catch { return []; }
}
const [recent, setRecent] = createSignal(load());
export { recent as recentSamples };

/** Call only after a library file has actually been imported, not on preview. */
export function rememberSample(file: Pick<LibFile, 'path'> & Partial<LibFile>) {
  if (typeof file.name !== 'string' || typeof file.group !== 'string') return;
  const next = addRecent(recent(), { path: file.path, name: file.name, group: file.group, root: file.root, pitched: file.pitched });
  setRecent(next);
  try { localStorage.setItem(RECENT_KEY, JSON.stringify(next)); } catch { /* private window */ }
}
