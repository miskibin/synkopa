# Agent pitch workflow — benchmark results on real recordings

Measured 2026-11-13 on branch `agent-pitch-editing`. Every number below comes from the JSON
reports these commands wrote; nothing was re-run selectively. Listener scores do not exist yet:
the blind kit is built, nobody has rated it.

## What is measured

The **agent** method is the workflow an agent runs through the CLI, executed with the same
engine functions: `pitch inspect` statistics → `pitch compare` (when there is a reference) →
`pitch suggest` with its defaults (`--min-cents 15 --max-cents 60`, A440 grid) → the planner's
edits applied with the `editTune` op's code → rendered by the pitch editor's renderer. The
**tune-ref** method is existing Tune to Reference (correction/vibrato/influence 100 %) on the same
cases, for comparison. Each render is analysed again (RMVPE) and scored:

* **frames** — |rendered F0 − truth| on stable voiced frames of scored notes (edges, glides and
  detector slips left out), median / P90 / MAE in cents, before → after.
* **slips** — notes with a known injected error: note-level error before → after, share fixed
  to within 20 ct.
* **correct notes moved** — notes the original sings within ±20 ct of a semitone, untouched by
  the injection: share moved more than 15 ct by the processing (unwanted correction).
* **intended kept** — deliberate differences (a note moved by ±200 ct = another melodic note;
  GTSinger notes flagged `glissando`): share moved less than 20 ct.
* **natural moved** — notes with no known intention (the original itself off the grid):
  reported only.
* octave errors, interval estimation (`semitones` of the best interval vs. the known one),
  `related` recognition, unrelated reference → audio bit-identical, note matching against known
  note correspondences, vibrato rate/depth change on held notes (> 0.6 s), note onset drift of the
  re-segmented render, voiced/unvoiced flips away from voicing edges, the correction-size
  histogram, run time.

A summary fails when it has no case, no criterion or a failed case (the old report passed
vacuously; fixed). Failures to build a case are reported as failed cases; dataset pairs that do
not exist (GTSinger segments with one index but different lyrics) are listed as `skipped`.

## Environment and data

* Windows 11, Intel Core i5-12400F (6 cores), 32 GB; Rust 1.99.0; release build.
* Detector: RMVPE (`rmvpe.onnx`, SHA-256 `5370e71a…660fd`, the model Synkopa installs), on CPU.
* **Annotated VocalSet** — Zenodo 7061507 (annotations) + 1442513 (`VocalSet11.zip` audio),
  CC BY 4.0; 20 professional singers, hand-checked F0 and note boundaries. 30 recordings
  (spread over singers/techniques) for the controlled cases, 30 cross-singer pairs, 15
  other-key references, 15 unrelated references.
* **GTSinger** — English subset fetched by `scripts/fetch_gtsinger.py` (184 segments, 233 MB),
  CC BY-NC-SA 4.0: the same phrase sung plainly and with vibrato / glissando by the same singer,
  and "Someone Like You" by a tenor and an alto a fourth apart. Word/note timing and score MIDI
  per note, technique flags; **no F0 annotation**: the frame truth there is the RMVPE analysis of
  the unmodified recording, which only measures what the processing changed.
* **MedleyDB** — the full dataset needs an access request; the open **MedleyDB_Sample**
  (Zenodo 1438309, CC BY-SA 4.0) has one song with an annotated lead vocal
  (`LizNelson_Rainfall`, stems 02/03 are backing vocals). 12 phrases of 3–12 s.
* Injected errors are made with Synkopa's own PSOLA on the real recording; truth is the dataset's
  annotation (VocalSet, MedleyDB) or the unmodified recording (GTSinger).

## Commands

