//! Standard MIDI files: reading the notes of a `.mid` (format 0 or 1) into parts, writing parts
//! as a format 1 file, and the General MIDI names and instrument choices an import uses.
//!
//! Pure data: no project, no engine. Times are beats (quarter notes).

use std::collections::HashMap;

use midly::num::{u15, u24, u28, u4, u7};
use midly::{Format, Header, MetaMessage, MidiMessage, Smf, Timing, TrackEvent, TrackEventKind};
use serde::Serialize;

/// The most notes one import takes (a whole orchestral score is a few tens of thousands).
pub const MAX_NOTES: usize = 200_000;
/// Ticks per beat of the files Synkopa writes: a 1/16 step is 240 ticks, a 1/64 triplet is exact.
pub const PPQ: u16 = 960;
/// The General MIDI drum channel (channel 10, counted from 0).
pub const DRUM_CHANNEL: u8 = 9;

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct SmfNote {
    pub key: u8,
    /// 1..=127.
    pub vel: u8,
    /// Beats from the start of the file.
    pub start: f64,
    /// Beats.
    pub len: f64,
}

/// The notes one MIDI channel plays in one track.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Part {
    /// Track index in the file.
    pub track: usize,
    /// MIDI channel 0..=15 (9 = drums).
    pub channel: u8,
    /// The track's name, or the instrument's when the track has none.
    pub name: String,
    /// General MIDI program of the channel's first program change.
    pub program: Option<u8>,
    pub notes: Vec<SmfNote>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MidiFile {
    pub format: u8,
    /// Ticks per beat of the file.
    pub ppq: u16,
    /// The first tempo the file sets (none = the MIDI default of 120 is implied).
    pub bpm: Option<f64>,
    /// Tempo changes after the first (Synkopa keeps one tempo: notes stay on their beats).
    pub tempo_changes: usize,
    pub parts: Vec<Part>,
    /// What the import cannot keep (time signatures other than 4/4, tempo changes …).
    pub warnings: Vec<String>,
    /// Beats, the end of the last note.
    pub end: f64,
}

