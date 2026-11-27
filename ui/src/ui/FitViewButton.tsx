import { Icon } from './icons';

/** Shared view-fit action beside a timeline overview, matching the Pitch Editor's icon. */
export function FitViewButton(props: { title: string; onClick: (el: HTMLButtonElement) => void }) {
  return <button type="button" class="iconbtn fitview" title={props.title} aria-label={props.title}
    onClick={e => props.onClick(e.currentTarget)}>
    <Icon d="M4 9V4h5 M20 9V4h-5 M4 15v5h5 M20 15v5h-5" size={14} />
  </button>;
}
