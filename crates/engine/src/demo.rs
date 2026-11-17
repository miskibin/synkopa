//! The projects the program ships with: the empty project it starts on (eight tracks, the
//! starter kit in the Channel Rack), the finished 2026 trap demo project (`demo_project`), and
//! the small fixed song tests and benchmarks build on (`sketch_project`).

use crate::catalog::{load_preset, new_fx};
use crate::model::*;

const TC: [&str; 10] = ["#4f8ef7", "#5aa9e6", "#69c3d6", "#47b8a8", "#5fbf8a", "#7c8cf8", "#e0a44a", "#9a8cf0", "#d9678f", "#8b93a1"];
const COLORS: [usize; 8] = [0, 6, 5, 3, 4, 8, 9, 9];

fn base(names: &[&str], ins_names: &[&str]) -> Project {
    let mut p = Project {
        version: 2,
        bpm: 128.0,
        tracks: vec![],
        inserts: vec![Insert {
            id: 0,
            name: "Master".into(),
            color: "#d5dae2".into(),
            vol: VOL_UNITY,
            pan: 0.0,
            width: 1.0,
            muted: false,
            solo: false,
            fx: vec![],
            sends: vec![],
            master: true,
        }],
        patterns: vec![],
        sources: vec![],
        clips: vec![],
        loop_range: None,
        automations: vec![],
        comps: vec![],
        channels: vec![],
        notebook: Notebook::default(),
        key: None,
        scale: 1,
        next_id: 1000,
    };
    for (i, name) in ins_names.iter().enumerate() {
        p.inserts.push(Insert {
            id: i as Id + 1,
            name: (*name).into(),
            color: TC[COLORS[i]].into(),
            vol: VOL_UNITY,
            pan: 0.0,
            width: 1.0,
            muted: false,
            solo: false,
            fx: vec![],
            sends: vec![],
            master: true,
        });
    }
    for (i, name) in names.iter().enumerate() {
        p.tracks.push(Track {
            id: i as Id + 1,
            name: (*name).into(),
            color: TC[COLORS[i]].into(),
            muted: false,
            solo: false,
            arm: false,
            vol: VOL_UNITY,
            h: 56.0,
            insert: i as Id + 1,
            icon: None,
        });
    }
    let limiter = new_fx(900, FxKind::Limit, p.bpm);
    p.inserts[0].fx.push(limiter);
    p
}

/// The empty project with track n sent to insert n (a multitrack session): what tests and
/// benchmarks that put effects on single tracks build on.
pub fn routed_project() -> Project {
    let mut p = base(
        &["Track 1", "Track 2", "Track 3", "Track 4", "Track 5", "Track 6", "Track 7", "Track 8"],
        &["Insert 1", "Insert 2", "Insert 3", "Insert 4", "Insert 5", "Insert 6", "Insert 7", "Insert 8"],
    );
    p.add_starter_kit();
    // one empty pattern, as in FL Studio: the Channel Rack and the Piano roll have something to edit
    p.patterns.push(Pattern { id: 1, name: "Pattern 1".into(), color: TC[0].into(), len: 4.0, min_len: 4.0, notes: vec![], swing: 0.0 });
    p
}

/// What the program starts with. Playlist tracks are plain lanes, as in FL Studio: none is tied
/// to a mixer insert (they play into the master); routing belongs to the channels, and a track
/// is sent to an insert by hand.
pub fn empty_project() -> Project {
    let mut p = routed_project();
    for t in &mut p.tracks {
        t.insert = 0;
    }
    p
}

/// A sketch of a song: three short patterns (beat, hats, bass line) on the starter kit plus
/// a gated 808 bass, with a vocal chain and a drum-bus compressor. Small and fixed: tests,
/// benchmarks and examples build on it.
pub fn sketch_project() -> Project {
    let mut p = base(
        &["Drums", "Hats", "Bass", "Pad", "Perc", "Vocal", "Track 7", "Track 8"],
        &["Drums", "Hats", "Bass", "Pad", "Perc", "Vocal", "Insert 7", "Insert 8"],
    );
    p.tracks[5].arm = true;
    let [kick, clap, hat, _snare] = p.add_starter_kit();
    let bass = p.factory_channel("bass-808.flac").expect("a standard-library file");
    for c in p.channels.iter_mut().filter(|c| c.id == bass) {
        c.name = "Bass".into();
        if let ChannelSrc::Sample { sampler, .. } = &mut c.src {
            sampler.gate = true;
        }
    }
    let n = |c: Id, step: f32, len: f32, key: Option<u8>| Note::new(c, step, len, key);
    p.patterns = vec![
        Pattern {
            id: 1,
            name: "Beat".into(),
            color: "#4f8ef7".into(),
            len: 4.0,
            min_len: 4.0,
            notes: [(0., kick), (4., kick), (8., kick), (12., kick), (10., kick), (4., clap), (12., clap)]
                .iter()
                .map(|&(s, i)| Note::trigger(i, s, None))
                .collect(),
            swing: 0.0,
        },
        Pattern {
            id: 2,
            name: "Hats".into(),
            color: "#e0a44a".into(),
            len: 4.0,
            min_len: 4.0,
            notes: [2., 6., 10., 14., 15., 7.].iter().map(|&s| Note::trigger(hat, s, None)).collect(),
            swing: 0.0,
        },
        Pattern {
            id: 3,
            name: "Bass".into(),
            color: "#7c8cf8".into(),
            len: 8.0,
            min_len: 4.0,
            notes: [
                (0., 3., 33),
                (4., 2., 33),
                (6., 2., 40),
                (8., 4., 36),
                (14., 2., 38),
                (16., 3., 29),
                (20., 2., 29),
                (22., 2., 36),
                (24., 6., 31),
                (30., 2., 34),
            ]
            .iter()
            .map(|&(s, l, k)| n(bass, s, l, Some(k)))
            .collect(),
            swing: 0.0,
        },
    ];
    let clip = |id, track, start, len, pat| Clip {
        id,
        track,
        start,
        len,
        offset: 0.0,
        kind: ClipKind::Pat,
        source: pat,
        muted: false,
        gain: 0.0,
        fade_in: 0.0,
        fade_out: 0.0,
        pan: 0.0,
        norm: false,
        rev: false,
        tune: None,
        stretch: 1.0,
        resample: false,
        pitch: 0.0,
        insert: None,
    };
    p.clips = vec![clip(1, 0, 0.0, 16.0, 1), clip(2, 0, 16.0, 16.0, 1), clip(3, 1, 8.0, 24.0, 2), clip(4, 2, 0.0, 32.0, 3)];
    // Vocal chain: EQ (custom) → comp → reverb, drum bus comp.
    let mut eq = new_fx(902, FxKind::Eq, p.bpm);
    load_preset(&mut eq, 1, p.bpm);
    p.inserts[6].fx = vec![eq, new_fx(903, FxKind::Comp, p.bpm), new_fx(904, FxKind::Reverb, p.bpm)];
    let mut comp = new_fx(905, FxKind::Comp, p.bpm);
    load_preset(&mut comp, 2, p.bpm);
    p.inserts[1].fx = vec![comp];
    p
}