/// Reads a standard MIDI file.
pub fn parse(bytes: &[u8]) -> Result<MidiFile, String> {
    let smf = Smf::parse(bytes).map_err(|e| format!("not a MIDI file: {e}"))?;
    let format = match smf.header.format {
        Format::SingleTrack => 0,
        Format::Parallel => 1,
        Format::Sequential => return Err("a format 2 MIDI file (independent sequences) is not supported".into()),
    };
    let ppq = match smf.header.timing {
        Timing::Metrical(t) => t.as_int(),
        Timing::Timecode(..) => return Err("a MIDI file timed in SMPTE frames is not supported (only beats)".into()),
    };
    if ppq == 0 {
        return Err("the MIDI file has 0 ticks per beat".into());
    }
    let beats = |tick: u64| tick as f64 / f64::from(ppq);
    let mut tempos: Vec<(u64, u32)> = vec![];
    let mut odd_meter = None;
    let mut parts: Vec<Part> = vec![];
    let mut total = 0usize;
    for (ti, track) in smf.tracks.iter().enumerate() {
        let mut tick = 0u64;
        let mut name: Option<String> = None;
        let mut instrument: Option<String> = None;
        let mut program: HashMap<u8, u8> = HashMap::new();
        // (channel, key) → note-on (tick, velocity), oldest first: a key struck again before
        // its note-off ends the oldest one first, as synthesizers do
        let mut open: HashMap<(u8, u8), Vec<(u64, u8)>> = HashMap::new();
        let mut notes: HashMap<u8, Vec<SmfNote>> = HashMap::new();
        let close = |notes: &mut HashMap<u8, Vec<SmfNote>>, ch: u8, key: u8, on: (u64, u8), off: u64| {
            notes.entry(ch).or_default().push(SmfNote { key, vel: on.1, start: beats(on.0), len: beats(off.max(on.0 + 1) - on.0) });
        };
        for ev in track {
            tick += u64::from(ev.delta.as_int());
            match ev.kind {
                TrackEventKind::Midi { channel, message } => {
                    let ch = channel.as_int();
                    match message {
                        MidiMessage::NoteOn { key, vel } if vel.as_int() > 0 => {
                            total += 1;
                            if total > MAX_NOTES {
                                return Err(format!("the MIDI file has more than {MAX_NOTES} notes"));
                            }
                            open.entry((ch, key.as_int())).or_default().push((tick, vel.as_int()));
                        }
                        MidiMessage::NoteOn { key, .. } | MidiMessage::NoteOff { key, .. } => {
                            let k = key.as_int();
                            if let Some(stack) = open.get_mut(&(ch, k)) {
                                if !stack.is_empty() {
                                    let on = stack.remove(0);
                                    close(&mut notes, ch, k, on, tick);
                                }
                            }
                        }
                        MidiMessage::ProgramChange { program: p } => {
                            program.entry(ch).or_insert(p.as_int());
                        }
                        _ => {}
                    }
                }
                TrackEventKind::Meta(m) => match m {
                    MetaMessage::Tempo(us) => tempos.push((tick, us.as_int())),
                    MetaMessage::TrackName(b) if name.is_none() => name = text(b),
                    MetaMessage::InstrumentName(b) if instrument.is_none() => instrument = text(b),
                    MetaMessage::TimeSignature(n, d, ..) if (n, d) != (4, 2) && odd_meter.is_none() => {
                        odd_meter = Some(format!("{n}/{}", 1u32 << d.min(6)));
                    }
                    _ => {}
                },
                _ => {}
            }
        }
        // notes still held at the end of the track end there
        for ((ch, key), stack) in open {
            for on in stack {
                close(&mut notes, ch, key, on, tick);
            }
        }
        let several = notes.len() > 1;
        let mut chans: Vec<u8> = notes.keys().copied().collect();
        chans.sort_unstable();
        for ch in chans {
            let mut list = notes.remove(&ch).unwrap_or_default();
            list.sort_by(|a, b| a.start.total_cmp(&b.start).then(a.key.cmp(&b.key)));
            let prog = program.get(&ch).copied();
            let fallback = if ch == DRUM_CHANNEL { "Drums".to_string() } else { prog.map(gm_name).unwrap_or("Piano").to_string() };
            // a format 0 file (or a track playing several channels) names each part by its instrument
            let name = match (&name, several) {
                (Some(n), false) => n.clone(),
                (Some(n), true) => format!("{n} · {fallback}"),
                (None, _) => instrument.clone().filter(|_| !several).unwrap_or(fallback),
            };
            parts.push(Part { track: ti, channel: ch, name, program: prog, notes: list });
        }
    }
    tempos.sort_by_key(|t| t.0);
    let mut warnings = vec![];
    // microseconds per beat are whole numbers: 97 BPM is stored as 96.99995, read back as 97
    let bpm = tempos.first().map(|t| (60_000_000_000.0 / f64::from(t.1.max(1))).round() / 1000.0);
    let tempo_changes = tempos.windows(2).filter(|w| w[0].1 != w[1].1).count();
    if tempo_changes > 0 {
        warnings.push(format!(
            "the file changes tempo {tempo_changes}×: Synkopa keeps one tempo ({:.2} BPM), the notes stay on their beats",
            bpm.unwrap_or(120.0)
        ));
    }
    if let Some(m) = odd_meter {
        warnings.push(format!("the file is in {m}: Synkopa's grid is 4/4, the notes keep their beats"));
    }
    let end = parts.iter().flat_map(|p| &p.notes).map(|n| n.start + n.len).fold(0.0, f64::max);
    Ok(MidiFile { format, ppq, bpm, tempo_changes, parts, warnings, end })
}

fn text(b: &[u8]) -> Option<String> {
    let s = String::from_utf8_lossy(b).trim().trim_matches('\0').to_string();
    (!s.is_empty()).then_some(s)
}

/// A track of a file to write.
#[derive(Clone, Debug)]
pub struct OutTrack {
    pub name: String,
    pub channel: u8,
    pub program: Option<u8>,
    pub notes: Vec<SmfNote>,
}

