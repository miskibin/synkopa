// The pieces an editor's bar is built from, shared by the Playlist and the Piano roll: the tool
// switch (one icon per tool), the quiet "name value" chip over a native select, and the grid chip
// (Snap) whose choices open as a compact grid instead of a long list.
import { For, Show } from 'solid-js';
import { createSignal, effect, onCleanup } from '../lib/reactive';
import { Icon } from './icons';

export interface BarTool<T extends string> { id: T; icon: string; title: string }

/** The tool switch: a radio group of icon buttons. */
export function ToolSeg<T extends string>(props: { label: string; tools: BarTool<T>[]; value: T; onPick: (id: T, el: HTMLElement) => void; size?: number; sw?: number }) {
  return (
    <div class="seg tools" role="radiogroup" aria-label={props.label}>
      <For each={props.tools}>{tl => (
        <button role="radio" aria-checked={props.value === tl.id ? 'true' : 'false'} class={{ on: props.value === tl.id }} title={tl.title} onClick={e => props.onPick(tl.id, e.currentTarget)}>
          <Icon d={tl.icon} size={props.size ?? 14} sw={props.sw} />
        </button>
      )}</For>
    </div>
  );
}

export type ChipOption = [label: string, value: string];

/** A setting as a chip: its dim name, then the value (a native select without the form-field box). */
export function ChipSelect(props: { label: string; title?: string; value: string; options: ChipOption[]; group?: { label: string; options: ChipOption[] }; onChange: (v: string, el: HTMLSelectElement) => void }) {
  return (
    <label class="chip" title={props.title}>{props.label}
      <select value={props.value} onChange={e => props.onChange(e.currentTarget.value, e.currentTarget)}>
        <For each={props.options}>{([l, v]) => <option value={v}>{l}</option>}</For>
        <Show when={props.group}>{g => <optgroup label={g().label}><For each={g().options}>{([l, v]) => <option value={v}>{l}</option>}</For></optgroup>}</Show>
      </select>
    </label>
  );
}

export interface GridChipProps {
  label: string;
  title?: string;
  value: string;
  /** The modes above the grid (auto, off): a wide button each, with an optional dim note (what auto resolves to now). */
  modes?: { label: string; value: string; note?: string }[];
  /** The grid's rows of values, each with an optional caption (Triplets). */
  sections: { label?: string; options: ChipOption[] }[];
  /** The grid value a mode stands for now (auto → 1/16): outlined, so the grid shows what is in effect. */
  implied?: string;
  onChange: (v: string, el: HTMLElement) => void;
}

/**
 * A setting with many short values (Snap) as a chip whose choices open as a grid: every value in
 * view at once, sorted coarse → fine left to right, instead of a tall list to scan. Arrow keys move
 * between the choices, Enter picks, Esc closes.
 */
export function GridChip(props: GridChipProps) {
  const [open, setOpen] = createSignal(false);
  let root!: HTMLDivElement, chip!: HTMLButtonElement;
  effect(() => {
    if (!open()) return;
    const away = (e: PointerEvent) => { if (!root.contains(e.target as Node)) setOpen(false); };
    document.addEventListener('pointerdown', away, true);
    // the open grid takes the keyboard: the cell under focus, else the chosen one
    queueMicrotask(() => root.querySelector<HTMLElement>('.gcpop [aria-checked="true"]')?.focus({ preventScroll: true }));
    onCleanup(() => document.removeEventListener('pointerdown', away, true));
  });
  const all = () => [...(props.modes ?? []).map(m => [m.label, m.value] as ChipOption), ...props.sections.flatMap(s => s.options)];
  const shown = () => all().find(o => o[1] === props.value)?.[0] ?? props.value;
  const pick = (v: string) => { setOpen(false); chip.focus({ preventScroll: true }); if (v !== props.value) props.onChange(v, chip); };
  const cols = () => Math.max(...props.sections.map(s => s.options.length), 1);
  const key = (e: KeyboardEvent) => {
    if (!open()) return;
    if (e.key === 'Escape') { e.preventDefault(); e.stopPropagation(); setOpen(false); chip.focus({ preventScroll: true }); return; }
    const btns = Array.from(root.querySelectorAll<HTMLButtonElement>('.gcpop button'));
    const i = btns.indexOf(document.activeElement as HTMLButtonElement);
    if (i < 0) return;
    const r = btns[i].getBoundingClientRect(), cx = r.left + r.width / 2, cy = r.top + r.height / 2;
    // the nearest button in the arrow's direction (rows differ in length, so by geometry, not index)
    const dir = ({ ArrowLeft: [-1, 0], ArrowRight: [1, 0], ArrowUp: [0, -1], ArrowDown: [0, 1] } as Record<string, [number, number]>)[e.key];
    if (!dir) return;
    e.preventDefault(); e.stopPropagation();
    let best: HTMLButtonElement | undefined, bd = Infinity;
    for (const b of btns) {
      if (b === btns[i]) continue;
      const q = b.getBoundingClientRect(), dx = q.left + q.width / 2 - cx, dy = q.top + q.height / 2 - cy;
      const along = dx * dir[0] + dy * dir[1], across = Math.abs(dx * dir[1] + dy * dir[0]);
      if (along <= 1) continue;
      const d = along + across * 3;
      if (d < bd) { bd = d; best = b; }
    }
    best?.focus({ preventScroll: true });
  };
  return (
    <div class="toolmenu gridchip" ref={root} onKeyDown={key}>
      <button ref={chip} class={['chip', { on: open() }]} title={props.title} aria-haspopup="true" aria-expanded={open() ? 'true' : 'false'} onClick={() => setOpen(v => !v)}>
        {props.label}<b class="gcval">{shown()}</b>
        <svg width="8" height="8" viewBox="0 0 8 8"><path d="M1 2.5l3 3 3-3" fill="none" stroke="currentColor" stroke-width="1.3" /></svg>
      </button>
      <Show when={open()}>
        <div class="gcpop" role="radiogroup" aria-label={props.label} style={{ '--cols': cols() }}>
          <Show when={props.modes?.length}>
            <div class="gcmodes">
              <For each={props.modes}>{m => (
                <button role="radio" aria-checked={m.value === props.value ? 'true' : 'false'} class={{ on: m.value === props.value }} onClick={() => pick(m.value)}>
                  <span>{m.label}</span><Show when={m.note}><small>{m.note}</small></Show>
                </button>
              )}</For>
            </div>
          </Show>
          <For each={props.sections}>{s => (
            <>
              <Show when={s.label}><div class="gccap">{s.label}</div></Show>
              <div class="gcgrid">
                <For each={s.options}>{([l, v]) => (
                  <button role="radio" aria-checked={v === props.value ? 'true' : 'false'} class={{ on: v === props.value, implied: v !== props.value && v === props.implied }} onClick={() => pick(v)}>{l}</button>
                )}</For>
              </div>
            </>
          )}</For>
        </div>
      </Show>
    </div>
  );
}