```sh
python crates/tune-bench/scripts/fetch_zenodo.py --out D:/wave-datasets vocalset medleydb
python crates/tune-bench/scripts/fetch_gtsinger.py --out D:/wave-datasets/gtsinger
B="cargo run --release -p wave-tune-bench --"
M="--rmvpe <path>/rmvpe.onnx"
$B agent-vocalset --root D:/wave-datasets/vocalset  $M --limit 30 --json agent-vocalset.json
$B agent-gtsinger --root D:/wave-datasets/gtsinger  $M --limit 16 --json agent-gtsinger.json
$B agent-medley   --root D:/wave-datasets/medleydb  $M --limit 12 --json agent-medley.json
$B estimation --vocalset D:/wave-datasets/vocalset --gtsinger D:/wave-datasets/gtsinger \
              --medley D:/wave-datasets/medleydb $M --limit 40 --json estimation.json
$B listen --out kit/ --vocalset D:/wave-datasets/vocalset --gtsinger D:/wave-datasets/gtsinger --fragments 12 --seed 7 $M
$B listen-score --kit kit/ --ratings kit/ratings.csv
```

`--dump DIR` writes every case's take, reference, output WAV and the planner's decisions.
Run time: VocalSet 1173 s (192 runs), GTSinger 574 s, MedleyDB 255 s; real-time factor ≈ 0.36
(analysis of take, reference and render dominates).

## Detector accuracy (estimation)

| data | frames | RPA (< 50 ct) | median abs | P90 abs | octave errs | voicing recall / false alarm |
| --- | --- | --- | --- | --- | --- | --- |
| VocalSet, 40 recordings vs annotation | 50 042 | 81.4 % | 19.0 ct | 72.5 ct | 1 110 frames (2.2 %) | 98.2 % / 7.3 % |
| MedleyDB lead vocal vs annotation | 31 796 | 95.9 % | 3.1 ct | 18.9 ct | 0 | 99.2 % / 17.3 % |
| GTSinger, 561 notes vs score MIDI | — | 93.2 % of notes | 20.3 ct | 45.4 ct | 0 | — |

VocalSet's larger error comes from its techniques (trills, fry, breathy, belt, fast scales) and
from the annotation's own octave choices; the score comparison bounds rather than measures the
detector (singers deviate from scores).

## Results

`agent` / `tune-ref`; ✗ = criterion failed.

### VocalSet — controlled (a real take with injected errors against its untouched self)

| group | method | slip error before → after (fixed < 20 ct) | correct moved | intended kept | frame median |
| --- | --- | --- | --- | --- | --- |
| slips ±25/±50 | agent | 36.7 → 25.0 ct (40 %) ✗ | 18 % ✗ (n 11) | — | 10.4 → 11.3 |
| | tune-ref | 36.7 → 5.2 ct (100 %) | 9 % ✗ | — | 10.4 → 13.2 |
| slips ±35 + drift 30 ct/s | agent | 38.2 → 23.1 ct (56 %) ✗ | — | — | 36.7 → 17.7 |
| | tune-ref | 38.2 → 6.4 ct (100 %) | — | — | 36.7 → 11.5 |
| slips ±40 + intended ±200 | agent | 39.2 → 24.9 ct (50 %) ✗ | 25 % ✗ (n 8) | **100 %** (16) | 21.9 → 14.5 |
| | tune-ref | 39.2 → 5.3 ct (100 %) | 12 % ✗ | **0 %** ✗ | 21.9 → 10.4 |
| large ±100 | agent (default 60 ct limit) | 102 → 102 ct (kept as intended) | 0 % | — | |
| | agent `--max-cents 250` | 102 → 61 ct (42 %) ✗ | 50 % ✗ (n 4) | — | 90.4 → 39.2 |
| | tune-ref | 102 → 6 ct (92 %) | 0 % | — | 90.4 → 10.2 |
| large ±200 | agent (default) | 197 → 195 ct (kept) | 0 % | — | |
| | agent `--max-cents 250` | 197 → 114 ct (36 %) ✗ | 12 % | — | 193.6 → 15.5 |
| | tune-ref | 197 → 10 ct (86 %) | 12 % | — | 193.6 → 12.5 |

