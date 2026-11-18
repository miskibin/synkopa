// The error prefixes in `errors.ts` must be exactly what the Rust side names
// (`wave_engine::errors`, pinned by `app/src/ui_prefixes_tests.rs` writing this fixture).
// A rename on either side fails here and there.
import { describe, expect, it } from 'vitest';
import fixture from './__fixtures__/error-prefixes.json';
import { ERR_EXISTS, ERR_INVALID_TARGET, ERR_NO_EDITOR, ERR_NO_STEM_MODEL, ERR_REVISION_CONFLICT } from './errors';

describe('error prefixes agree with the engine', () => {
  it('every prefix match the fixture the engine writes', () => {
    expect(ERR_EXISTS).toBe(fixture.exists);
    expect(ERR_REVISION_CONFLICT).toBe(fixture.revisionConflict);
    expect(ERR_INVALID_TARGET).toBe(fixture.invalidTarget);
    expect(ERR_NO_EDITOR).toBe(fixture.noEditor);
    expect(ERR_NO_STEM_MODEL).toBe(fixture.noStemModel);
  });
});
