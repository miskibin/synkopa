// The pieces an editor's bar is built from, shared by the Playlist and the Piano roll: the tool
// switch (one icon per tool) and the quiet "name value" chip over a native select.
import { For, Show } from 'solid-js';
import { Icon } from './icons';

export interface BarTool<T extends string> { id: T; icon: string; title: string }

/** The tool switch: a radio group of icon buttons. */
export function ToolSeg<T extends string>(props: { label: string; tools: BarTool<T>[]; value: T; onPick: (id: T, el: HTMLElement) => void; size?: number; sw?: number }) {
  return (
    <div class="seg tools" role="radiogroup" aria-label={props.label}>
      <For each={props.tools}>{tl => (
        <button role="radio" aria-checked={props.value === tl.id} classList={{ on: props.value === tl.id }} title={tl.title} onClick={e => props.onPick(tl.id, e.currentTarget)}>
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