### VocalSet — real other singers, other keys, unrelated references

| group | method | slips before → after | correct moved | interval exact | related | note matching | vibrato depth Δ P90 | onset drift P90 |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| cross-singer (30 real pairs) | agent | 41.9 → 34.9 ct ✗ | 12 % ✗ | 17/30 ✗ | 17/30 | 66 % ✗ | 2.3 ct | 5 ms |
| | tune-ref | 41.9 → **51.3** ct ✗ | **33 %** ✗ | 20/30 | 19/30 | — | 25.4 ct ✗ | 45 ms ✗ |
| reference in another key (+3/−5/+7/−12/+4/−2 st) | agent | 42.2 → 27.8 ct ✗ | 15 % ✗ | **15/15** | 15/15 | 98 % | 2.8 ct | 10 ms |
| | tune-ref | 42.2 → **189.9** ct ✗ | **81 %** ✗ | 2/15 ✗ | — | — | 22.0 ct ✗ | 50 ms ✗ |
| unrelated reference (15) | agent | audio untouched **15/15** | | | | | | |
| | tune-ref | audio untouched 13/15 ✗ | | | | | | |

### GTSinger — ornaments and a tenor/alto pair

| group | method | slips before → after | correct moved | ornaments kept | interval | related | vibrato depth Δ P90 |
| --- | --- | --- | --- | --- | --- | --- | --- |
| natural glissando take, plain reference (9) | agent | — | — | 78 % ✗ | 9/9 | 5/9 ✗ | 6.0 ct |
| | tune-ref | — | — | 39 % ✗ | 9/9 | 9/9 | 17.6 ct ✗ |
| natural vibrato take, plain reference (7) | agent | — | — | 67 % ✗ | 6/7 ✗ | 5/7 ✗ | 0.0 ct |
| | tune-ref | — | — | 22 % ✗ | 7/7 | 7/7 | 22.6 ct ✗ |
| slips, glissando reference (9) | agent | 40.2 → 29.8 ct ✗ | 8 % ✗ | — | 9/9 | 8/9 ✗ | 1.3 ct |
| | tune-ref | 40.2 → 45.3 ct ✗ | 56 % ✗ | — | 9/9 | 9/9 | 4.7 ct |
| slips, vibrato reference (7) | agent | 40.6 → 22.4 ct ✗ | 6 % ✗ | — | 6/7 ✗ | 6/7 ✗ | 5.8 ct |
| | tune-ref | 40.6 → 37.9 ct ✗ | 56 % ✗ | — | 7/7 | 7/7 | 18.1 ct ✗ |
| tenor ↔ alto, a fourth apart (10) | agent | 40.1 → 31.6 ct ✗ | 6 % ✗ | — | **10/10** (±5 st) | 5/10 ✗ | 0.4 ct |
| | tune-ref | 40.1 → **79.1** ct ✗ | 15 % ✗ | — | 0/10 ✗ | 3/10 ✗ | 0.0 ct |

### MedleyDB — a song's lead vocal

| group | method | slips before → after (fixed) | correct moved | frame median | onset drift P90 |
| --- | --- | --- | --- | --- | --- |
| slips against the untouched phrase (12) | agent | 42.9 → 27.3 ct (47 %) ✗ | 23 % ✗ | 2.4 → 2.8 ct | 5 ms |
| | tune-ref | 42.9 → 13.9 ct (89 %) | 19 % ✗ | 2.4 → 7.1 ct | 50 ms ✗ |
| backing vocal as the reference (18 pairs) | agent | — | **0 %** | 1.7 → 1.7 ct | 0 ms |
| | tune-ref | — | **38 %** ✗ (frame MAE 2.4 → 98.8 ct) | | 30 ms ✗ |