/// A format 1 file: a tempo track (tempo, 4/4), then one track per part.
pub fn write(bpm: f64, title: &str, tracks: &[OutTrack]) -> Result<Vec<u8>, String> {
    let mut smf = Smf::new(Header::new(Format::Parallel, Timing::Metrical(u15::new(PPQ))));
    let us = (60_000_000.0 / bpm.clamp(10.0, 999.0)).round() as u32;
    let mut head: Vec<TrackEvent> = vec![];
    let names: Vec<Vec<u8>> = std::iter::once(title).chain(tracks.iter().map(|t| t.name.as_str())).map(|s| s.as_bytes().to_vec()).collect();
    head.push(meta(0, MetaMessage::TrackName(&names[0])));
    head.push(meta(0, MetaMessage::Tempo(u24::new(us))));
    head.push(meta(0, MetaMessage::TimeSignature(4, 2, 24, 8)));
    head.push(meta(0, MetaMessage::EndOfTrack));
    smf.tracks.push(head);
    for (k, t) in tracks.iter().enumerate() {
        let ch = u4::new(t.channel.min(15));
        // (tick, order, event): note-offs sort before note-ons at the same tick, so a repeated
        // key is not cut by its own previous note
        let mut evs: Vec<(u64, u8, TrackEventKind)> = vec![];
        for n in &t.notes {
            let a = to_tick(n.start);
            let b = to_tick(n.start + n.len).max(a + 1);
            let key = u7::new(n.key.min(127));
            evs.push((a, 1, TrackEventKind::Midi { channel: ch, message: MidiMessage::NoteOn { key, vel: u7::new(n.vel.clamp(1, 127)) } }));
            evs.push((b, 0, TrackEventKind::Midi { channel: ch, message: MidiMessage::NoteOff { key, vel: u7::new(64) } }));
        }
        evs.sort_by_key(|e| (e.0, e.1));
        let mut out: Vec<TrackEvent> = vec![meta(0, MetaMessage::TrackName(&names[k + 1]))];
        if let Some(p) = t.program {
            out.push(TrackEvent {
                delta: u28::new(0),
                kind: TrackEventKind::Midi { channel: ch, message: MidiMessage::ProgramChange { program: u7::new(p.min(127)) } },
            });
        }
        let mut at = 0u64;
        for (tick, _, kind) in evs {
            let delta = u28::try_from((tick - at).min(u64::from(u28::max_value().as_int())) as u32).unwrap_or(u28::max_value());
            out.push(TrackEvent { delta, kind });
            at = tick;
        }
        out.push(meta(0, MetaMessage::EndOfTrack));
        smf.tracks.push(out);
    }
    let mut bytes = vec![];
    smf.write_std(&mut bytes).map_err(|e| format!("writing MIDI: {e}"))?;
    Ok(bytes)
}

fn to_tick(beats: f64) -> u64 {
    (beats.max(0.0) * f64::from(PPQ)).round() as u64
}

fn meta(delta: u32, m: MetaMessage<'_>) -> TrackEvent<'_> {
    TrackEvent { delta: u28::new(delta), kind: TrackEventKind::Meta(m) }
}

// ---------------------------------------------------------------- General MIDI

const GM: [&str; 128] = [
    "Acoustic Grand Piano",
    "Bright Acoustic Piano",
    "Electric Grand Piano",
    "Honky-tonk Piano",
    "Electric Piano 1",
    "Electric Piano 2",
    "Harpsichord",
    "Clavinet",
    "Celesta",
    "Glockenspiel",
    "Music Box",
    "Vibraphone",
    "Marimba",
    "Xylophone",
    "Tubular Bells",
    "Dulcimer",
    "Drawbar Organ",
    "Percussive Organ",
    "Rock Organ",
    "Church Organ",
    "Reed Organ",
    "Accordion",
    "Harmonica",
    "Tango Accordion",
    "Nylon Guitar",
    "Steel Guitar",
    "Jazz Guitar",
    "Clean Guitar",
    "Muted Guitar",
    "Overdriven Guitar",
    "Distortion Guitar",
    "Guitar Harmonics",
    "Acoustic Bass",
    "Fingered Bass",
    "Picked Bass",
    "Fretless Bass",
    "Slap Bass 1",
    "Slap Bass 2",
    "Synth Bass 1",
    "Synth Bass 2",
    "Violin",
    "Viola",
    "Cello",
    "Contrabass",
    "Tremolo Strings",
    "Pizzicato Strings",
    "Orchestral Harp",
    "Timpani",
    "String Ensemble 1",
    "String Ensemble 2",
    "Synth Strings 1",
    "Synth Strings 2",
    "Choir Aahs",
    "Voice Oohs",
    "Synth Voice",
    "Orchestra Hit",
    "Trumpet",
    "Trombone",
    "Tuba",
    "Muted Trumpet",
    "French Horn",
    "Brass Section",
    "Synth Brass 1",
    "Synth Brass 2",
    "Soprano Sax",
    "Alto Sax",
    "Tenor Sax",
    "Baritone Sax",
    "Oboe",
    "English Horn",
    "Bassoon",
    "Clarinet",
    "Piccolo",
    "Flute",
    "Recorder",
    "Pan Flute",
    "Blown Bottle",
    "Shakuhachi",
    "Whistle",
    "Ocarina",
    "Square Lead",
    "Saw Lead",
    "Calliope Lead",
    "Chiff Lead",
    "Charang Lead",
    "Voice Lead",
    "Fifths Lead",
    "Bass + Lead",
    "New Age Pad",
    "Warm Pad",
    "Polysynth Pad",
    "Choir Pad",
    "Bowed Pad",
    "Metallic Pad",
    "Halo Pad",
    "Sweep Pad",
    "Rain FX",
    "Soundtrack FX",
    "Crystal FX",
    "Atmosphere FX",
    "Brightness FX",
    "Goblins FX",
    "Echoes FX",
    "Sci-fi FX",
    "Sitar",
    "Banjo",
    "Shamisen",
    "Koto",
    "Kalimba",
    "Bagpipe",
    "Fiddle",
    "Shanai",
    "Tinkle Bell",
    "Agogo",
    "Steel Drums",
    "Woodblock",
    "Taiko Drum",
    "Melodic Tom",
    "Synth Drum",
    "Reverse Cymbal",
    "Guitar Fret Noise",
    "Breath Noise",
    "Seashore",
    "Bird Tweet",
    "Telephone Ring",
    "Helicopter",
    "Applause",
    "Gunshot",
];

/// The General MIDI name of a program.
pub fn gm_name(program: u8) -> &'static str {
    GM[usize::from(program.min(127))]
}

