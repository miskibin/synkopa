// Refresh the documentation sprite from the effect picker and Channel Rack artwork.
import { readFile, writeFile } from 'node:fs/promises';
import { createRequire } from 'node:module';

const require = createRequire(new URL('../ui/package.json', import.meta.url));
const { faRecordVinyl, faWaveSquare } = require('@fortawesome/free-solid-svg-icons');
const read = path => readFile(new URL(path, import.meta.url), 'utf8');
const [art, instruments] = await Promise.all([
  read('../ui/src/ui/fxArt.ts'),
  read('../ui/src/ui/instIcons.tsx'),
]);

const symbols = Array.from(art.matchAll(/^  (\w+): svg\(`([\s\S]*?)`\),/gm), ([, id, body]) =>
  `  <symbol id="${id}" viewBox="0 0 100 100" fill="none">${body}</symbol>`
);
if (symbols.length < 23) throw new Error('Missing effect artwork');

for (const [id, glyph] of [['sample', faRecordVinyl], ['synth', faWaveSquare]]) {
  const [width, height, , , path] = glyph.icon;
  symbols.push(`  <symbol id="${id}" viewBox="0 0 ${width} ${height}"><path fill="currentColor" d="${[].concat(path).join(' ')}"/></symbol>`);
}
const piano = instruments.match(/^const PIANO = '([^']+)';/m)?.[1];
if (!piano) throw new Error('Missing Synkopa Instruments glyph');
symbols.push(`  <symbol id="piano" viewBox="0 0 24 24"><path fill="currentColor" fill-rule="evenodd" d="${piano}"/></symbol>`);

await writeFile(new URL('../site/docs/images/plugin-icons.svg', import.meta.url), [
  '<svg xmlns="http://www.w3.org/2000/svg">',
  '  <!-- Effect artwork from ui/src/ui/fxArt.ts. Channel glyphs from ui/src/ui/instIcons.tsx.',
  '       Font Awesome Free icons (record-vinyl, wave-square), CC BY 4.0, fontawesome.com. -->',
  ...symbols,
  '</svg>',
  '',
].join('\n'));
