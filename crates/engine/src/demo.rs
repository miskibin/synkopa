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

/// The demo project: 40 bars (160 beats) of 2026 nocturnal dark trap in D# minor at 136 BPM.
///
/// Built from embedded standard-library samples and built-in instruments/DSP so it opens and
/// plays identically on every machine without external downloads:
/// * Hard-hitting, clean trap kick (`Trap Kick` sculpted with `+2.5 dB @ 78 Hz` chest punch and
///   a `2.3 kHz` high-cut removing harsh fizz) snapping in front of a `22 ms`-pocketed `808 Spin`
///   bassline that stays anchored on `D#1` for 38 of the 40 bars — stepping briefly to `D#4` at
///   dry phrase endings and lifting to `G#1` (`iv`) for a single bar in Hook B and the Climax;
/// * Cohesive dark-cinematic sample & post-processed synth world: `Vox Dark` and `Pad Cinematic`
///   (flipped backwards with `rev: true` in Act II!) are joined by a BBD-chorused, half-reversed
///   (`Warp` `Reverse — co pół taktu`), dub-delayed `Ghost Bell` and a weeping Mike Dean-style
///   legato sine-triangle `Analog Glide` lead filtered below `2 kHz` inside the same dark space;
/// * Full-beat organic transitions (`Beat Bus`): `Synths Bus`, `808`, and `Drum Bus` sum through
///   automated `Half-Speed` + `Hall Reverb` + `24 dB/oct Low-Pass Filter` curves across the intro,
///   turnarounds, mid-song `380 ms` silence switch, and underwater outro.
pub fn demo_project() -> Project {
    const TRACKS: [(&str, &str, &str, &str); 15] = [
        ("Dark Vox", "#9a8cf0", "vocal", "Vox"),
        ("Analog Pad", "#47b8a8", "synth", "Pad"),
        ("Ghost Bell", "#69c3d6", "synth", "Ghost Bell"),
        ("Analog Glide", "#5aa9e6", "synth", "Glide Lead"),
        ("808", "#7c8cf8", "bass", "808"),
        ("Kick", "#4f8ef7", "drums", "Kick"),
        ("Clap & Snare", "#d9678f", "drums", "Tops"),
        ("Hats", "#e0a44a", "drums", "Tops"),
        ("Beat Low-Pass", "#69c3d6", "bus", "Beat Bus"),
        ("Beat Half-Time", "#b07cf0", "bus", "Beat Bus"),
        ("Beat Reverb", "#47b8a8", "bus", "Beat Bus"),
        ("Sidechain Pump", "#e0a44a", "bus", "Synths Bus"),
        ("Glide Filter", "#5aa9e6", "bus", "Glide Lead"),
        ("Switch & Fade", "#d9678f", "bus", "Beat Bus"),
        ("Vocal", "#5fbf8a", "vocal", "Vocal"),
    ];
    const INSERTS: [(&str, &str); 12] = [
        ("Vox", "#9a8cf0"),
        ("Pad", "#47b8a8"),
        ("Ghost Bell", "#69c3d6"),
        ("Glide Lead", "#5aa9e6"),
        ("Synths Bus", "#b07cf0"),
        ("808", "#7c8cf8"),
        ("Kick", "#4f8ef7"),
        ("Tops", "#e0a44a"),
        ("Drum Bus", "#d9678f"),
        ("Space", "#47b8a8"),
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

    // Hard-knocking trap kick (tightened tail + +3.2 dB @ 95 Hz chest punch + 2.2 kHz beater snap + 3.2 kHz high-cut removing harsh fizz!)
    let ch_kick = add_sample(&mut p, "kick-trap.flac", "Kick", "Kick", 0.85, 0.0, &|s| {
        s.end = 0.54;
        s.fade_out_ms = 24.0;
    });
    let ch_clap = add_sample(&mut p, "clap-trap-crisp.flac", "Clap Crisp", "Tops", 0.70, 0.0, &|_| {});
    let ch_clap_body = add_sample(&mut p, "clap-snare.flac", "Clap Body", "Tops", 0.67, 0.0, &|s| {
        s.end = 0.65;
        s.fade_out_ms = 42.0;
    });
    let ch_clap_wide = add_sample(&mut p, "clap-group.flac", "Wide Clap", "Tops", 0.64, 0.06, &|s| {
        s.start = 0.01;
        s.end = 0.55;
        s.semi = -1;
        s.fade_out_ms = 55.0;
    });
    let ch_snare = add_sample(&mut p, "snare-trap.flac", "Snare", "Tops", 0.68, -0.04, &|_| {});
    let ch_rev_clap = add_sample(&mut p, "clap-808.flac", "Rev Clap", "Tops", 0.52, -0.14, &|s| {
        s.reverse = true;
        s.start = 0.45;
        s.end = 0.96;
        s.fade_in_ms = 35.0;
    });
    let ch_bounce = add_sample(&mut p, "snare-trap-crisp.flac", "Bounce Snare", "Tops", 0.63, -0.16, &|_| {});
    let ch_rim = add_sample(&mut p, "rimshot-tight.flac", "Rim", "Tops", 0.65, 0.18, &|s| {
        s.end = 0.24;
        s.fade_out_ms = 22.0;
    });
    let ch_hat = add_sample(&mut p, "hat-trap-tick.flac", "Hat", "Tops", 0.64, 0.10, &|s| {
        s.cut = 1;
    });
    let ch_ohat = add_sample(&mut p, "hat-open-choked.flac", "Open Hat", "Tops", 0.61, -0.18, &|s| {
        s.cut = 1;
    });
    // 28 ms attack clears the front transient completely so every Kick slams hard in the chest before the 808 blooms
    let ch_808 = add_sample(&mut p, "bass-808-spin.flac", "808 Spin", "808", 0.78, 0.0, &|s| {
        s.attack = 28.0;
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

    // Haunted dark triangle/sine bell with soft attack & tape pitch warble (post-processed through BBD Chorus + Warp Reverse + Dub Delay!)
    let ch_bell = add_synth(
        &mut p,
        "Dzwonek Ciepły",
        "Ghost Bell",
        "#69c3d6",
        "Ghost Bell",
        0.74,
        &[
            ("amp_a", 22.0),
            ("amp_d", 2200.0),
            ("amp_r", 1100.0),
            ("flt_cutoff", 2100.0),
            ("flt_res", 8.0),
            ("l1_rate", 0.65),
            ("m2_src", 2.0),
            ("m2_dst", 0.0),
            ("m2_amt", 0.30),
            ("fx_reverb", 35.0),
            ("fx_out", 5.5),
        ],
    );

    // Mike Dean-style wide analog saw/tri legato Moog lead (warm detuned unison LP24 @ 1350 Hz — velvety depth, zero sine whistle!)
    let ch_glide = add_synth(
        &mut p,
        "Lead Gwizdek",
        "Analog Glide",
        "#5aa9e6",
        "Glide Lead",
        0.65,
        &[
            ("o1_wave", 2.0),
            ("o1_level", 68.0),
            ("o2_wave", 1.0),
            ("o2_level", 40.0),
            ("o2_coarse", 0.0),
            ("uni_voices", 2.0),
            ("uni_detune", 28.0),
            ("uni_width", 90.0),
            ("amp_a", 65.0),
            ("amp_r", 520.0),
            ("flt_type", 1.0),
            ("flt_cutoff", 1350.0),
            ("flt_res", 6.0),
            ("flt_drive", 22.0),
            ("v_mode", 2.0),
            ("v_glide", 145.0),
            ("fx_chorus", 40.0),
            ("fx_reverb", 42.0),
            ("fx_out", -13.5),
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

    // Pattern 2: 808 Root — pure D#1 (MIDI 27) root pedal (Verse B & Act II B)
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
            held(ch_808, 62.5, 1.5, 51, 0.76),
        ],
    );

    // Pattern 4: 808 Subtle Lift — stays on D#1 for Bars 1, 2 & 4, lifting ONLY in Bar 3 to G#1 (iv) and resolving darkly on D#1!
    let p_808_lift = make_pat(
        "808 Subtle Lift",
        "#9a8cf0",
        4.0,
        0.0,
        vec![
            held(ch_808, 0.0, 9.5, 27, 0.94),   // D#1
            held(ch_808, 10.0, 5.5, 27, 0.86),  // D#1
            held(ch_808, 16.0, 11.5, 27, 0.92), // D#1
            held(ch_808, 28.0, 3.5, 27, 0.82),  // D#1
            held(ch_808, 32.0, 9.5, 32, 0.92),  // G#1 (single 1-bar dark iv lift!)
            held(ch_808, 42.0, 5.5, 32, 0.86),  // G#1
            held(ch_808, 48.0, 14.5, 27, 0.94), // dark, heavy D#1 resolution into Outro
        ],
    );

    // Pattern 5: 808 Verse — ultra-minimal 1-hit-per-bar D#1 sub pillow (Verse A & Outro)
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

    // Pattern 6: Kick Punch — hard, tight trap knock locked to the 808 downbeats & syncopations
    let p_kick = make_pat(
        "Kick Punch",
        "#4f8ef7",
        2.0,
        0.0,
        [(0.0, 1.0), (10.0, 0.92), (16.0, 0.98), (28.0, 0.88)].into_iter().map(|(s, v)| trig(ch_kick, s, None, v)).collect(),
    );

    // Pattern 7: Clap & Bounce — 4-layer backbeat (pre-flammed Wide Clap + warm Clap Body + Clap Crisp + Snare + Rev Clap pre-suck!)
    let p_clap = make_pat("Clap & Bounce", "#d9678f", 2.0, 0.14, {
        let mut v = vec![
            trig(ch_clap_wide, 7.94, None, 0.80),
            trig(ch_clap, 8.0, None, 0.90),
            trig(ch_clap_body, 8.0, None, 0.84),
            trig(ch_snare, 8.0, None, 0.76),
            held(ch_rev_clap, 22.0, 2.0, 60, 0.68),
            trig(ch_clap_wide, 23.94, None, 0.84),
            trig(ch_clap, 24.0, None, 0.92),
            trig(ch_clap_body, 24.0, None, 0.86),
            trig(ch_snare, 24.0, None, 0.80),
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

    // Pattern 8: Verse Strip — layered 3-sample Clap (Wide flam + Body + Crisp) so even the stripped Verse clap sounds full & 3D!
    let p_clap_sparse = make_pat("Verse Strip", "#d9678f", 2.0, 0.16, {
        let mut v = vec![
            trig(ch_clap_wide, 7.94, None, 0.72),
            trig(ch_clap, 8.0, None, 0.86),
            trig(ch_clap_body, 8.0, None, 0.80),
            trig(ch_rim, 14.0, None, 0.62),
            trig(ch_clap_wide, 23.94, None, 0.76),
            trig(ch_clap, 24.0, None, 0.88),
            trig(ch_clap_body, 24.0, None, 0.82),
            trig(ch_rim, 27.0, None, 0.54),
            trig(ch_bounce, 30.0, None, 0.58),
        ];
        v.sort_by(|a, b| a.step.total_cmp(&b.step));
        v
    });

    // Pattern 9: Hats Nocturne (18% swing + deep ghosting)
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

    // Pattern 10: Hats 2093 (18% BNYX micro-swing + wide velocity ghosting + pitched rolls)
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

    // Pattern 11: Ghost Bell — sparse, aching 4-bar motif processed through BBD Chorus + Warp Half-Bar Reverse + Dub Delay
    // Strictly voices D#m (A#4, F#4), A#m/G#m (F4, D#5, B4) so it melts into Dark Vox & Analog Pad like a vintage sample!
    let p_ghost_bell = make_pat(
        "Ghost Bell",
        "#69c3d6",
        4.0,
        0.0,
        vec![
            held(ch_bell, 4.0, 6.0, 70, 0.68),  // A#4
            held(ch_bell, 12.0, 4.0, 66, 0.58), // F#4
            held(ch_bell, 20.0, 8.0, 65, 0.64), // F4
            held(ch_bell, 36.0, 6.0, 75, 0.66), // D#5
            held(ch_bell, 44.0, 4.0, 70, 0.58), // A#4
            held(ch_bell, 52.0, 8.0, 66, 0.62), // F#4
        ],
    );

    // Pattern 12: Analog Glide — slow, weeping 1-bar Mike Dean legato cries in Act II B & Climax
    let p_glide_act2 = make_pat(
        "Analog Glide (Rev)",
        "#5aa9e6",
        4.0,
        0.0,
        vec![
            held(ch_glide, 2.0, 14.5, 70, 0.74),  // A#4 (over D#m)
            held(ch_glide, 16.0, 16.5, 71, 0.76), // glides +1 st to B4 (over Reversed Pad G#m/B!)
            held(ch_glide, 32.0, 16.5, 65, 0.72), // glides down to F4 (over Reversed Pad A#m!)
            held(ch_glide, 48.0, 14.0, 66, 0.72), // glides to F#4 (over D#m)
        ],
    );
    let p_glide_climax = make_pat(
        "Analog Glide (Climax)",
        "#5aa9e6",
        4.0,
        0.0,
        vec![
            held(ch_glide, 2.0, 14.5, 70, 0.74),  // A#4 (over D#m)
            held(ch_glide, 16.0, 16.5, 65, 0.72), // glides to F4 (over Forward Pad A#m!)
            held(ch_glide, 32.0, 16.5, 71, 0.78), // glides up to B4 (over Forward Pad G#m/B!)
            held(ch_glide, 48.0, 14.0, 66, 0.72), // glides down to F#4 (over D#m)
        ],
    );

    p.patterns = pats;

    // ---- Melodic audio loops
    let src_vox = crate::factory::source(&mut p, "loop-vox-dark-120-dmin.ogg").expect("dark vox loop");
    let src_pad = crate::factory::source(&mut p, "loop-pad-cinematic-110-ebmin.ogg").expect("cinematic pad loop");

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
        (112.0, 16.0, -3.5, 0.0),
        (128.0, 16.0, -2.5, 0.0),
        (144.0, 8.0, -3.0, 2.4),
    ] {
        let i = put(&mut clips, ClipKind::Audio, "Dark Vox", start, len, src_vox);
        clips[i].stretch = 0.8823529411764706; // 120 / 136 BPM
        clips[i].pitch = 1.0;
        clips[i].gain = gain;
        clips[i].fade_in = if start == 144.0 { 0.35 } else { 0.04 };
        clips[i].fade_out = if fout > 0.0 { fout } else { 0.06 };
    }

    // 2. Analog Pad — forward in Act I, Bridge, Climax & Outro; REVERSED (`rev: true`) in Act II (96..128) for a seamless dark tape-flip switch!
    for &(start, len, gain, fin, fout, rev) in &[
        (0.0, 16.0, -3.5, 0.35, 0.08, false),
        (32.0, 16.0, -4.5, 0.10, 0.08, false),
        (80.0, 15.0, -3.5, 0.15, 0.08, false),
        (96.0, 16.0, -3.0, 0.05, 0.08, true),  // Act II A: Reversed Analog Pad flip!
        (112.0, 16.0, -4.0, 0.08, 0.08, true), // Act II B: Reversed Analog Pad under Ghost Bell & Glide!
        (128.0, 16.0, -4.0, 0.10, 0.08, false),
        (144.0, 16.0, -3.5, 0.08, 3.8, false),
    ] {
        let i = put(&mut clips, ClipKind::Audio, "Analog Pad", start, len, src_pad);
        clips[i].stretch = 0.8088235294117647; // 110 / 136 BPM
        clips[i].gain = gain;
        clips[i].fade_in = fin;
        clips[i].fade_out = fout;
        clips[i].rev = rev;
    }

    // 3. Pattern clips (foreshadowing Ghost Bell subtly in Hook B & Bridge so Act II / Climax feels 100% organic, never tacked-on!)
    for &(trk, pat, spans) in &[
        ("Ghost Bell", p_ghost_bell, &[(32.0, 16.0), (80.0, 15.0), (96.0, 32.0), (128.0, 16.0)][..]),
        ("Analog Glide", p_glide_act2, &[(112.0, 16.0)][..]),
        ("Analog Glide", p_glide_climax, &[(128.0, 16.0)][..]),
        ("808", p_reese, &[(0.0, 16.0), (80.0, 15.0)][..]),
        ("808", p_808_turn, &[(16.0, 16.0), (96.0, 16.0)][..]),
        ("808", p_808_lift, &[(32.0, 16.0), (128.0, 16.0)][..]),
        ("808", p_808_verse, &[(48.0, 16.0), (144.0, 16.0)][..]),
        ("808", p_808_root, &[(64.0, 16.0), (112.0, 16.0)][..]),
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
    let m_lim = make_fx(FxKind::Limit, 1, &[("gain", 3.4), ("ceil", -0.9), ("rel", 35.0), ("look", 1.5)]);
    p.inserts[0].fx = vec![m_eq, m_sat, m_lim];

    // Insert 1 (Vox) & Insert 2 (Pad: Dynamic EQ 185 Hz ducking + synced 2-bar Stereo Phaser + Ensemble Chorus!)
    let vox_eq = make_fx(FxKind::Eq, 11, &[]);
    let vox_verb = make_fx(FxKind::Reverb, 0, &[("mix", 20.0), ("duck", 4.5)]);
    let mut pad_eq = make_fx(FxKind::Eq, 4, &[]);
    if let Some(bands) = &mut pad_eq.bands {
        bands[1] = EqBand {
            on: true,
            kind: BandType::Peak,
            f: 185.0,
            g: -2.0,
            q: 1.3,
            slope: 12,
            place: Placement::Stereo,
            dynamic: Some(DynBand { range: -3.5, thr: -22.0 }),
        };
    }
    let pad_phaser = make_fx(FxKind::Phaser, 7, &[("mix", 20.0)]);
    let pad_chorus = make_fx(FxKind::Chorus, 2, &[("hp", 190.0), ("mix", 28.0)]);

    // Insert 3 (Ghost Bell — heavy analog post-processing so it sounds like a warped vintage tape sample, NEVER raw MIDI!):
    // 1. 220 Hz..2.2 kHz dark tape bandpass EQ
    // 2. Vintage BBD Chorus (`preset 3`)
    // 3. Warp (`tslot 11` = `Reverse — co pół taktu` at 55% mix!)
    // 4. Tape Dub Delay (`preset 8`, cutoff 1900 Hz) + Dark Hall Reverb (highcut 3000 Hz)
    let mut bell_eq = make_fx(FxKind::Eq, 6, &[]);
    if let Some(bands) = &mut bell_eq.bands {
        bands[0].f = 220.0;
        bands[6].f = 2200.0;
    }
    let bell_chorus = make_fx(FxKind::Chorus, 3, &[("tone", 4200.0), ("mix", 45.0)]);
    let bell_warp = make_fx(FxKind::Warp, 0, &[("tslot", 11.0), ("tmix", 55.0), ("vmix", 0.0)]);
    let bell_dly = make_fx(FxKind::Delay, 8, &[("cutoff", 1900.0), ("mix", 28.0)]);
    let bell_verb = make_fx(FxKind::Reverb, 2, &[("size", 2.8), ("lo", 260.0), ("hi", 3000.0), ("duck", 3.5), ("mix", 42.0)]);

    // Insert 4 (Glide Lead — Mike Dean weeping wide analog Moog lead with Mid/Side EQ sculpting!):
    let mut glide_eq = make_fx(FxKind::Eq, 6, &[]);
    if let Some(bands) = &mut glide_eq.bands {
        bands[0].f = 240.0;
        bands[3] = EqBand { on: true, kind: BandType::Peak, f: 1050.0, g: -3.0, q: 1.1, slope: 12, place: Placement::Mid, dynamic: None };
        bands[5] =
            EqBand { on: true, kind: BandType::Highshelf, f: 2400.0, g: 2.0, q: 0.71, slope: 12, place: Placement::Side, dynamic: None };
        bands[6].f = 2200.0;
    }
    let glide_chorus = make_fx(FxKind::Chorus, 2, &[("hp", 220.0), ("mix", 45.0)]);
    let glide_dly = make_fx(FxKind::Delay, 8, &[("cutoff", 1900.0), ("mix", 36.0)]);
    let glide_verb = make_fx(FxKind::Reverb, 2, &[("size", 3.0), ("lo", 260.0), ("hi", 2900.0), ("duck", 4.5), ("mix", 48.0)]);

    // Insert 5 (Synths Bus): Deep Kick-synced Warp sidechain pocket so every Kick slams straight through the pads & vocals!
    let mut syn_bus_warp = make_fx(FxKind::Warp, 0, &[("tslot", 8.0), ("vslot", 2.0), ("tmix", 0.0), ("vmix", 0.0)]);
    if let Some(maps) = &mut syn_bus_warp.maps {
        let duck = |t: f64, v: f32, tension: f32| AutoPoint { t, v, tension, shape: CurveShape::Curve };
        maps.vol[2] = WarpSlot {
            name: "Trap Kick Pocket".into(),
            points: vec![
                duck(0.0, 0.10, 0.0),
                duck(0.44, 1.0, -0.55),
                duck(2.5, 1.0, 0.0),
                duck(2.5, 0.16, 0.0),
                duck(2.94, 1.0, -0.55),
                duck(4.0, 1.0, 0.0),
            ],
        };
    }
    let warp_fx_id = syn_bus_warp.id;
    let syn_bus_verb = make_fx(FxKind::Reverb, 2, &[("mix", 12.0), ("duck", 5.0), ("lo", 280.0), ("hi", 6000.0)]);

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

    // Insert 7 (Kick): Hard 95 Hz sternum thump + 2.2 kHz crisp beater snap + 3.2 kHz high-cut (100% physical slam, 0% harsh fizz!)
    let mut kick_eq = make_fx(FxKind::Eq, 0, &[("out", -1.0)]);
    if let Some(bands) = &mut kick_eq.bands {
        bands[0] =
            EqBand { on: true, kind: BandType::Lowcut, f: 28.0, g: 0.0, q: 0.71, slope: 12, place: Placement::Stereo, dynamic: None };
        bands[1] = EqBand { on: true, kind: BandType::Peak, f: 95.0, g: 3.2, q: 0.9, slope: 12, place: Placement::Stereo, dynamic: None };
        bands[2] = EqBand { on: true, kind: BandType::Peak, f: 310.0, g: -3.5, q: 1.3, slope: 12, place: Placement::Stereo, dynamic: None };
        bands[3].on = false;
        bands[4] = EqBand { on: true, kind: BandType::Peak, f: 2200.0, g: 2.0, q: 1.1, slope: 12, place: Placement::Stereo, dynamic: None };
        bands[5].on = false;
        bands[6] =
            EqBand { on: true, kind: BandType::Highcut, f: 3200.0, g: 0.0, q: 0.71, slope: 12, place: Placement::Stereo, dynamic: None };
    }

    // Insert 8 (Tops) & Insert 9 (Drum Bus): Crisp punch comp + clean Tape bus glue
    let tops_eq = make_fx(FxKind::Eq, 10, &[]);
    let tops_sat = make_fx(FxKind::Sat, 0, &[("drive", 18.0), ("lowcut", 95.0), ("highcut", 14000.0), ("mix", 40.0)]);
    let bus_comp = make_fx(FxKind::Comp, 2, &[("att", 32.0), ("rel", 80.0), ("ratio", 2.2), ("gain", 1.2)]);
    let bus_glue = make_fx(FxKind::Sat, 8, &[("drive", 16.0), ("mix", 35.0)]);

    // Insert 10 (Space — 100% wet parallel Hall send bus giving the layered Claps, Vox & Ghost Bell shared 3D depth!)
    let space_verb = make_fx(FxKind::Reverb, 6, &[("size", 2.5), ("lo", 280.0), ("hi", 4800.0)]);

    // Insert 11 (Beat Bus — Half-Speed + Hall Reverb + 24 dB/oct Low-Pass Filter across the whole beat!)
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

    // Insert 12 (Vocal — complete 6-stage WAVE Vocal Suite armed & ready in D# minor: Denoiser -> Rider -> Pitch Correct -> EQ -> FET -> De-esser!)
    let mut voc_denoise = make_fx(FxKind::Denoise, 0, &[]);
    voc_denoise.bypass = true; // ready on the strip without adding FFT latency before a mic is connected
    let voc_rider = make_fx(FxKind::Rider, 1, &[]);
    let voc_tune = make_fx(FxKind::Tune, 2, &[("key", 3.0), ("scale", 2.0), ("retune", 12.0), ("humanize", 15.0)]);
    let voc_eq = make_fx(FxKind::Eq, 1, &[]);
    let voc_fet = make_fx(FxKind::Fet, 1, &[]);
    let voc_deess = make_fx(FxKind::Deess, 0, &[]);

    let id_syn_bus = ins_id("Synths Bus");
    let id_drum_bus = ins_id("Drum Bus");
    let id_space = ins_id("Space");
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

    configure_ins(
        &mut p,
        "Vox",
        0.74,
        1.20,
        false,
        vec![InsertSend { to: id_syn_bus, gain: 1.0 }, InsertSend { to: id_space, gain: 0.16 }],
        vec![vox_eq, vox_verb],
    );
    configure_ins(&mut p, "Pad", 0.71, 1.30, false, vec![InsertSend { to: id_syn_bus, gain: 1.0 }], vec![pad_eq, pad_phaser, pad_chorus]);
    configure_ins(
        &mut p,
        "Ghost Bell",
        0.74,
        1.35,
        false,
        vec![InsertSend { to: id_syn_bus, gain: 1.0 }, InsertSend { to: id_space, gain: 0.20 }],
        vec![bell_eq, bell_chorus, bell_warp, bell_dly, bell_verb],
    );
    configure_ins(
        &mut p,
        "Glide Lead",
        0.66,
        1.35,
        false,
        vec![InsertSend { to: id_syn_bus, gain: 1.0 }],
        vec![glide_eq, glide_chorus, glide_dly, glide_verb],
    );
    configure_ins(
        &mut p,
        "Synths Bus",
        0.75,
        1.10,
        false,
        vec![InsertSend { to: id_beat_bus, gain: 1.0 }],
        vec![syn_bus_warp, syn_bus_verb],
    );

    configure_ins(&mut p, "808", 0.72, 0.08, false, vec![InsertSend { to: id_beat_bus, gain: 1.0 }], vec![bass_eq, bass_sat]);
    configure_ins(&mut p, "Kick", 0.81, 0.0, false, vec![InsertSend { to: id_drum_bus, gain: 1.0 }], vec![kick_eq]);
    configure_ins(
        &mut p,
        "Tops",
        0.71,
        1.10,
        false,
        vec![InsertSend { to: id_drum_bus, gain: 1.0 }, InsertSend { to: id_space, gain: 0.16 }],
        vec![tops_eq, tops_sat],
    );
    configure_ins(&mut p, "Drum Bus", 0.78, 1.0, false, vec![InsertSend { to: id_beat_bus, gain: 1.0 }], vec![bus_comp, bus_glue]);
    configure_ins(&mut p, "Space", 0.66, 1.35, false, vec![InsertSend { to: id_beat_bus, gain: 1.0 }], vec![space_verb]);

    const BEAT_BUS_VOL: f32 = 0.70;
    configure_ins(&mut p, "Beat Bus", BEAT_BUS_VOL, 1.0, true, vec![], vec![beat_half, beat_verb, beat_lpf]);
    configure_ins(
        &mut p,
        "Vocal",
        VOL_UNITY,
        1.0,
        true,
        vec![InsertSend { to: id_space, gain: 0.18 }],
        vec![voc_denoise, voc_rider, voc_tune, voc_eq, voc_fet, voc_deess],
    );

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
            hold(16.0, 0.72),
            hold(48.0, 0.0),
            hold(64.0, 0.55),
            hold(80.0, 0.0),
            hold(96.0, 0.78),
            hold(144.0, 0.0),
            AutoPoint::at(160.0, 0.0),
        ],
    });
    put(&mut clips, ClipKind::Auto, "Sidechain Pump", 0.0, 160.0, auto_vmix_id);

    // 5. Synth Cutoff Automation (`AutoTarget::Synth`) — gently opens Analog Glide's Moog filter across Act II B & Climax!
    let auto_glide_flt_id = p.alloc_id();
    p.automations.push(Automation {
        id: auto_glide_flt_id,
        name: "Glide Moog Cutoff".into(),
        color: "#5aa9e6".into(),
        target: AutoTarget::Synth { chan_id: ch_glide, param: "flt_cutoff".into() },
        points: vec![
            AutoPoint::at(0.0, 0.585),
            curve(24.0, 0.638, 0.15),
            AutoPoint { t: 32.0, v: 0.595, tension: 0.0, shape: CurveShape::Double },
        ],
    });
    put(&mut clips, ClipKind::Auto, "Glide Filter", 112.0, 32.0, auto_glide_flt_id);

    // 6. Beat Bus Pre-Drop Silence Cut (`95.08..95.96`), Bar 32 +1dB slingshot makeup, & Smooth S-Curve Underwater Outro Fade
    let auto_bvol_id = p.alloc_id();
    p.automations.push(Automation {
        id: auto_bvol_id,
        name: "Switch Cut & Outro Fade".into(),
        color: "#d9678f".into(),
        target: AutoTarget::InsertVol { id: id_beat_bus },
        points: vec![
            AutoPoint::at(0.0, BEAT_BUS_VOL),
            hold(94.82, BEAT_BUS_VOL),
            AutoPoint::at(94.98, 0.0),
            hold(95.96, 0.0),
            AutoPoint::at(96.0, BEAT_BUS_VOL),
            hold(124.5, BEAT_BUS_VOL),
            curve(127.4, 0.75, 0.0),
            curve(128.0, BEAT_BUS_VOL, 0.0),
            hold(151.0, BEAT_BUS_VOL),
            AutoPoint { t: 160.0, v: 0.0, tension: 0.0, shape: CurveShape::Double },
        ],
    });
    put(&mut clips, ClipKind::Auto, "Switch & Fade", 0.0, 160.0, auto_bvol_id);

    // ---- Multi-page bar-synced Notebook (each line lights up on its bar during playback!)
    let page_tour_id = p.alloc_id();
    let page_vox_id = p.alloc_id();
    p.notebook.pages = vec![
        LyricPage {
            id: page_tour_id,
            name: "Producer Tour".into(),
            start_bar: 1,
            text: concat!(
                "[Intro — Full Beat on Beat Bus: Half-Speed + Hall Reverb + Low-Pass Sweep]\n",
                "Submerged full-beat intro (~550 Hz LPF + 100% Half-Speed)\n",
                "Analog Pad (Dynamic EQ + 2-bar Stereo Phaser + Chorus)\n",
                "Dark Vox (+1 st pitch-preserving stretch: 120 -> 136 BPM)\n",
                "Low-Pass sweeps wide open into the dry drop\n",
                "[Act I — Hard Sternum Kick + D#1 808 + 4-Layer Flam Clap]\n",
                "Kick (95 Hz punch + 28 ms 808 pocket) + Warp sidechain ducking\n",
                "Backbeat: pre-flammed Wide Clap + Clap Body + Crisp + Snare + Rev Clap\n",
                "Hats 2093: 18% BNYX swing + cut-group choked Open Hat + triplet rolls\n",
                "Woody D#3 turnaround flick on the last 16th\n",
                "Ghost Bell enters (BBD Chorus + Warp 1/2-bar Reverse + Tape Dub Delay)\n",
                "Parallel Space send bus adds shared 3D hall bloom\n",
                "808 Subtle Lift steps to G#1 (iv) for one bar\n",
                "Beat Bus Low-Pass + Reverb dunk melts Hook into Verse\n",
                "[Verse — Minimal D#1 Sub Pillow & Open Vocal Pocket]\n",
                "Bare 1-hit-per-bar D#1 808 + 3-layer Clap & Rimshot\n",
                "Vocal insert (Ins 12) armed in D#m: Denoiser -> Rider -> Tune -> EQ -> FET -> De-ess\n\n\n",
                "Swung Nocturne Hats & Kick re-enter for Verse B\n\n\n\n",
                "[Beat Switch — Half-Time Cathedral Wash -> 380 ms Silence Cut]\n",
                "Entire Beat Bus drops into 100% Half-Speed + swelling Hall Reverb\n\n",
                "Low-Pass creeps upward (500 Hz -> 1.45 kHz) building tension\n",
                "380 ms digital-zero silence vacuum wipes all tails before Beat 1\n",
                "[Act II — Reversed Analog Pad Flip + Weeping Moog Lead]\n",
                "Analog Pad flipped backwards (Clip rev: true) over D#1 808\n\n\n",
                "Woody D#3 808 turnaround inflection\n",
                "Analog Glide enters: unison saw Moog portamento + automated cutoff\n\n\n",
                "Half-Speed + Low-Pass slingshot (+1 dB makeup) into Climax\n",
                "[Climax — Forward Pad + Dark Vox + Ghost Bell + Analog Glide]\n",
                "All harmonic layers unite; Kick/Snare slice through via Warp pocket\n\n",
                "Bar 35 G#1 bass lift -> dark D#1 resolution\n\n",
                "[Outro — Underwater Half-Time Fade]\n",
                "Beat Bus submerges back into Half-Speed + Reverb\n\n",
                "Low-Pass closes to 180 Hz with S-curve fade to silence\n"
            )
            .into(),
        },
        LyricPage {
            id: page_vox_id,
            name: "Vocal Pad (D#m)".into(),
            start_bar: 13,
            text: concat!(
                "[Verse — 136 BPM · D# minor (Ins 12 armed with live Pitch Correct)]\n",
                "midnight chrome on the windowpane\n",
                "low tide hummin' through the heavy rain\n",
                "keep the circle quiet, keep the vision plain\n",
                "every shadow movin' in a slower lane\n",
                "[Verse B — Drums In]\n",
                "step out clean when the filter breaks\n",
                "cold neon glow on the silver lakes\n",
                "no false moves, yeah we know the stakes\n",
                "let the sub talk back till the ceiling shakes"
            )
            .into(),
        },
    ];

    p.clips = clips;
    p
}
