// Automatic icons for context-menu items, picked from the label (Font Awesome Free solid,
// CC BY 4.0). An item's own `icon` (a 24×24 stroke path) wins over these.
import type { IconDefinition } from '@fortawesome/free-solid-svg-icons';
import {
  faArrowDown, faArrowLeft, faArrowRight, faArrowsUpDown, faArrowUp, faBan, faChartLine, faCheck, faCheckDouble, faClone, faCopy,
  faCrosshairs, faEraser, faEye, faEyeSlash, faFileAudio, faFileCirclePlus, faFillDrip, faFingerprint, faGripLinesVertical,
  faHeadphones, faKeyboard, faLayerGroup, faMagnifyingGlass, faMusic, faPaintbrush, faPaste, faPen, faPencil, faPlay, faPlus,
  faRotateLeft, faScissors, faSliders, faStopwatch, faTableCells, faTrashCan, faVolumeHigh, faVolumeXmark, faWandMagicSparkles,
  faWaveSquare, faCircle, faFolderOpen, faBell, faBolt, faCircleDot, faCloud, faDrumSteelpan, faGuitar, faHandsClapping, faRadio,
  faStairs, faWind, faDrum, faCompactDisc, faHandFist,
  faFloppyDisk, faFilePen, faFileExport, faFileZipper, faFileImport, faGear, faRightFromBracket, faRotateRight,
  faMagnifyingGlassPlus, faMagnifyingGlassMinus, faWindowRestore, faArrowsRotate, faGift, faRepeat,
  faUpRightAndDownLeftFromCenter, faDownLeftAndUpRightToCenter, faComments, faBookOpen,
} from '@fortawesome/free-solid-svg-icons';
import { INST_ICONS } from './instIcons';

/** `eo`: the path uses the even-odd fill rule (our own 24×24 instrument drawings). */
export type MenuGlyph = { vb: string; d: string; eo?: boolean };
const g = (i: IconDefinition): MenuGlyph => {
  const [w, h, , , d] = i.icon;
  return { vb: `0 0 ${w} ${h}`, d: Array.isArray(d) ? d.join(' ') : d };
};

/** Discord's logo (Simple Icons, CC0): Help ▸ Join the Discord. */
const DISCORD: MenuGlyph = { vb: '0 0 24 24', d: 'M20.317 4.3698a19.7913 19.7913 0 00-4.8851-1.5152.0741.0741 0 00-.0785.0371c-.211.3753-.4447.8648-.6083 1.2495-1.8447-.2762-3.68-.2762-5.4868 0-.1636-.3933-.4058-.8742-.6177-1.2495a.077.077 0 00-.0785-.037 19.7363 19.7363 0 00-4.8852 1.515.0699.0699 0 00-.0321.0277C.5334 9.0458-.319 13.5799.0992 18.0578a.0824.0824 0 00.0312.0561c2.0528 1.5076 4.0413 2.4228 5.9929 3.0294a.0777.0777 0 00.0842-.0276c.4616-.6304.8731-1.2952 1.226-1.9942a.076.076 0 00-.0416-.1057c-.6528-.2476-1.2743-.5495-1.8722-.8923a.077.077 0 01-.0076-.1277c.1258-.0943.2517-.1923.3718-.2914a.0743.0743 0 01.0776-.0105c3.9278 1.7933 8.18 1.7933 12.0614 0a.0739.0739 0 01.0785.0095c.1202.099.246.1981.3728.2924a.077.077 0 01-.0066.1276 12.2986 12.2986 0 01-1.873.8914.0766.0766 0 00-.0407.1067c.3604.698.7719 1.3628 1.225 1.9932a.076.076 0 00.0842.0286c1.961-.6067 3.9495-1.5219 6.0023-3.0294a.077.077 0 00.0313-.0552c.5004-5.177-.8382-9.6739-3.5485-13.6604a.061.061 0 00-.0312-.0286zM8.02 15.3312c-1.1825 0-2.1569-1.0857-2.1569-2.419 0-1.3332.9555-2.4189 2.157-2.4189 1.2108 0 2.1757 1.0952 2.1568 2.419 0 1.3332-.9555 2.4189-2.1569 2.4189zm7.9748 0c-1.1825 0-2.1569-1.0857-2.1569-2.419 0-1.3332.9554-2.4189 2.1569-2.4189 1.2108 0 2.1757 1.0952 2.1568 2.419 0 1.3332-.946 2.4189-2.1568 2.4189Z' };