No octave errors were introduced by the agent in any suite; V/UV corruption stayed ≤ 0.1 % for
both methods except tune-ref with backing-vocal references (1.0 %).

## What this shows

* **Safety is where the agent workflow is better.** With another singer, another key, an
  ornamented reference or a harmony part, existing Tune to Reference often makes the take worse
  (cross-singer slip error 42 → 51 ct, other-key 42 → 190 ct: it only handles octave
  transpositions; harmony references move 38 % of correct notes). The agent workflow estimated
  every non-octave transposition exactly (15/15 VocalSet, 10/10 tenor/alto), kept every
  deliberate ±200 ct note (16/16, tune-ref 0/16), never touched audio for an unrelated reference
  (15/15), left the lead alone against backing vocals (0 % moved) and kept vibrato and timing.
* **Correction strength is where it is worse.** Against a reference of the same singer and part,
  Tune to Reference fixes 89–100 % of injected slips; the conservative planner fixes 40–58 %. The
  planner keeps notes it cannot judge (short, sliding, unsure, two pitches in one detected
  note, a matched reference note more than 60 ct away) and treats a semitone-or-more difference
  as intended. `--max-cents 250` recovers the ±200 ct cases partly (median 194 → 16 ct) but not
  ±100 ct reliably. An agent that knows the take is far off can also use Tune to Reference or
  explicit `pitch edit --target`.
* **Unwanted corrections are not solved.** 6–25 % of correctly sung notes moved more than 15 ct
  (criterion ≤ 5 %). Inspected cases (MedleyDB): mostly short notes where RMVPE's median
  disagrees with the human annotation by ~20 ct, and detected notes that span a slip and its
  neighbour. A first fix (keep notes holding two pitches) cut MedleyDB's rate from 37.5 % to 23 %.
* **`related` is strict.** Same-singer plain vs. ornamented phrases and the tenor/alto pairs were
  recognised as the same melody in only 50–89 % of cases (required: ≥ 50 % of matched notes agree
  within 50 ct); unrecognised references change nothing, which is the intended failure mode.
* The acceptance targets in the task (frame median < 20 ct on stable regions: met by the agent
  in every group except ±100/±200 ct errors at the default limit, which it keeps by design; ≥ 95 % intended differences kept: met for VocalSet ±200 ct notes, not for GTSinger
  glissandi (78 %) / vibrato notes (67 %); significant reduction of introduced errors: partial;
  no significant degradation of correct notes: not met) are reported as measured.

## Limitations

* No listening results: the kit (`listen`, 12 fragments, original / tune-ref / agent) is built
  but has not been rated. No perceptual claim is made.
* GTSinger has no F0 annotation; its frame truth is the unmodified recording's analysis.
  MedleyDB is one song (the sample); the full dataset needs an access request.
* The reference tuning grid is A440 unless `--tuning` is given; unaccompanied datasets have no
  backing track to measure it from, so singers who sit consistently sharp/flat are corrected
  toward A440 in these runs.
* Injected errors are constant per note (plus a drift group); real intonation errors also
  include scoops and wavering that these cases do not model.

---

# Sung note transcription — the pitch editor's notes and *Copy notes* (issue #76)

Measured 2026-11-15, RMVPE, same machine as above. Every number comes from the JSON reports of
the commands below.

## What is measured

Note-level accuracy as in mir_eval.transcription / MIREX note tracking: a detected note matches
an annotated one when its onset is within 50 ms and its pitch within 50 cents (**COnP**); **COnPOff**
also needs the offset within max(50 ms, 20 % of the note); **COn** is onsets only. Notes are
paired by a maximum bipartite matching; the F1 is reported. Error kinds are counted apart:
annotated notes split into several (`frag` = extra pieces), several annotated notes in one
(`merged`), annotated notes without a note (`missed`), notes over silence/transitions (`spur`)
and wrong keys (`wrongP`). What is scored is what the user gets: **pasted** = *Copy notes* as
it pastes now (sung timing, semitone keys, monophonic); **grid 1/16** = the earlier paste
(1/16 grid at 120 BPM).