/// What an imported part plays in Synkopa.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Instrument {
    /// Synkopa Keys, the sampled grand piano.
    Keys,
    /// A factory synth preset.
    Synth { preset: &'static str },
}

/// The Synkopa instrument closest to a General MIDI program (none = a piano part).
/// `keys`: Synkopa Keys is on this computer (otherwise pianos get a synth piano).
pub fn instrument_for(program: Option<u8>, keys: bool) -> Instrument {
    let p = program.unwrap_or(0);
    let synth = |preset| Instrument::Synth { preset };
    match p {
        0..=3 if keys => Instrument::Keys,
        0..=3 => synth("Klawisz Miękki"),
        4 | 5 => synth("EP Klasyczny"),
        6 | 7 => synth("Klawisz Miękki"),
        11 => synth("Vibrafon"),
        12 | 13 => synth("Marimba"),
        8..=15 => synth("Dzwonek Ciepły"),
        16..=23 => synth("Organy"),
        24..=31 => synth("Pluck Ciepły"),
        38 | 39 => synth("Bas Pluck"),
        32..=37 => synth("Bas Analog"),
        46 => synth("Pluck Szklany"),
        40..=51 => synth("Smyczki"),
        52..=54 => synth("Pad Powietrzny"),
        55..=71 => synth("Brass"),
        72..=79 => synth("Lead Gwizdek"),
        81 => synth("Supersaw Lead"),
        80..=87 => synth("Lead Ostry"),
        88..=95 => synth("Pad Ciepły"),
        96..=103 => synth("Pad Ruch"),
        104..=111 => synth("Pluck Ciepły"),
        112..=119 => synth("Dzwonek Ciepły"),
        _ => synth("Szum Wiatru"),
    }
}

/// The program a Synkopa synth preset category stands for on export (the inverse of
/// [`instrument_for`], one program per category).
pub fn program_for_category(category: &str) -> u8 {
    match category {
        "808" | "Bas" => 38,
        "Pluck" => 25,
        "Lead" => 81,
        "Pad" => 89,
        "Klawisze" => 4,
        "Dzwonki" => 11,
        "Dęte/Smyczki" => 48,
        "Arp" => 81,
        "Lo-fi" => 4,
        "FX" => 99,
        _ => 0,
    }
}