// First match wins: put specific phrases before generic words.
const RULES: [RegExp, IconDefinition][] = [
  [/^auto wokal|^auto[ -]?vocal/i, faWandMagicSparkles],
  [/^zmień nazwę|^nazwa|^rename|^name/i, faPen],
  [/^odcisz|^unmute/i, faVolumeHigh],
  [/^wycisz(?! zaznaczone)|^zakończ odsłuch|^mute(?! selected)|^end preview|^stop preview/i, faVolumeXmark],
  [/^duplikuj|^klonuj|^duplicate|^clone/i, faClone],
  [/^kopiuj|^copy/i, faCopy],
  [/^wytnij|^cut/i, faScissors],
  [/^wklej|^schowek|^paste|^clipboard/i, faPaste],
  [/^podziel|^podział|^granica|^split|^boundary/i, faGripLinesVertical],
  [/^spłaszcz|^flatten/i, faLayerGroup],
  [/^ukryj take|^hide take/i, faEyeSlash],
  [/^pokaż take|^show take/i, faEye],
  [/^użyj całego take|^domyślny compu|^crossfade|^use (the )?whole take|^default comp/i, faLayerGroup],
  [/^usuń|^wyczyść|^delete|^remove|^clear/i, faTrashCan],
  [/^edytuj pattern|^piano roll|^edit pattern/i, faTableCells],
  [/^edytuj sample|^plik z dysku|^dodaj jako kanał samplera|^sampler|^edit sample|^file from disk|^add as sampler channel/i, faFileAudio],
  [/^edytuj wysokość|^wysokość|^edit pitch|^pitch/i, faArrowsUpDown],
  [/^wykryj tempo|^wybij tempo|^tempo|^detect tempo|^tap tempo/i, faStopwatch],
  [/^edytuj punkty|^automatyz|^edit points|^automation/i, faChartLine],
  [/^kanał miksera|^ścieżka klipu|^dodaj efekt|^mixer channel|^(clip )?track( of (the )?clip)?$|^clip track|^add effect|^add fx/i, faSliders],
  [/^zaznacz|^select/i, faCheckDouble],
  [/^make unique/i, faFingerprint],
  [/^odsłuchaj|^preview|^audition/i, faPlay],
  [/^solo/i, faHeadphones],
  [/^wstaw ścieżkę powyżej|^insert track above/i, faArrowUp],
  [/^wstaw ścieżkę poniżej|^insert track below/i, faArrowDown],
  [/^nowa strona|^nowy band|^dodaj|^wstaw|^new page|^new band|^add|^insert/i, faPlus],
  [/^nowy|^new/i, faFileCirclePlus],
  [/^przesuń w lewo|^move left/i, faArrowLeft],
  [/^przesuń w prawo|^move right/i, faArrowRight],
  [/^wypełnij|^fill/i, faFillDrip],
  [/^reset|^przywróć|^restore/i, faRotateLeft],
  [/^ustaw start|^set start/i, faCrosshairs],
  [/^rysuj|^ołówek|^draw|^pencil/i, faPencil],
  [/^maluj|^paint/i, faPaintbrush],
  [/^gumka|^eraser/i, faEraser],
  [/^wbudowane instrumenty|^pianino|^built-in instruments|^piano/i, faMusic],
  [/^klawiatura|^midi|^keyboard/i, faKeyboard],
  [/^szukaj|^search/i, faMagnifyingGlass],
  [/^anuluj|^cancel/i, faBan],
  [/^uzbrój|^nagrywa|^arm|^record/i, faCircle],
  [/^otwórz|^open/i, faFolderOpen],
  [/^pokaż|^show/i, faEye],
  [/^ukryj|^hide/i, faEyeSlash],
  [/wave|^sample/i, faWaveSquare],
];

const cache = new Map<string, MenuGlyph | null>();
/** The mark of a toggle item that is on. */
export const CHECK_GLYPH: MenuGlyph = g(faCheck);
export function menuGlyph(label: string): MenuGlyph | null {
  let v = cache.get(label);
  if (v === undefined) {
    const r = RULES.find(([re]) => re.test(label.trim()));
    v = r ? g(r[1]) : null;
    cache.set(label, v);
  }
  return v;
}

// ---------------------------------------------------------------- application menus
// The menu bar names its glyphs explicitly: its labels are translated and too generic to guess from.
export const APP_GLYPH = {
  new: g(faFileCirclePlus), open: g(faFolderOpen), save: g(faFloppyDisk), saveAs: g(faFilePen),
  export: g(faFileAudio), exportZip: g(faFileZipper), midiImport: g(faFileImport), midiExport: g(faFileExport),
  settings: g(faGear), exit: g(faRightFromBracket),
  undo: g(faRotateLeft), redo: g(faRotateRight), cut: g(faScissors), copy: g(faCopy), paste: g(faPaste),
  dup: g(faClone), del: g(faTrashCan), selAll: g(faCheckDouble),
  expand: g(faUpRightAndDownLeftFromCenter), collapse: g(faDownLeftAndUpRightToCenter), windows: g(faWindowRestore),
  zoomIn: g(faMagnifyingGlassPlus), zoomOut: g(faMagnifyingGlassMinus),
  shortcuts: g(faKeyboard), updates: g(faArrowsRotate), whatsNew: g(faGift), ask: g(faComments), docs: g(faBookOpen), discord: DISCORD,
} satisfies Record<string, MenuGlyph>;