* **Annotated VocalSet** (282 exercises after leaving out 6 takes whose note annotation is
  1.2–1.5 s off the audio and 12 whose fast runs are annotated only at their ends). Pitch truth
  is the median of the hand-checked F0 of each note — the score column is often a semitone off
  what was sung. VocalSet annotates the glide between legato notes as a separate *Transition*
  and starts the next note where the pitch has **settled**; the change a listener hears (and
  where a Piano roll note belongs) is during the glide, so scores are given both ways:
  **mid** = a legato note starts at the middle of its annotated transition, **settled** = the
  annotation as published.
* **GTSinger** English subset, plain singing (Control_Group, 184 segments of real songs with
  aligned score notes: repeated notes on new syllables, melismas, slides). The technique groups
  (vibrato/glissando) ornament the written note, so they are reported apart only.
* **Held out**: the parameters were chosen on VocalSet's training singers and GTSinger's
  Alto-1/Tenor-1 (`--maximin`: the best *worse* of the two datasets' COnP); the rows below are
  VocalSet's test singers (female2/8, male3/5/10) and GTSinger's Alto-2, never used for tuning.

## Commands

```sh
B="cargo run --release -p wave-tune-bench --"
$B transcribe-grid --maximin --root D:/wave-datasets/vocalset --limit 300 \
   --gtsinger D:/wave-datasets/gtsinger --glimit 185 --json final.json --dump dump/
# the tuning sweep that chose the defaults (`TrackParams::default`):
$B transcribe-grid --maximin … --sigmas 0.6,0.8,1.0,1.2 --switches 4,6,8,12 --smooths 0.15,0.2,0.25 \
   --glides 45,60 --mins 0.05,0.08 --onsets 0,6,9,12 --glide-notes 0.12,0.16 --steadies 0.05,0.07 \
   --arrives 0.5,0.7 --snaps 0,2,3
$B transcribe --root D:/wave-datasets/vocalset --limit 80 --basic-pitch nmp.onnx   # Basic Pitch baseline
cargo test --release -p wave-engine note_track_speed -- --ignored --nocapture       # cost
```

## Results (held-out singers)

Before = the earlier segmenter and paste (1/16 grid); after = the note tracker and the new paste.

| data | | COn | COnP | COnPOff | notes / annotated | extra pieces | merged | wrong key |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| VocalSet test, mid | before | 0.398 | 0.305 | 0.142 | 1776 / 687 | 1010 | 16 | 120 |
| | **after** | **0.671** | **0.570** | **0.419** | 984 / 687 | 290 | 13 | 59 |
| VocalSet test, settled | before | 0.286 | 0.221 | 0.091 | 1776 / 687 | 957 | 7 | 109 |
| | after | 0.257 | 0.194 | 0.123 | 984 / 687 | 268 | 13 | 57 |
| GTSinger songs (Alto-2) | before | 0.433 | 0.296 | 0.147 | 793 / 647 | 221 | 62 | 72 |
| | **after** | **0.471** | **0.384** | **0.236** | 753 / 647 | 164 | 53 | 62 |
| GTSinger technique groups | before (pasted) | 0.418 | 0.280 | 0.145 | 1886 / 1104 | 867 | 79 | 154 |
| | after | 0.497 | 0.393 | 0.228 | 1294 / 1104 | 307 | 96 | 164 |

Notes per sung note by VocalSet technique (all 282 takes), before → after: vibrato 6.0 → 1.56,
slow piano 4.1 → 1.38, messa di voce 17.9 → 2.78, long forte 16.2 → 2.92, trill 23.2 → 6.06,
trillo 10.9 → 3.71, straight 1.4 → 1.20, belt 2.3 → 1.27, fast forte 1.7 → 0.95, fast piano
1.5 → 0.99.

**Basic Pitch** (Spotify `nmp.onnx`, icassp_2022, library-default decoding, made monophonic),
80 VocalSet takes, settled onsets, notes as detected: COn 0.188, COnP 0.133, COnPOff 0.059 —
below the earlier segmenter on the same takes (0.217 / 0.171 / 0.074). It also splits sustained
sung notes; its paper notes sung pitch "fluctuation around a center pitch" makes singing hard.

**Cost**: a minute of singing takes 1.8 ms of note tracking and 49 ms of spectral onset strength
(release, i5-12400F) — under 1 % of the RMVPE analysis it runs with.

## What this shows

* Vibrato, drift, scoops and slides no longer become notes: on held-out exercises the notes per
  sung note fall from 2.6 to 1.4 and twice as many notes are placed right (COnP 0.31 → 0.57,
  transition-middle onsets). On real songs COnP rises 0.30 → 0.38 and COnPOff 0.15 → 0.24.
* Against VocalSet's *settled* onsets the onset-only scores do not improve (COnP 0.22 → 0.19):
  the tracker starts a legato note during its glide (that is where the change is heard, and
  where GTSinger's song annotations put it); VocalSet starts it after the glide. Offsets and
  full-note scores still improve there (COnPOff 0.09 → 0.12).
* The 1/16 grid itself cost accuracy (it moves notes up to half a step and collapsed quick
  ones), so *Copy notes* now keeps the sung timing.

## Effect on Tune to Reference and the agent workflow

Both work on the editor's notes, so they were run again (same commands as above, `--limit 30 /
16 / 12`) against a build of `main` before this change. Failed criteria, before → after:
VocalSet **54 → 49**, GTSinger **36 → 34**, MedleyDB **7 → 8**. Vibrato is kept far better
(vibrato-depth change passes in 6 more suites: a note now holds its whole vibrato, so a
correction moves the centre instead of flattening the swings) and the planner corrects more of
the injected slips (introduced-error reduction passes in 3 more suites).

Two things changed with it:
* `pitch compare` called a reference related on as little as one matched note (an unrelated
  exercise with one coincidental note was "the same melody" once its fragments were gone). It
  now needs at least 3 confidently matched notes, 55 % of them agreeing (was 50 %): on these
  suites no unrelated reference or harmony part is taken for the melody (4 of 33 were at 50 %),
  and 110 of 141 related references are (117). Refusing changes nothing; accepting a wrong one
  edits the take.
* The agent suites' *note onset drift* check re-segments the render to see that timing was
  kept; it now uses the plain splitter, because the note tracker places a legato change along
  its glide, which a pitch correction legitimately reshapes.

Still failing after (not before): MedleyDB Tune to Reference with a **harmony** part as the
reference reports one octave error and 1.28 % voicing changes (limit 1 %). Tune to Reference
at 100 % follows whatever it is given; before and after it pulls the lead onto the harmony in
most of those cases (frame error ~2 → 230–510 ct before, the same class after), one note now
by more than 6 semitones. Vibrato-depth change fails in two suites that passed (GTSinger agent
with a glissando reference, MedleyDB Tune to Reference on slips: P90 17 ct, limit 15).

## Limitations

* Trills are transcribed as the two notes they alternate between (VocalSet annotates a trill as
  one note); long notes sung with a crescendo or a drift of about a semitone (messa di voce,
  long forte) still split, 2.8–2.9 notes per annotated note.
* A half-step ornament shorter than about 150 ms (a whole-step one shorter than 90 ms) stays in
  its note: at that length it is indistinguishable from a vibrato swing. Split it in the editor.
* Onsets are 50 ms-tolerance hard: COn on songs is 0.47. A learned onset model (as in
  MIR-ST500 / ROSVOT systems) would be the next step and is not part of this change.
* MIR-ST500 was not run: its audio is links to commercial recordings (mixes needing vocal
  separation).