/// The standard-library sample that plays a General MIDI drum key, and its GM name.
pub fn drum_for(key: u8) -> (&'static str, &'static str) {
    match key {
        35 => ("kick-studio.flac", "Acoustic Bass Drum"),
        36 => ("kick-808.flac", "Bass Drum"),
        37 => ("sidestick-studio.flac", "Side Stick"),
        38 => ("snare-studio.flac", "Snare"),
        39 => ("clap-808.flac", "Hand Clap"),
        40 => ("snare-808-snappy.flac", "Electric Snare"),
        41 => ("tom-studio-floor.flac", "Low Floor Tom"),
        42 => ("hat-808-closed.flac", "Closed Hi-hat"),
        43 => ("tom-808-low.flac", "High Floor Tom"),
        44 => ("hat-808-pedal.flac", "Pedal Hi-hat"),
        45 => ("tom-studio-low.flac", "Low Tom"),
        46 => ("hat-808-open.flac", "Open Hi-hat"),
        47 => ("tom-808-mid.flac", "Low-Mid Tom"),
        48 => ("tom-808-high.flac", "Hi-Mid Tom"),
        49 | 57 => ("crash-studio.flac", "Crash Cymbal"),
        50 => ("tom-studio-high.flac", "High Tom"),
        51 | 59 => ("ride-studio.flac", "Ride Cymbal"),
        52 => ("cymbal-808-bright.flac", "Chinese Cymbal"),
        53 => ("ride-bell-studio.flac", "Ride Bell"),
        54 => ("tambourine.flac", "Tambourine"),
        55 => ("cymbal-808-short.flac", "Splash Cymbal"),
        56 => ("cowbell-808.flac", "Cowbell"),
        58 => ("cabasa.flac", "Vibraslap"),
        60 => ("bongo-high.flac", "Hi Bongo"),
        61 => ("bongo-low.flac", "Low Bongo"),
        62 => ("conga-808-high.flac", "Mute Hi Conga"),
        63 => ("conga-808-mid.flac", "Open Hi Conga"),
        64 => ("conga-808-low.flac", "Low Conga"),
        69 => ("cabasa.flac", "Cabasa"),
        70 => ("maracas-808.flac", "Maracas"),
        75 => ("claves-808.flac", "Claves"),
        76 | 77 => ("woodblock.flac", "Wood Block"),
        80 | 81 => ("triangle.flac", "Triangle"),
        82 => ("shaker.flac", "Shaker"),
        k if k < 35 => ("kick-808-click.flac", "Percussion"),
        _ => ("conga.flac", "Percussion"),
    }
}