// ---------------------------------------------------------------- instrument picker
// Keyed by engine ids (synth categories, built-in instruments), never by translated labels.

const SYNTH_CATEGORY: Record<string, MenuGlyph> = {
  '808': g(faCircleDot), // the speaker cone of a sub hit
  'Bas': INST_ICONS.bass,
  'Pluck': g(faGuitar),
  'Lead': g(faBolt),
  'Pad': g(faCloud),
  'Klawisze': INST_ICONS.piano,
  'Dzwonki': g(faBell),
  'Dęte/Smyczki': g(faWind),
  'Arp': g(faStairs),
  'Lo-fi': g(faRadio),
  'FX': INST_ICONS.fx,
};
/** A synth preset category's glyph (its presets show it too); a category added later gets the synth wave. */
export const synthCategoryGlyph = (category: string): MenuGlyph => SYNTH_CATEGORY[category] ?? INST_ICONS.synth;

const BUILTIN: Record<string, MenuGlyph> = {
  kick: INST_ICONS.drums,
  clap: g(faHandsClapping),
  hat: g(faDrumSteelpan),
  bass: INST_ICONS.bass,
};
/** Glyph for a built-in instrument id (kick, clap, hat, bass). */
export const builtinGlyph = (inst: string): MenuGlyph => BUILTIN[inst] ?? INST_ICONS.drums;

const LIB_GROUP: Record<string, MenuGlyph> = {
  Kicks: g(faCircleDot), Snares: g(faDrum), Claps: g(faHandsClapping), Hats: g(faDrumSteelpan),
  Cymbals: g(faCompactDisc), Toms: INST_ICONS.drums, Percussion: g(faHandFist), 808: INST_ICONS.bass, Tonal: g(faMusic),
  Orchestra: g(faWind), FX: INST_ICONS.fx, Loops: g(faRepeat),
};
/** Glyph for a standard-library sample group (by its id, not its translated name); other folders get the sample icon. */
export const libGlyph = (group: string): MenuGlyph => LIB_GROUP[group] ?? INST_ICONS.sample;

// What a file or folder of the user's own samples is, read from its name: the same marks as the
// standard library's groups, so a "Kicks" folder looks like the built-in kicks.
const NAME_GLYPH: [RegExp, MenuGlyph][] = [
  [/kick|\bbd\b|stopa/i, LIB_GROUP.Kicks],
  [/snare|\bsd\b|rim|werbel/i, LIB_GROUP.Snares],
  [/clap|snap|kla[sś]/i, LIB_GROUP.Claps],
  [/hat|\bhh\b|\boh\b|\bch\b/i, LIB_GROUP.Hats],
  [/cymb|crash|ride|splash|china|talerz/i, LIB_GROUP.Cymbals],
  [/\btoms?\b/i, LIB_GROUP.Toms],
  [/808|\bsub\b|bass|\bbas\b/i, INST_ICONS.bass],
  [/perc|conga|bongo|shaker|tamb|cowbell|clave|triangle/i, LIB_GROUP.Percussion],
  [/orch|string|brass|violin|cello|horn|trumpet|smycz|flute|timpani/i, LIB_GROUP.Orchestra],
  [/\bfx\b|riser|sweep|impact|noise|whoosh|swell|uplift|downlift|efekt/i, LIB_GROUP.FX],
  [/vo[ck]|vox|voice|wokal|chant|adlib/i, INST_ICONS.vocal],
  [/guit|gitar/i, INST_ICONS.guitar],
  [/piano|keys|rhodes|organ|chord|akord/i, INST_ICONS.piano],
  [/synth|lead|pad|pluck|arp|bell|melod|tonal/i, LIB_GROUP.Tonal],
  [/loop|p[ęe]tl|drum|beat|break/i, INST_ICONS.drums],
];
const nameCache = new Map<string, MenuGlyph | null>();
function nameGlyph(name: string): MenuGlyph | null {
  let v = nameCache.get(name);
  if (v === undefined) { v = NAME_GLYPH.find(([re]) => re.test(name))?.[1] ?? null; nameCache.set(name, v); }
  return v;
}
/** Glyph of a library group row: the standard library's own, else read from the group's name. */
export const libGroupGlyph = (group: string): MenuGlyph => LIB_GROUP[group] ?? nameGlyph(group) ?? INST_ICONS.sample;
/** Glyph of a library file: its standard-library group's, else read from its name, then its group's name. */
export const libFileGlyph = (f: { name: string; group: string }): MenuGlyph => LIB_GROUP[f.group] ?? nameGlyph(f.name) ?? nameGlyph(f.group) ?? INST_ICONS.sample;

/** Other instrument-picker rows. */
export const PICK_GLYPH = {
  emptySynth: INST_ICONS.synth,
  plugin: INST_ICONS.synth,
  keys: INST_ICONS.piano,
  musicBox: INST_ICONS.synth,
  source: INST_ICONS.sample,
} satisfies Record<string, MenuGlyph>;
