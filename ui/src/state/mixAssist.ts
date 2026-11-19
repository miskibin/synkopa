// Mix assistant (crates/engine/src/masking.rs): which inserts cover each other, from telemetry,
// and the two things the user can do about it. The UI's only permanent trace is a dot on the
// masker's mixer strip, and only while a collision is reported.
import { createSignal } from 'solid-js';
import { be } from '../api/backend';
import type { Collision } from '../api/telemetry';
import type { Id } from '../api/types';
import { t } from '../lib/i18n';
import { enqueue, resync } from './edit';
import { insName, S } from './store';
import { say } from './ui';

const [collisions, setCollisions] = createSignal<Collision[]>([], { equals: (a, b) => a === b });
export { collisions };

/** Called by the telemetry loop; the decoder hands over a new array only when the report changed. */
export function noteCollisions(list: Collision[]) {
  if (list !== collisions()) setCollisions(list);
}

/** Collisions where `id` is the insert that should give way. */
export const maskedBy = (id: Id) => collisions().filter(c => c.masker === id);

/** "44–56 Hz", "180 Hz–1.1 kHz", "1.4–5.7 kHz". */
export function hzRange(lo: number, hi: number): string {
  const f = (x: number) => (x >= 1000 ? (x / 1000).toFixed(x >= 10000 ? 0 : 1) : String(Math.round(x)));
  if (lo >= 1000) return `${f(lo)}–${f(hi)} kHz`;
  if (hi >= 1000) return `${f(lo)} Hz–${f(hi)} kHz`;
  return `${f(lo)}–${f(hi)} Hz`;
}

const nameOf = (id: Id) => {
  const m = S.project.inserts.find(x => x.id === id);
  return m ? m.name || insName(m) : String(id);
};

/** One sentence: "808 covers Kick at 44–56 Hz". */
export const collisionText = (c: Collision) => t('c.mask.covers', { a: nameOf(c.masker), b: nameOf(c.protect), hz: hzRange(c.lo, c.hi) });

function drop(c: Collision) {
  setCollisions(collisions().filter(x => !(x.masker === c.masker && x.protect === c.protect)));
}

/** Adds the sidechained dynamic EQ to the masker (one undo step, an ordinary EQ in its chain). */
export async function fixCollision(c: Collision) {
  try {
    await enqueue(() => Promise.resolve());
    await be().mixAssist('unmask', { masker: c.masker, protect: c.protect, lo: c.lo, hi: c.hi, center: c.center, overlap: c.overlap, keyDb: c.keyDb });
    await resync();
    drop(c);
    say(t('c.mask.fixed', { a: nameOf(c.masker), b: nameOf(c.protect) }));
  } catch (e) {
    say(t('c.mask.failed', { why: String(e).replace(/^Error: /, '') }));
  }
}

/** Stops reporting the pair (kept in the project, undoable). */
export async function ignoreCollision(c: Collision) {
  try {
    await enqueue(() => Promise.resolve());
    await be().mixAssist('mask_ignore', { a: c.masker, b: c.protect });
    await resync();
    drop(c);
    say(t('c.mask.ignored', { a: nameOf(c.masker), b: nameOf(c.protect) }));
  } catch (e) {
    say(t('c.mask.failed', { why: String(e).replace(/^Error: /, '') }));
  }
}

/** The app setting (Settings ▸ Editing); the engine forgets what it heard when switched off. */
export async function setAssistant(on: boolean) {
  try { await be().mixAssist('mix_assistant', { on }); } catch { /* an old engine: nothing to switch */ }
  if (!on) setCollisions([]);
}
