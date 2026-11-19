/** The command palette's filter: every word of the query must be in the command's text; accents and case are ignored. */
const fold = (s: string) => s.normalize('NFD').replace(/[̀-ͯ]/g, '').replace(/ł/g, 'l').toLowerCase();
export function matchCommand(text: string, query: string): boolean {
  const hay = fold(text);
  return fold(query).split(/\s+/).filter(Boolean).every(w => hay.includes(w));
}
