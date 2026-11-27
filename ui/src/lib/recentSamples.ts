import type { LibFile } from '../api/types';

export const RECENT_LIMIT = 30;
export const RECENT_KEY = 'wave.browser.recent';

/** A newest-first, unique-by-path list. Keep the latest metadata for a reused file. */
export function addRecent(list: readonly LibFile[], file: LibFile): LibFile[] {
  return [file, ...list.filter(f => f.path !== file.path)].slice(0, RECENT_LIMIT);
}

/** Storage may be corrupt or from an older version; only accept valid sample metadata. */
export function parseRecent(raw: string | null): LibFile[] {
  try {
    const data: unknown = JSON.parse(raw ?? '[]');
    if (!Array.isArray(data)) return [];
    const result: LibFile[] = [];
    for (const f of data) {
      if (!f || typeof f.path !== 'string' || !f.path || typeof f.name !== 'string' || typeof f.group !== 'string' || result.some(x => x.path === f.path)) continue;
      result.push({ path: f.path, name: f.name, group: f.group, ...(Number.isInteger(f.root) && f.root >= 0 && f.root <= 127 ? { root: f.root } : {}), ...(f.pitched === true ? { pitched: true } : {}) });
      if (result.length === RECENT_LIMIT) break;
    }
    return result;
  } catch { return []; }
}