/// The General MIDI drum key a standard-library drum sample stands for (its group decides).
pub fn drum_key_for(file: &str) -> u8 {
    if let Some(k) = (27..=87).find(|k| drum_for(*k).0 == file) {
        return k;
    }
    let f = file.to_ascii_lowercase();
    let has = |s: &str| f.contains(s);
    if has("kick") {
        36
    } else if has("rim") || has("stick") {
        37
    } else if has("clap") {
        39
    } else if has("snare") || has("cajon") {
        38
    } else if has("hat") && has("open") {
        46
    } else if has("hat") && has("pedal") {
        44
    } else if has("hat") {
        42
    } else if has("ride") {
        51
    } else if has("crash") || has("cymbal") {
        49
    } else if has("tom") {
        45
    } else if has("cowbell") {
        56
    } else if has("conga") {
        63
    } else if has("bongo") {
        60
    } else if has("shaker") || has("maracas") {
        70
    } else if has("tambourine") {
        54
    } else if has("clave") {
        75
    } else if has("woodblock") {
        76
    } else if has("triangle") {
        81
    } else {
        39
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn n(key: u8, start: f64, len: f64, vel: u8) -> SmfNote {
        SmfNote { key, vel, start, len }
    }

    #[test]
    fn written_files_read_back_note_for_note_with_tempo_names_and_programs() {
        let tracks = vec![
            OutTrack {
                name: "Bass".into(),
                channel: 0,
                program: Some(38),
                notes: vec![n(36, 0.0, 1.0, 100), n(36, 1.0, 0.25, 90), n(43, 2.5, 1.5, 127)],
            },
            OutTrack {
                name: "Drums".into(),
                channel: DRUM_CHANNEL,
                program: None,
                notes: vec![n(36, 0.0, 0.25, 110), n(42, 0.5, 0.25, 70)],
            },
        ];
        let bytes = write(93.5, "Song", &tracks).unwrap();
        let f = parse(&bytes).unwrap();
        assert_eq!(f.format, 1);
        assert!((f.bpm.unwrap() - 93.5).abs() < 0.01);
        assert_eq!(f.tempo_changes, 0);
        assert!(f.warnings.is_empty());
        assert_eq!(f.parts.len(), 2);
        assert_eq!(f.parts[0].name, "Bass");
        assert_eq!(f.parts[0].program, Some(38));
        assert_eq!(f.parts[0].notes, tracks[0].notes);
        assert_eq!(f.parts[1].channel, DRUM_CHANNEL);
        assert_eq!(f.parts[1].notes, tracks[1].notes);
        assert_eq!(f.end, 4.0);
    }

    #[test]
    fn repeated_keys_overlaps_zero_velocity_offs_and_unended_notes() {
        let ch = u4::new(0);
        let on = |k: u8, v: u8| TrackEventKind::Midi { channel: ch, message: MidiMessage::NoteOn { key: u7::new(k), vel: u7::new(v) } };
        let ev = |d: u32, kind| TrackEvent { delta: u28::new(d), kind };
        let mut smf = Smf::new(Header::new(Format::SingleTrack, Timing::Metrical(u15::new(480))));
        smf.tracks.push(vec![
            ev(0, on(60, 100)),
            ev(240, on(60, 80)), // the same key again before its note-off
            ev(240, on(60, 0)),  // ends the first (zero velocity = off)
            ev(240, on(60, 0)),  // ends the second
            ev(0, on(64, 50)),   // never ended: lasts to the end of the track
            ev(480, TrackEventKind::Meta(MetaMessage::EndOfTrack)),
        ]);
        let mut b = vec![];
        smf.write_std(&mut b).unwrap();
        let f = parse(&b).unwrap();
        assert_eq!(f.bpm, None);
        assert_eq!(f.parts[0].notes, vec![n(60, 0.0, 1.0, 100), n(60, 0.5, 1.0, 80), n(64, 1.5, 1.0, 50)]);
    }

    #[test]
    fn a_format_0_file_splits_by_channel_and_reports_tempo_and_meter() {
        let ev = |d: u32, kind| TrackEvent { delta: u28::new(d), kind };
        let note = |c: u8, k: u8, v: u8| TrackEventKind::Midi {
            channel: u4::new(c),
            message: MidiMessage::NoteOn { key: u7::new(k), vel: u7::new(v) },
        };
        let mut smf = Smf::new(Header::new(Format::SingleTrack, Timing::Metrical(u15::new(96))));
        smf.tracks.push(vec![
            ev(0, TrackEventKind::Meta(MetaMessage::Tempo(u24::new(500_000)))),
            ev(0, TrackEventKind::Meta(MetaMessage::TimeSignature(3, 2, 24, 8))),
            ev(0, TrackEventKind::Midi { channel: u4::new(1), message: MidiMessage::ProgramChange { program: u7::new(33) } }),
            ev(0, note(1, 40, 90)),
            ev(0, note(9, 36, 90)),
            ev(96, note(1, 40, 0)),
            ev(0, note(9, 36, 0)),
            ev(0, TrackEventKind::Meta(MetaMessage::Tempo(u24::new(400_000)))),
        ]);
        let mut b = vec![];
        smf.write_std(&mut b).unwrap();
        let f = parse(&b).unwrap();
        assert_eq!(f.format, 0);
        assert_eq!(f.bpm, Some(120.0));
        assert_eq!(f.tempo_changes, 1);
        assert_eq!(f.warnings.len(), 2, "{:?}", f.warnings);
        assert_eq!(f.parts.iter().map(|p| (p.channel, p.name.as_str())).collect::<Vec<_>>(), vec![(1, "Fingered Bass"), (9, "Drums")]);
    }

    #[test]
    fn unsupported_files_are_errors() {
        assert!(parse(b"not midi").is_err());
        let mut smf = Smf::new(Header::new(Format::Sequential, Timing::Metrical(u15::new(96))));
        smf.tracks.push(vec![]);
        let mut b = vec![];
        smf.write_std(&mut b).unwrap();
        assert!(parse(&b).unwrap_err().contains("format 2"));
    }

    #[test]
    fn instruments_and_drums_map_both_ways() {
        assert_eq!(instrument_for(None, true), Instrument::Keys);
        assert_eq!(instrument_for(Some(0), false), Instrument::Synth { preset: "Klawisz Miękki" });
        assert_eq!(instrument_for(Some(33), true), Instrument::Synth { preset: "Bas Analog" });
        let presets: Vec<&str> = crate::poly::presets::all().iter().map(|p| p.name).collect();
        for p in 0..=127u8 {
            if let Instrument::Synth { preset } = instrument_for(Some(p), false) {
                assert!(presets.contains(&preset), "{p}: no preset {preset}");
            }
        }
        let files: Vec<&str> = crate::factory::files().iter().map(|f| f.file).collect();
        for k in 0..=127u8 {
            let (f, _) = drum_for(k);
            assert!(files.contains(&f), "{k}: no sample {f}");
            if (35..=56).contains(&k) && ![57, 59].contains(&k) {
                assert_eq!(drum_key_for(f), k, "{k} {f}");
            }
        }
        assert_eq!(drum_key_for("kick-808-boom.flac"), 36);
        assert_eq!(drum_key_for("hat-studio-open.flac"), 46);
    }
}
