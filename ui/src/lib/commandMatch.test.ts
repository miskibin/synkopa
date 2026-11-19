import { describe, expect, it } from 'vitest';
import { matchCommand } from './commandMatch';

describe('matchCommand', () => {
  it('needs every word of the query, in any order, ignoring case', () => {
    expect(matchCommand('File Export audio…', 'exp')).toBe(true);
    expect(matchCommand('File Export audio…', 'audio file')).toBe(true);
    expect(matchCommand('File Export audio…', 'export midi')).toBe(false);
    expect(matchCommand('Edit Undo', '')).toBe(true);
  });
  it('ignores Polish accents', () => {
    expect(matchCommand('Plik Zapisz jako…', 'zapisz')).toBe(true);
    expect(matchCommand('Widok Przeglądarka', 'przegladarka')).toBe(true);
    expect(matchCommand('Edycja Wytnij', 'wklej')).toBe(false);
  });
});