/// Beats per minute of the 2026 trap demo ("OBSIDIAN 2093").
pub const DEMO_BPM: f64 = 136.0;

/// The demo project: 40 bars (160 beats) of 2026 dystopian / nocturnal trap in D# minor at 136 BPM.
///
/// Built from embedded standard-library samples and built-in instruments/DSP so it opens and
/// plays identically on every machine without external downloads:
/// * Clean, un-distorted analog punch kick (`808 Kick Short`) tucked smoothly into a warm
///   `808 Spin` bassline — anchored on the `D#1` root pedal through the intro, verse, and
///   opening drops (with brief 1.5-step high `D#4` turnaround flicks), then blooming into
///   melodic `D#m -> A#m -> B -> G#m` movement on the second half of the hooks and climax;
/// * Full-beat organic transitions (`Beat Bus`): `Synths Bus`, `808`, and `Drum Bus` sum
///   through an automated `Half-Speed` + `Hall Reverb` + `24 dB/oct Low-Pass Filter` chain —
///   sweeping open from a submerged half-time intro, dunking smoothly at section boundaries,
///   building through a mid-song half-time wash into a 380 ms pre-drop silence cut, and
///   melting back into dark half-time on the outro;
/// * Two-act palette: Act I pairs `Vox Dark` and `Pad Cinematic` over swung BNYX/Metro drums,
///   switching in Act II to mid-scooped `Synth Crushed`, `Synth Cyber`, and wide unison
///   `2093 Wide Pulse`, plus an armed `Vocal` strip pre-set to D# minor.
pub fn demo_project() -> Project {
    const TRACKS: [(&str, &str, &str, &str); 14] = [
        ("Dark Vox", "#9a8cf0", "vocal", "Vox"),
        ("Analog Pad", "#47b8a8", "synth", "Pad"),
        ("Crushed 2093", "#69c3d6", "sample", "Industrial"),
        ("Cyber & Pulse", "#5aa9e6", "synth", "Lead"),
        ("808", "#7c8cf8", "bass", "808"),
        ("Kick", "#4f8ef7", "drums", "Kick"),
        ("Clap & Snare", "#d9678f", "drums", "Tops"),
        ("Hats", "#e0a44a", "drums", "Tops"),
        ("Beat Low-Pass", "#69c3d6", "bus", "Beat Bus"),
        ("Beat Half-Time", "#b07cf0", "bus", "Beat Bus"),
        ("Beat Reverb", "#47b8a8", "bus", "Beat Bus"),
        ("Sidechain Pump", "#e0a44a", "bus", "Synths Bus"),
        ("Switch & Fade", "#d9678f", "bus", "Beat Bus"),
        ("Vocal", "#5fbf8a", "vocal", "Vocal"),
    ];
    const INSERTS: [(&str, &str); 11] = [
        ("Vox", "#9a8cf0"),
        ("Pad", "#47b8a8"),
        ("Industrial", "#69c3d6"),
        ("Lead", "#5aa9e6"),
        ("Synths Bus", "#b07cf0"),
        ("808", "#7c8cf8"),
        ("Kick", "#4f8ef7"),
        ("Tops", "#e0a44a"),
        ("Drum Bus", "#d9678f"),
        ("Beat Bus", "#69c3d6"),
        ("Vocal", "#5fbf8a"),
    ];

    let mut p = base(&[], &[]);
    p.bpm = DEMO_BPM;
    p.key = Some(3); // D# minor
    p.scale = 2;

    for (i, &(name, color)) in INSERTS.iter().enumerate() {
        p.inserts.push(Insert {
            id: i as Id + 1,
            name: name.into(),
            color: color.into(),
            vol: VOL_UNITY,
            pan: 0.0,
            width: 1.0,
            muted: false,
            solo: false,
            fx: vec![],
            sends: vec![],
            master: true,
        });
    }
    let ins_id = |name: &str| {
        if name == "Master" {
            0
        } else {
            INSERTS.iter().position(|&(n, _)| n == name).map(|i| i as Id + 1).expect("demo insert")
        }
    };

    for (i, &(name, color, icon, route)) in TRACKS.iter().enumerate() {
        p.tracks.push(Track {
            id: i as Id + 1,
            name: name.into(),
            color: color.into(),
            muted: false,
            solo: false,
            arm: name == "Vocal",
            vol: VOL_UNITY,
            h: 50.0,
            insert: ins_id(route),
            icon: Some(icon.into()),
        });
    }
    let trk_idx = |name: &str| TRACKS.iter().position(|&(n, _, _, _)| n == name).expect("demo track") as u32;

    // ---- Channel Rack
    let add_sample = |p: &mut Project, file: &str, name: &str, ins: &str, vol: f32, pan: f32, edit: &dyn Fn(&mut SamplerState)| -> Id {
        let id = p.factory_channel(file).expect("standard-library sample");
        let c = p.channels.iter_mut().find(|c| c.id == id).expect("channel just added");
        c.name = name.into();
        c.insert = Some(ins_id(ins));
        c.vol = vol;
        c.pan = pan;
        if let ChannelSrc::Sample { sampler, .. } = &mut c.src {
            edit(sampler);
        }
        id
    };

    // Clean, un-distorted analog punch kick (trimmed short so sub clears cleanly into the 808)
    let ch_kick = add_sample(&mut p, "kick-808-short.flac", "Kick", "Kick", 0.74, 0.0, &|s| {
        s.end = 0.58;
        s.fade_out_ms = 28.0;
    });
    let ch_clap = add_sample(&mut p, "clap-trap-crisp.flac", "Clap", "Tops", 0.75, 0.0, &|_| {});
    let ch_snare = add_sample(&mut p, "snare-trap.flac", "Snare", "Tops", 0.72, 0.0, &|_| {});
    let ch_bounce = add_sample(&mut p, "snare-trap-crisp.flac", "Bounce Snare", "Tops", 0.64, -0.16, &|_| {});
    let ch_rim = add_sample(&mut p, "rimshot-tight.flac", "Rim", "Tops", 0.66, 0.18, &|s| {
        s.end = 0.24;
        s.fade_out_ms = 22.0;
    });
    let ch_hat = add_sample(&mut p, "hat-trap-tick.flac", "Hat", "Tops", 0.65, 0.10, &|s| {
        s.cut = 1;
    });
    let ch_ohat = add_sample(&mut p, "hat-open-choked.flac", "Open Hat", "Tops", 0.62, -0.18, &|s| {
        s.cut = 1;
    });
    let ch_808 = add_sample(&mut p, "bass-808-spin.flac", "808 Spin", "808", 0.78, 0.0, &|s| {
        s.attack = 12.0;
        s.gate = true;
        s.release = 75.0;
        s.cut = 2;
    });

    let add_synth = |p: &mut Project, preset: &str, name: &str, color: &str, ins: &str, vol: f32, kv: &[(&str, f64)]| -> Id {
        let id = p.alloc_id();
        let mut state = crate::poly::preset_state(preset).expect("factory synth preset");
        for &(k, v) in kv {
            state.params.insert(k.into(), v);
        }
        let mut ch = Channel::new(id, name, color, ChannelSrc::Synth { synth: state });
        ch.insert = Some(ins_id(ins));
        ch.vol = vol;
        p.channels.push(ch);
        id
    };

    let ch_reese = add_synth(
        &mut p,
        "Reese Szeroki",
        "Obsidian Sub",
        "#7c8cf8",
        "Pad",
        0.66,
        &[("flt_cutoff", 175.0), ("flt_res", 10.0), ("amp_a", 65.0), ("amp_r", 340.0), ("v_glide", 80.0), ("fx_out", -15.5)],
    );
    let ch_pulse = add_synth(
        &mut p,
        "Future Chord",
        "2093 Wide Pulse",
        "#5aa9e6",
        "Lead",
        0.69,
        &[
            ("uni_voices", 3.0),
            ("uni_detune", 34.0),
            ("uni_width", 100.0),
            ("flt_cutoff", 1450.0),
            ("flt_res", 10.0),
            ("flt_drive", 26.0),
            ("flt_env", 2.0),
            ("mod_d", 220.0),
            ("amp_a", 3.0),
            ("amp_d", 380.0),
            ("amp_s", 25.0),
            ("amp_r", 200.0),
            ("v_mode", 2.0),
            ("v_glide", 45.0),
            ("fx_chorus", 35.0),
            ("fx_delay", 24.0),
            ("fx_dtime", 3.0),
            ("fx_reverb", 22.0),
            ("fx_out", -6.5),
        ],
    );

    // ---- Patterns
    let held = |ch: Id, step: f32, len: f32, key: u8, v: f32| Note { ch, step, len, p: Some(key), v, trig: Some(false) };
    let trig = |ch: Id, step: f32, key: Option<u8>, v: f32| Note { ch, step, len: 1.0, p: key, v, trig: Some(true) };

    let mut pats: Vec<Pattern> = vec![];
    let mut make_pat = |name: &str, color: &str, bars: f64, swing: f32, notes: Vec<Note>| -> Id {
        let id = pats.len() as Id + 1;
        let mut pt = Pattern { id, name: name.into(), color: color.into(), len: bars * 4.0, min_len: bars * 4.0, notes, swing };
        pt.fit_len();
        pats.push(pt);
        id
    };

    // Pattern 1: Obsidian Sub — steady D#1 (MIDI 27) drone for Intro & Bridge
    let p_reese = make_pat("Obsidian Sub", "#47b8a8", 4.0, 0.0, vec![held(ch_reese, 0.0, 63.0, 27, 0.80)]);

    // Pattern 2: 808 Root — calm D#1 (MIDI 27) root pedal (Verse B)
    let p_808_root = make_pat(
        "808 Root",
        "#7c8cf8",
        4.0,
        0.0,
        vec![
            held(ch_808, 0.0, 9.5, 27, 0.94),
            held(ch_808, 10.0, 5.5, 27, 0.86),
            held(ch_808, 16.0, 11.5, 27, 0.92),
            held(ch_808, 28.0, 3.5, 27, 0.80),
            held(ch_808, 32.0, 9.5, 27, 0.94),
            held(ch_808, 42.0, 5.5, 27, 0.86),
            held(ch_808, 48.0, 14.5, 27, 0.92),
        ],
    );

    // Pattern 3: 808 High Flick — D#1 root pedal + brief 1.5-step high D#4 (MIDI 63) turnaround pop (Hook A & Act II A)
    let p_808_turn = make_pat(
        "808 High Flick",
        "#7c8cf8",
        4.0,
        0.0,
        vec![
            held(ch_808, 0.0, 9.5, 27, 0.94),
            held(ch_808, 10.0, 5.5, 27, 0.86),
            held(ch_808, 16.0, 11.5, 27, 0.92),
            held(ch_808, 28.0, 3.5, 27, 0.80),
            held(ch_808, 32.0, 9.5, 27, 0.94),
            held(ch_808, 42.0, 5.5, 27, 0.86),
            held(ch_808, 48.0, 13.5, 27, 0.92),
            held(ch_808, 62.5, 1.5, 63, 0.70),
        ],
    );

    // Pattern 4: 808 Melodic — opens up harmonically in Hook B (bars 9-12): D#1->F#1 | A#1->F1 | B1->G#1 | D#1->A#1
    let p_808_melodic = make_pat(
        "808 Melodic",
        "#9a8cf0",
        4.0,
        0.0,
        vec![
            held(ch_808, 0.0, 9.5, 27, 0.94),   // D#1
            held(ch_808, 10.0, 5.5, 30, 0.86),  // F#1 (m3 lift)
            held(ch_808, 16.0, 11.5, 34, 0.90), // A#1 (v)
            held(ch_808, 28.0, 3.5, 29, 0.82),  // F1
            held(ch_808, 32.0, 9.5, 35, 0.92),  // B1 (bVI cinematic lift)
            held(ch_808, 42.0, 5.5, 32, 0.86),  // G#1 (iv)
            held(ch_808, 48.0, 9.5, 27, 0.92),  // D#1
            held(ch_808, 58.0, 5.5, 34, 0.84),  // A#1
        ],
    );

    // Pattern 5: 808 Phrygian — subtle dark Phrygian bII movement in Act II B (bars 29-32)
    let p_808_phrygian = make_pat(
        "808 Phrygian",
        "#69c3d6",
        4.0,
        0.0,
        vec![
            held(ch_808, 0.0, 9.5, 27, 0.94),   // D#1
            held(ch_808, 10.0, 5.5, 27, 0.86),  // D#1
            held(ch_808, 16.0, 11.5, 27, 0.92), // D#1
            held(ch_808, 28.0, 3.5, 30, 0.84),  // F#1
            held(ch_808, 32.0, 9.5, 28, 0.92),  // E1 (Phrygian bII tension)
            held(ch_808, 42.0, 5.5, 32, 0.86),  // G#1
            held(ch_808, 48.0, 14.5, 27, 0.94), // D#1
        ],
    );

    // Pattern 6: 808 Climax — full melodic progression crowned with the crisp dry D#4 high flick into the Outro
    let p_808_climax = make_pat(
        "808 Climax",
        "#9a8cf0",
        4.0,
        0.0,
        vec![
            held(ch_808, 0.0, 9.5, 27, 0.95),   // D#1
            held(ch_808, 10.0, 5.5, 30, 0.86),  // F#1
            held(ch_808, 16.0, 11.5, 34, 0.92), // A#1
            held(ch_808, 28.0, 3.5, 29, 0.82),  // F1
            held(ch_808, 32.0, 9.5, 35, 0.94),  // B1
            held(ch_808, 42.0, 5.5, 32, 0.86),  // G#1
            held(ch_808, 48.0, 9.5, 27, 0.94),  // D#1
            held(ch_808, 58.0, 4.0, 34, 0.84),  // A#1
            held(ch_808, 62.5, 1.5, 63, 0.70),  // High D#4 flick!
        ],
    );

    // Pattern 7: 808 Verse — ultra-minimal 1-hit-per-bar D#1 sub pillow (Verse A & Outro)
    let p_808_verse = make_pat(
        "808 Verse",
        "#7c8cf8",
        4.0,
        0.0,
        vec![
            held(ch_808, 0.0, 12.0, 27, 0.90),
            held(ch_808, 16.0, 12.0, 27, 0.88),
            held(ch_808, 32.0, 12.0, 27, 0.90),
            held(ch_808, 48.0, 12.0, 27, 0.88),
        ],
    );

    // Pattern 8: Kick — clean, gentle chest thump synced to the 808 attacks
    let p_kick = make_pat(
        "Kick",
        "#4f8ef7",
        2.0,
        0.0,
        [(0.0, 0.92), (10.0, 0.82), (16.0, 0.90), (28.0, 0.78)].into_iter().map(|(s, v)| trig(ch_kick, s, None, v)).collect(),
    );

    // Pattern 9: Clap & Bounce (14% BNYX pocket swing)
    let p_clap = make_pat("Clap & Bounce", "#d9678f", 2.0, 0.14, {
        let mut v = vec![
            trig(ch_clap, 8.0, None, 0.94),
            trig(ch_clap, 24.0, None, 0.96),
            trig(ch_snare, 8.0, None, 0.78),
            trig(ch_snare, 24.0, None, 0.82),
            trig(ch_bounce, 11.0, None, 0.54),
            trig(ch_bounce, 27.0, None, 0.60),
            trig(ch_bounce, 29.5, None, 0.44),
            trig(ch_rim, 3.0, None, 0.58),
            trig(ch_rim, 14.0, None, 0.68),
            trig(ch_rim, 19.0, None, 0.54),
            trig(ch_rim, 30.0, None, 0.70),
        ];
        v.sort_by(|a, b| a.step.total_cmp(&b.step));
        v
    });

    // Pattern 10: Verse Strip (clap + rim pocket)
    let p_clap_sparse = make_pat(
        "Verse Strip",
        "#d9678f",
        2.0,
        0.16,
        vec![
            trig(ch_clap, 8.0, None, 0.90),
            trig(ch_rim, 14.0, None, 0.62),
            trig(ch_clap, 24.0, None, 0.92),
            trig(ch_rim, 27.0, None, 0.54),
            trig(ch_bounce, 30.0, None, 0.58),
        ],
    );

    // Pattern 11: Hats Nocturne (18% swing + deep ghosting)
    let p_hats_sparse = make_pat(
        "Hats Nocturne",
        "#e0a44a",
        2.0,
        0.18,
        [
            (0.0, 0.70),
            (2.0, 0.36),
            (4.0, 0.62),
            (6.0, 0.40),
            (10.0, 0.52),
            (12.0, 0.66),
            (14.0, 0.36),
            (15.0, 0.28),
            (16.0, 0.72),
            (18.0, 0.38),
            (20.0, 0.60),
            (22.0, 0.42),
            (26.0, 0.50),
            (28.0, 0.64),
            (30.0, 0.38),
        ]
        .into_iter()
        .map(|(s, v)| trig(ch_hat, s, None, v))
        .collect(),
    );

    // Pattern 12: Hats 2093 (18% BNYX micro-swing + wide velocity ghosting + pitched rolls)
    let p_hats_2093 = make_pat("Hats 2093", "#e0a44a", 2.0, 0.18, {
        let mut v = vec![
            trig(ch_hat, 0.0, Some(60), 0.84),
            trig(ch_hat, 1.0, Some(60), 0.28),
            trig(ch_ohat, 2.0, None, 0.74),
            trig(ch_hat, 4.0, Some(60), 0.76),
            trig(ch_hat, 6.0, Some(61), 0.28),
            trig(ch_hat, 6.5, Some(60), 0.42),
            trig(ch_hat, 7.0, Some(59), 0.58),
            trig(ch_hat, 7.5, Some(58), 0.74),
            trig(ch_hat, 8.0, Some(60), 0.82),
            trig(ch_hat, 10.0, Some(60), 0.48),
            trig(ch_hat, 11.0, Some(59), 0.26),
            trig(ch_hat, 12.0, Some(60), 0.74),
            trig(ch_hat, 13.0, Some(58), 0.32),
            trig(ch_hat, 14.0, Some(60), 0.60),
            trig(ch_hat, 16.0, Some(60), 0.86),
            trig(ch_hat, 17.0, Some(60), 0.28),
            trig(ch_ohat, 18.0, None, 0.72),
            trig(ch_hat, 20.0, Some(60), 0.76),
            trig(ch_hat, 22.0, Some(60), 0.50),
            trig(ch_hat, 23.0, Some(62), 0.32),
            trig(ch_hat, 24.0, Some(60), 0.80),
            trig(ch_hat, 26.0, Some(60), 0.52),
            trig(ch_hat, 28.0, Some(60), 0.74),
            trig(ch_hat, 30.0, Some(59), 0.68),
            trig(ch_hat, 30.667, Some(57), 0.46),
            trig(ch_hat, 31.333, Some(54), 0.30),
        ];
        v.sort_by(|a, b| a.step.total_cmp(&b.step));
        v
    });

    // Pattern 13: 2093 Wide Pulse — upper-mid stereo call-and-response
    let p_pulse = make_pat(
        "2093 Wide Pulse",
        "#5aa9e6",
        2.0,
        0.12,
        vec![
            held(ch_pulse, 0.0, 2.0, 63, 0.80),
            held(ch_pulse, 3.0, 2.0, 75, 0.72),
            held(ch_pulse, 6.0, 2.0, 64, 0.78),
            held(ch_pulse, 8.0, 3.0, 63, 0.80),
            held(ch_pulse, 14.0, 2.0, 70, 0.70),
            held(ch_pulse, 16.0, 2.0, 63, 0.82),
            held(ch_pulse, 19.0, 2.0, 75, 0.74),
            held(ch_pulse, 22.0, 3.0, 70, 0.76),
            held(ch_pulse, 26.0, 2.0, 66, 0.72),
            held(ch_pulse, 30.0, 2.0, 64, 0.78),
        ],
    );

    p.patterns = pats;

    // ---- Melodic audio loops
    let src_vox = crate::factory::source(&mut p, "loop-vox-dark-120-dmin.ogg").expect("dark vox loop");
    let src_pad = crate::factory::source(&mut p, "loop-pad-cinematic-110-ebmin.ogg").expect("cinematic pad loop");
    let src_crushed = crate::factory::source(&mut p, "loop-synth-crushed-67-dsharpmin.ogg").expect("crushed 2093 loop");
    let src_cyber = crate::factory::source(&mut p, "loop-synth-cyber-140-dsharpmin.ogg").expect("cyber synth loop");

    let mut clips: Vec<Clip> = vec![];
    let put = |clips: &mut Vec<Clip>, kind: ClipKind, track_name: &str, start: f64, len: f64, source: Id| -> usize {
        let id = clips.len() as Id + 1;
        clips.push(Clip {
            id,
            track: trk_idx(track_name),
            start,
            len,
            offset: 0.0,
            kind,
            source,
            muted: false,
            gain: 0.0,
            fade_in: 0.0,
            fade_out: 0.0,
            pan: 0.0,
            norm: false,
            rev: false,
            tune: None,
            stretch: 1.0,
            resample: false,
            pitch: 0.0,
            insert: None,
        });
        clips.len() - 1
    };

    // 1. Dark Vox (+1 st -> D#m)
    for &(start, len, gain, fout) in &[
        (0.0, 16.0, -2.0, 0.0),
        (16.0, 16.0, -2.5, 0.0),
        (32.0, 16.0, -2.5, 0.0),
        (48.0, 16.0, -3.0, 0.0),
        (64.0, 16.0, -2.5, 0.0),
        (80.0, 15.0, -2.0, 0.08),
        (128.0, 16.0, -3.5, 0.0),
        (144.0, 8.0, -3.0, 2.4),
    ] {
        let i = put(&mut clips, ClipKind::Audio, "Dark Vox", start, len, src_vox);
        clips[i].stretch = 0.8823529411764706; // 120 / 136 BPM
        clips[i].pitch = 1.0;
        clips[i].gain = gain;
        clips[i].fade_in = if start == 144.0 { 0.35 } else { 0.04 };
        clips[i].fade_out = if fout > 0.0 { fout } else { 0.06 };
    }

    // 2. Analog Pad (native Ebm = D#m)
    for &(start, len, gain, fin, fout) in &[
        (0.0, 16.0, -3.5, 0.35, 0.08),
        (32.0, 16.0, -5.0, 0.10, 0.08),
        (80.0, 15.0, -3.5, 0.15, 0.08),
        (128.0, 16.0, -4.5, 0.12, 0.08),
        (144.0, 16.0, -3.5, 0.08, 3.8),
    ] {
        let i = put(&mut clips, ClipKind::Audio, "Analog Pad", start, len, src_pad);
        clips[i].stretch = 0.8088235294117647; // 110 / 136 BPM
        clips[i].gain = gain;
        clips[i].fade_in = fin;
        clips[i].fade_out = fout;
    }

    // 3. Crushed 2093 (high-passed at 260 Hz)
    for &(start, len, gain, fin) in
        &[(84.0, 11.0, -4.5, 1.2), (96.0, 16.0, -1.5, 0.02), (112.0, 16.0, -1.5, 0.02), (128.0, 16.0, -2.5, 0.02)]
    {
        let i = put(&mut clips, ClipKind::Audio, "Crushed 2093", start, len, src_crushed);
        clips[i].stretch = 0.9852941176470588; // 134 / 136 BPM
        clips[i].gain = gain;
        clips[i].fade_in = fin;
        clips[i].fade_out = 0.05;
    }

    // 4. Cyber Synth 140 D#m -> 136 BPM
    for &(start, len, gain) in &[(112.0, 16.0, -5.5), (128.0, 16.0, -6.5)] {
        let i = put(&mut clips, ClipKind::Audio, "Cyber & Pulse", start, len, src_cyber);
        clips[i].stretch = 1.0294117647058822; // 140 / 136 BPM
        clips[i].gain = gain;
        clips[i].fade_in = 0.05;
        clips[i].fade_out = 0.08;
    }

    // 5. Pattern clips
    for &(trk, pat, spans) in &[
        ("Cyber & Pulse", p_pulse, &[(104.0, 24.0), (128.0, 16.0)][..]),
        ("808", p_reese, &[(0.0, 16.0), (80.0, 15.0)][..]),
        ("808", p_808_turn, &[(16.0, 16.0), (96.0, 16.0)][..]),
        ("808", p_808_melodic, &[(32.0, 16.0)][..]),
        ("808", p_808_verse, &[(48.0, 16.0), (144.0, 16.0)][..]),
        ("808", p_808_root, &[(64.0, 16.0)][..]),
        ("808", p_808_phrygian, &[(112.0, 16.0)][..]),
        ("808", p_808_climax, &[(128.0, 16.0)][..]),
        ("Kick", p_kick, &[(16.0, 32.0), (64.0, 16.0), (96.0, 48.0)][..]),
        ("Clap & Snare", p_clap_sparse, &[(0.0, 16.0), (48.0, 16.0), (80.0, 15.0), (144.0, 16.0)][..]),
        ("Clap & Snare", p_clap, &[(16.0, 32.0), (64.0, 16.0), (96.0, 48.0)][..]),
        ("Hats", p_hats_sparse, &[(0.0, 16.0), (64.0, 16.0), (80.0, 15.0), (144.0, 16.0)][..]),
        ("Hats", p_hats_2093, &[(16.0, 32.0), (96.0, 48.0)][..]),
    ] {
        for &(start, len) in spans {
            put(&mut clips, ClipKind::Pat, trk, start, len, pat);
        }
    }

    // ---- Mixer FX chains
    let mut fx_id: Id = 900;
    let mut make_fx = |kind: FxKind, preset: usize, kv: &[(&str, f64)]| -> Fx {
        fx_id += 1;
        let mut f = new_fx(fx_id, kind, DEMO_BPM);
        load_preset(&mut f, preset, DEMO_BPM);
        for &(k, v) in kv {
            f.params.insert(k.into(), v);
        }
        f
    };

    // Master: subtle smile EQ -> gentle Tape glue -> punch-preserving Limiter
    let m_eq = make_fx(FxKind::Eq, 8, &[]);
    let m_sat = make_fx(FxKind::Sat, 8, &[("drive", 12.0), ("mix", 35.0)]);
    let m_lim = make_fx(FxKind::Limit, 1, &[("gain", 2.6), ("ceil", -0.9), ("rel", 35.0), ("look", 1.5)]);
    p.inserts[0].fx = vec![m_eq, m_sat, m_lim];

    // Insert 1 (Vox) & Insert 2 (Pad)
    let vox_eq = make_fx(FxKind::Eq, 11, &[]);
    let vox_verb = make_fx(FxKind::Reverb, 0, &[("mix", 20.0), ("duck", 4.5)]);
    let pad_eq = make_fx(FxKind::Eq, 4, &[]);
    let pad_chorus = make_fx(FxKind::Chorus, 2, &[("hp", 190.0), ("mix", 30.0)]);

    // Insert 3 (Industrial — Crushed 2093)
    let mut ind_eq = make_fx(FxKind::Eq, 11, &[]);
    if let Some(bands) = &mut ind_eq.bands {
        bands[0].f = 260.0;
        bands[2] = EqBand {
            on: true,
            kind: BandType::Peak,
            f: 480.0,
            g: -4.5,
            q: 1.1,
            slope: 12,
            place: Placement::Mid,
            dynamic: Some(DynBand { range: -3.0, thr: -20.0 }),
        };
        bands[5] =
            EqBand { on: true, kind: BandType::Highshelf, f: 2600.0, g: 2.5, q: 0.71, slope: 12, place: Placement::Side, dynamic: None };
    }
    let ind_chorus = make_fx(FxKind::Chorus, 0, &[("hp", 260.0), ("width", 160.0), ("mix", 24.0)]);

    // Insert 4 (Lead — Cyber & 2093 Wide Pulse)
    let mut lead_eq = make_fx(FxKind::Eq, 6, &[]);
    if let Some(bands) = &mut lead_eq.bands {
        bands[0].f = 180.0;
        bands[2] = EqBand { on: true, kind: BandType::Peak, f: 550.0, g: -3.5, q: 1.2, slope: 12, place: Placement::Mid, dynamic: None };
        bands[4] =
            EqBand { on: true, kind: BandType::Peak, f: 2100.0, g: -2.5, q: 1.1, slope: 12, place: Placement::Stereo, dynamic: None };
        bands[6].f = 7800.0;
    }
    let lead_dly = make_fx(FxKind::Delay, 2, &[("mix", 24.0), ("cutoff", 4400.0), ("spread", 100.0)]);

    // Insert 5 (Synths Bus): Kick-synced Warp sidechain pocket
    let mut syn_bus_warp = make_fx(FxKind::Warp, 0, &[("tslot", 8.0), ("vslot", 2.0), ("tmix", 0.0), ("vmix", 0.0)]);
    if let Some(maps) = &mut syn_bus_warp.maps {
        let duck = |t: f64, v: f32, tension: f32| AutoPoint { t, v, tension, shape: CurveShape::Curve };
        maps.vol[2] = WarpSlot {
            name: "Trap Kick Pocket".into(),
            points: vec![
                duck(0.0, 0.25, 0.0),
                duck(0.42, 1.0, -0.55),
                duck(2.5, 1.0, 0.0),
                duck(2.5, 0.32, 0.0),
                duck(2.92, 1.0, -0.55),
                duck(4.0, 1.0, 0.0),
            ],
        };
    }
    let warp_fx_id = syn_bus_warp.id;
    let syn_bus_verb = make_fx(FxKind::Reverb, 2, &[("mix", 12.0), ("duck", 5.0), ("lo", 280.0), ("hi", 6200.0)]);

    // Insert 6 (808): Warm, smooth low end with 2000 Hz high-cut
    let mut bass_eq = make_fx(FxKind::Eq, 3, &[]);
    if let Some(bands) = &mut bass_eq.bands {
        bands[0].f = 26.0;
        bands[1].f = 44.0;
        bands[1].g = 2.0;
        bands[2].f = 210.0;
        bands[2].g = -4.0;
        bands[6].on = true;
        bands[6].f = 2000.0;
        bands[6].slope = 12;
    }
    let bass_sat = make_fx(FxKind::Sat, 5, &[("drive", 22.0), ("lowcut", 110.0), ("thump", 15.0), ("mix", 28.0)]);

    // Insert 7 (Kick): Clean low-cut + 300 Hz boxiness dip — ZERO high-end boost or crunch!
    let kick_eq = make_fx(FxKind::Eq, 4, &[]);

    // Insert 8 (Tops) & Insert 9 (Drum Bus): Clean glue comp + subtle Tape bus glue (no overdrive!)
    let tops_eq = make_fx(FxKind::Eq, 10, &[]);
    let tops_sat = make_fx(FxKind::Sat, 0, &[("drive", 18.0), ("lowcut", 95.0), ("highcut", 14000.0), ("mix", 40.0)]);
    let bus_comp = make_fx(FxKind::Comp, 2, &[("att", 30.0), ("rel", 90.0), ("ratio", 2.0), ("gain", 0.8)]);
    let bus_glue = make_fx(FxKind::Sat, 8, &[("drive", 14.0), ("mix", 32.0)]);

    // Insert 10 (Beat Bus — Half-Speed + Hall Reverb + 24 dB/oct Low-Pass Filter across the whole beat!)
    let beat_half = make_fx(FxKind::Half, 8, &[("speed", 1.0), ("loop", 3.0), ("smooth", 18.0), ("lo", 120.0), ("mix", 0.0)]);
    let beat_half_id = beat_half.id;
    let beat_verb = make_fx(FxKind::Reverb, 2, &[("size", 2.6), ("pre", 18.0), ("lo", 220.0), ("hi", 5200.0), ("duck", 1.5), ("mix", 0.0)]);
    let beat_verb_id = beat_verb.id;
    let mut beat_lpf = make_fx(FxKind::Eq, 0, &[]);
    if let Some(bands) = &mut beat_lpf.bands {
        for b in bands.iter_mut() {
            b.on = false;
        }
        bands[6] =
            EqBand { on: true, kind: BandType::Highcut, f: 20000.0, g: 0.0, q: 0.82, slope: 24, place: Placement::Stereo, dynamic: None };
    }
    let beat_lpf_id = beat_lpf.id;

    // Insert 11 (Vocal — armed & ready in D# minor)
    let voc_tune = make_fx(FxKind::Tune, 2, &[("key", 3.0), ("scale", 2.0), ("retune", 12.0), ("humanize", 15.0)]);
    let voc_eq = make_fx(FxKind::Eq, 1, &[]);
    let voc_fet = make_fx(FxKind::Fet, 1, &[]);
    let voc_deess = make_fx(FxKind::Deess, 0, &[]);

    let id_syn_bus = ins_id("Synths Bus");
    let id_drum_bus = ins_id("Drum Bus");
    let id_beat_bus = ins_id("Beat Bus");

    let configure_ins = |p: &mut Project, name: &str, vol: f32, width: f32, master: bool, sends: Vec<InsertSend>, fx: Vec<Fx>| {
        let id = ins_id(name);
        let m = p.inserts.iter_mut().find(|m| m.id == id).expect("demo insert");
        m.vol = vol;
        m.width = width;
        m.master = master;
        m.sends = sends;
        m.fx = fx;
    };

    configure_ins(&mut p, "Vox", 0.74, 1.20, false, vec![InsertSend { to: id_syn_bus, gain: 1.0 }], vec![vox_eq, vox_verb]);
    configure_ins(&mut p, "Pad", 0.70, 1.30, false, vec![InsertSend { to: id_syn_bus, gain: 1.0 }], vec![pad_eq, pad_chorus]);
    configure_ins(&mut p, "Industrial", 0.75, 1.35, false, vec![InsertSend { to: id_syn_bus, gain: 1.0 }], vec![ind_eq, ind_chorus]);
    configure_ins(&mut p, "Lead", 0.68, 1.40, false, vec![InsertSend { to: id_syn_bus, gain: 1.0 }], vec![lead_eq, lead_dly]);
    configure_ins(
        &mut p,
        "Synths Bus",
        0.77,
        1.10,
        false,
        vec![InsertSend { to: id_beat_bus, gain: 1.0 }],
        vec![syn_bus_warp, syn_bus_verb],
    );

    configure_ins(&mut p, "808", 0.73, 0.08, false, vec![InsertSend { to: id_beat_bus, gain: 1.0 }], vec![bass_eq, bass_sat]);
    configure_ins(&mut p, "Kick", 0.68, 0.0, false, vec![InsertSend { to: id_drum_bus, gain: 1.0 }], vec![kick_eq]);
    configure_ins(&mut p, "Tops", 0.74, 1.08, false, vec![InsertSend { to: id_drum_bus, gain: 1.0 }], vec![tops_eq, tops_sat]);
    configure_ins(&mut p, "Drum Bus", 0.77, 1.0, false, vec![InsertSend { to: id_beat_bus, gain: 1.0 }], vec![bus_comp, bus_glue]);

    const BEAT_BUS_VOL: f32 = 0.74;
    configure_ins(&mut p, "Beat Bus", BEAT_BUS_VOL, 1.0, true, vec![], vec![beat_half, beat_verb, beat_lpf]);
    configure_ins(&mut p, "Vocal", VOL_UNITY, 1.0, true, vec![], vec![voc_tune, voc_eq, voc_fet, voc_deess]);

    // ---- Automation clips
    let hold = |t: f64, v: f32| AutoPoint { t, v, tension: 0.0, shape: CurveShape::Hold };
    let curve = |t: f64, v: f32, tension: f32| AutoPoint { t, v, tension, shape: CurveShape::Curve };

    // 1. Beat Bus Low-Pass Filter (`band:6:f`)
    let auto_lpf_id = p.alloc_id();
    p.automations.push(Automation {
        id: auto_lpf_id,
        name: "Beat Low-Pass Filter".into(),
        color: "#69c3d6".into(),
        target: AutoTarget::Fx { fx_id: beat_lpf_id, param: "band:6:f".into() },
        points: vec![
            AutoPoint::at(0.0, 0.48),
            curve(4.0, 0.54, 0.0),
            curve(14.5, 0.78, 0.0),
            curve(16.0, 1.0, 0.20),
            hold(44.0, 1.0),
            curve(47.6, 0.50, 0.42),
            curve(48.2, 1.0, 0.25),
            hold(79.6, 1.0),
            curve(81.5, 0.52, 0.30),
            curve(87.0, 0.465, 0.0),
            curve(95.0, 0.62, 0.15),
            hold(95.6, 1.0),
            hold(124.0, 1.0),
            curve(127.4, 0.56, 0.40),
            curve(128.1, 1.0, 0.25),
            hold(143.6, 1.0),
            curve(144.8, 0.62, 0.25),
            curve(160.0, 0.32, 0.0),
        ],
    });
    put(&mut clips, ClipKind::Auto, "Beat Low-Pass", 0.0, 160.0, auto_lpf_id);

    // 2. Beat Bus Half-Speed (`mix`)
    let auto_half_id = p.alloc_id();
    p.automations.push(Automation {
        id: auto_half_id,
        name: "Beat Half-Speed".into(),
        color: "#b07cf0".into(),
        target: AutoTarget::Fx { fx_id: beat_half_id, param: "mix".into() },
        points: vec![
            AutoPoint::at(0.0, 1.0),
            hold(12.0, 1.0),
            curve(15.8, 0.0, 0.0),
            hold(80.0, 0.0),
            curve(80.6, 1.0, 0.0),
            hold(95.5, 1.0),
            hold(95.8, 0.0),
            hold(125.0, 0.0),
            curve(125.8, 0.85, 0.0),
            curve(127.9, 0.0, 0.0),
            hold(143.8, 0.0),
            curve(144.5, 1.0, 0.0),
            AutoPoint::at(160.0, 1.0),
        ],
    });
    put(&mut clips, ClipKind::Auto, "Beat Half-Time", 0.0, 160.0, auto_half_id);

    // 3. Beat Bus Hall Reverb Wash (`mix`)
    let auto_verb_id = p.alloc_id();
    p.automations.push(Automation {
        id: auto_verb_id,
        name: "Beat Reverb Wash".into(),
        color: "#47b8a8".into(),
        target: AutoTarget::Fx { fx_id: beat_verb_id, param: "mix".into() },
        points: vec![
            AutoPoint::at(0.0, 0.32),
            curve(14.5, 0.24, 0.0),
            curve(16.6, 0.0, 0.0),
            hold(44.0, 0.0),
            curve(47.6, 0.26, 0.20),
            curve(48.6, 0.0, 0.0),
            hold(80.0, 0.0),
            curve(81.0, 0.24, 0.0),
            curve(94.8, 0.40, 0.20),
            hold(95.7, 0.0),
            hold(124.5, 0.0),
            curve(127.5, 0.24, 0.20),
            curve(128.5, 0.0, 0.0),
            hold(143.8, 0.0),
            curve(144.8, 0.38, 0.0),
            AutoPoint::at(160.0, 0.45),
        ],
    });
    put(&mut clips, ClipKind::Auto, "Beat Reverb", 0.0, 160.0, auto_verb_id);

    // 4. Synths Sidechain Pump (`vmix`)
    let auto_vmix_id = p.alloc_id();
    p.automations.push(Automation {
        id: auto_vmix_id,
        name: "Synths Kick Pocket".into(),
        color: "#e0a44a".into(),
        target: AutoTarget::Fx { fx_id: warp_fx_id, param: "vmix".into() },
        points: vec![
            AutoPoint::at(0.0, 0.0),
            hold(16.0, 0.48),
            hold(48.0, 0.0),
            hold(64.0, 0.38),
            hold(80.0, 0.0),
            hold(96.0, 0.62),
            hold(144.0, 0.0),
            AutoPoint::at(160.0, 0.0),
        ],
    });
    put(&mut clips, ClipKind::Auto, "Sidechain Pump", 0.0, 160.0, auto_vmix_id);

    // 5. Beat Bus Pre-Drop Silence Cut (`95.08..95.96`), Bar 32 +1dB slingshot makeup, & Smooth Underwater Outro Fade
    let auto_bvol_id = p.alloc_id();
    p.automations.push(Automation {
        id: auto_bvol_id,
        name: "Switch Cut & Outro Fade".into(),
        color: "#d9678f".into(),
        target: AutoTarget::InsertVol { id: id_beat_bus },
        points: vec![
            AutoPoint::at(0.0, BEAT_BUS_VOL),
            hold(94.9, BEAT_BUS_VOL),
            AutoPoint::at(95.08, 0.0),
            hold(95.96, 0.0),
            AutoPoint::at(96.0, BEAT_BUS_VOL),
            hold(124.5, BEAT_BUS_VOL),
            curve(127.4, 0.77, 0.0),
            curve(128.0, BEAT_BUS_VOL, 0.0),
            hold(151.0, BEAT_BUS_VOL),
            curve(160.0, 0.0, -0.20),
        ],
    });
    put(&mut clips, ClipKind::Auto, "Switch & Fade", 0.0, 160.0, auto_bvol_id);

    // ---- Notebook structure notes
    let page_id = p.alloc_id();
    p.notebook.pages.push(LyricPage {
        id: page_id,
        name: "OBSIDIAN 2093 (136 D#m)".into(),
        start_bar: 1,
        text: concat!(
            "[Intro — Beat in Half-Time + Low-Pass + Reverb]\n\n\n\n\n",
            "[Act I — Dry Drop: D#1 Pedal -> Melodic 808]\n\n\n\n\n\n\n\n\n",
            "[Verse — Minimal D#1 Sub Pocket]\n\n\n\n\n\n\n\n\n",
            "[Bridge — Half-Time + Low-Pass Wash -> Cut]\n\n\n\n\n",
            "[Act II — 2093 Wide Synth Groove]\n\n\n\n\n\n\n\n\n",
            "[Climax — Melodic 808 + Utopia x 2093 Hybrid]\n\n\n\n\n",
            "[Outro — Underwater Half-Time Fade]"
        )
        .into(),
    });

    p.clips = clips;
    p
}
