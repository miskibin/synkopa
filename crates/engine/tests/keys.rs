//! Synkopa Keys: sampled SFZ instruments as Channel Rack channels (model, ops, rendering).

use std::sync::Arc;

use wave_engine::keys::{bank_from, KeysBank, DEFAULT_INSTRUMENT};
use wave_engine::sources::Audio;
use wave_engine::*;

const SR: u32 = 48000;
/// 128 BPM: a sixteenth is 5625 frames.
const STEP: usize = 5625;

fn rms(x: &[f32]) -> f64 {
    (x.iter().map(|v| (*v as f64).powi(2)).sum::<f64>() / x.len().max(1) as f64).sqrt()
}

/// `secs` of a steady sine at `hz`, peak `amp` (a stand-in for one recording).
fn tone_for(hz: f64, amp: f32, secs: f64) -> Audio {
    let n = (SR as f64 * secs) as usize;
    let mut d = Vec::with_capacity(n * 2);
    for i in 0..n {
        let v = (i as f64 * hz * std::f64::consts::TAU / SR as f64).sin() as f32 * amp;
        d.extend([v, v]);
    }
    Audio::from_stereo(d, SR, 2)
}

fn tone(hz: f64, amp: f32) -> Audio {
    tone_for(hz, amp, 2.0)
}

/// C4 and D#4, each soft (quiet) and loud, levelled inside their velocity bands the way the
/// grand piano is (continuous across the boundary).
const LAYERS: &str = "
<global> ampeg_release=0.35
<group> lovel=1 hivel=63 amp_velcurve_1=0.03 amp_velcurve_32=1 amp_velcurve_63=1.97
<region> sample=c-soft lokey=0 hikey=61 pitch_keycenter=60
<region> sample=ds-soft lokey=62 hikey=127 pitch_keycenter=63
<group> lovel=64 hivel=127 amp_velcurve_64=0.4 amp_velcurve_96=0.6 amp_velcurve_127=0.79
<region> sample=c-loud lokey=0 hikey=61 pitch_keycenter=60
<region> sample=ds-loud lokey=62 hikey=127 pitch_keycenter=63
";

/// A key-release click on every key, 20 dB down.
const CLICK: &str = "<group> trigger=release volume=-20 amp_veltrack=0 <region> sample=click lokey=0 hikey=127";

fn layers() -> Vec<(&'static str, Audio)> {
    vec![("c-soft", tone(261.63, 0.1)), ("c-loud", tone(261.63, 0.5)), ("ds-soft", tone(311.13, 0.1)), ("ds-loud", tone(311.13, 0.5))]
}

fn bank() -> Arc<KeysBank> {
    Arc::new(bank_from(LAYERS, layers()).unwrap())
}

fn bank_with_click() -> Arc<KeysBank> {
    let mut s = layers();
    s.push(("click", tone_for(1000.0, 0.5, 0.05)));
    Arc::new(bank_from(&format!("{LAYERS}\n{CLICK}"), s).unwrap())
}

const CH: Id = 2000;

fn project(notes: Vec<Note>, keys: KeysState) -> Project {
    let mut p = Project::empty();
    p.inserts[0].fx.clear();
    p.channels.push(Channel {
        name: "Synkopa Keys".into(),
        src: ChannelSrc::Keys { keys },
        root: 60,
        ..Channel::new(CH, "", "#4f8ef7", ChannelSrc::Keys { keys: Default::default() })
    });
    p.patterns = vec![Pattern { id: 1, name: "P".into(), color: "#4f8ef7".into(), len: 4.0, min_len: 4.0, notes, swing: 0.0 }];
    p.clips = vec![Clip {
        id: 1,
        track: 0,
        start: 0.0,
        len: 4.0,
        offset: 0.0,
        kind: ClipKind::Pat,
        source: 1,
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
        root: 60,
    }];
    p
}

fn render(p: Project, with_bank: bool) -> Vec<f32> {
    render_with(p, with_bank.then(bank))
}

fn render_with(p: Project, bank: Option<Arc<KeysBank>>) -> Vec<f32> {
    let e = Engine::new(p, Arc::new(NoPlugins));
    if let Some(b) = bank {
        e.set_keys_bank(DEFAULT_INSTRUMENT, Some(b));
    }
    e.render(0.0, 4.0, 0.0, SR).unwrap().as_chunks::<2>().0.iter().map(|f| f[0]).collect()
}

fn note(step: f32, len: f32, key: u8, v: f32) -> Note {
    Note { v, ..Note::new(CH, step, len, Some(key)) }
}

/// Dominant frequency of `x` by zero crossings.
fn freq(x: &[f32]) -> f64 {
    let n = x.windows(2).filter(|w| w[0] <= 0.0 && w[1] > 0.0).count();
    n as f64 * SR as f64 / x.len() as f64
}

#[test]
fn silent_until_the_instrument_is_loaded() {
    let p = project(vec![note(0.0, 4.0, 60, 0.9)], KeysState::default());
    assert_eq!(rms(&render(p.clone(), false)), 0.0);
    assert!(rms(&render(p.clone(), true)) > 0.005);
    // a bank loaded for another instrument does not play this channel
    let e = Engine::new(p, Arc::new(NoPlugins));
    e.set_keys_bank("wurlitzer", Some(bank()));
    assert_eq!(rms(&e.render(0.0, 4.0, 0.0, SR).unwrap()), 0.0);
    assert_eq!(e.keys_instruments(), [DEFAULT_INSTRUMENT]);
}

#[test]
fn plays_the_nearest_recording_at_the_note_pitch() {
    // D4 (62) is mapped to D#4 (63): that recording, one semitone down
    let x = render(project(vec![note(0.0, 8.0, 62, 0.9)], KeysState::default()), true);
    let f = freq(&x[2000..40000]);
    assert!((f - 293.66).abs() < 3.0, "D4 expected, got {f:.1} Hz");
    let x = render(project(vec![note(0.0, 8.0, 60, 0.9)], KeysState::default()), true);
    let f = freq(&x[2000..40000]);
    assert!((f - 261.63).abs() < 3.0, "C4 expected, got {f:.1} Hz");
}

#[test]
fn velocity_picks_the_layer_and_dynamics_scales_inside_it() {
    let level = |v: f32, dynamics: f32| {
        rms(&render(project(vec![note(0.0, 8.0, 60, v)], KeysState { dynamics, ..Default::default() }), true)[2000..40000])
    };
    // the loud recording is five times the soft one
    assert!(level(0.9, 0.0) / level(0.2, 0.0) > 4.5);
    // with dynamics off every note of a layer is as loud as the recording
    assert!((level(0.6, 0.0) / level(0.9, 0.0) - 1.0).abs() < 0.01);
    // with dynamics on a harder hit inside the layer is louder
    assert!(level(0.95, 100.0) > level(0.6, 100.0) * 1.3);
    // across the layer boundary the level is continuous: the timbre changes, the level does not jump
    let (below, above) = (level(63.0 / 127.0, 100.0), level(64.0 / 127.0, 100.0));
    assert!((above / below - 1.0).abs() < 0.08, "{below} -> {above}");
}

#[test]
fn a_note_rings_for_its_length_then_fades_over_the_release() {
    // a one-step note, then silence
    let short = render(project(vec![note(0.0, 1.0, 60, 0.9)], KeysState { release: Some(40.0), ..Default::default() }), true);
    let long = render(project(vec![note(0.0, 1.0, 60, 0.9)], KeysState { release: Some(2000.0), ..Default::default() }), true);
    assert!(rms(&short[1000..5000]) > 0.05, "the note sounds while held");
    // 100 ms after the key is let go: gone with a 40 ms release, still ringing with 2 s
    let after = STEP + 4800..STEP + 9600;
    assert!(rms(&short[after.clone()]) < 1e-3);
    assert!(rms(&long[after.clone()]) > 0.025);
    // without an override, the instrument's own 350 ms: still there at 100 ms, gone by 1 s
    let own = render(project(vec![note(0.0, 1.0, 60, 0.9)], KeysState::default()), true);
    assert!(rms(&own[after]) > 1e-3);
    assert!(rms(&own[STEP + 48000..STEP + 52800]) < 1e-3);
    // and a release never ends in a step (no click): the largest jump stays small
    let jump = short.windows(2).map(|w| (w[1] - w[0]).abs()).fold(0f32, f32::max);
    assert!(jump < 0.05, "click of {jump}");
}

#[test]
fn settings_are_validated_saved_and_undone() {
    let mut p = project(vec![], KeysState::default());
    let ch = p.channels.iter().find(|c| c.id == CH).unwrap().clone();
    let keys = |release, dynamics, instrument: &str| KeysState { instrument: instrument.into(), release, dynamics, articulation: None };
    for k in [keys(Some(5.0), 100.0, DEFAULT_INSTRUMENT), keys(Some(300.0), 140.0, DEFAULT_INSTRUMENT), keys(None, 100.0, "kazoo")] {
        let bad = Channel { src: ChannelSrc::Keys { keys: k.clone() }, ..ch.clone() };
        assert!(ops::apply(&mut p, &Op::PutChannel { channel: bad, index: None }).is_err(), "{k:?}");
    }

    let e = Engine::new(p, Arc::new(NoPlugins));
    let good = Channel { src: ChannelSrc::Keys { keys: keys(Some(900.0), 40.0, "wurlitzer") }, ..ch };
    e.apply(&[Op::PutChannel { channel: good.clone(), index: None }], false, None).unwrap();
    assert_eq!(e.project().channel(CH).unwrap().src, good.src);
    assert_eq!(e.keys_instruments(), ["wurlitzer"]);

    // JSON: {"kind":"keys","keys":{"instrument":"wurlitzer","release":900,"dynamics":40}}, and a
    // bare {"kind":"keys"} loads with the defaults
    let v = serde_json::to_value(e.project().channel(CH).unwrap()).unwrap();
    assert_eq!(v["kind"], "keys");
    assert_eq!(v["keys"]["release"], 900.0);
    assert_eq!(v["keys"]["instrument"], "wurlitzer");
    let mut bare = v.clone();
    bare.as_object_mut().unwrap().remove("keys");
    let c: Channel = serde_json::from_value(bare).unwrap();
    assert_eq!(c.src, ChannelSrc::Keys { keys: KeysState::default() });
    // a project saved before there was an instrument choice plays the grand piano, with its release
    let old: KeysState = serde_json::from_str(r#"{"release":350,"dynamics":100}"#).unwrap();
    assert_eq!(old, keys(Some(350.0), 100.0, DEFAULT_INSTRUMENT));
    // a new channel follows the instrument's own release and does not write one
    assert!(serde_json::to_value(KeysState::default()).unwrap().get("release").is_none());

    e.undo();
    assert_eq!(e.project().channel(CH).unwrap().src, ChannelSrc::Keys { keys: KeysState::default() });
}

#[test]
fn a_chord_is_the_plain_sum_of_its_notes() {
    // a limiter on the piano used to squash chords (pumping, the tone changing as keys were
    // added): every note must sound in the chord exactly as it sounds alone
    let keys: Vec<u8> = (58..66).collect();
    let chord = render(project(keys.iter().map(|&k| note(0.0, 8.0, k, 1.0)).collect(), KeysState::default()), true);
    let mut sum = vec![0f32; chord.len()];
    for &k in &keys {
        for (s, x) in sum.iter_mut().zip(render(project(vec![note(0.0, 8.0, k, 1.0)], KeysState::default()), true)) {
            *s += x;
        }
    }
    let err = chord.iter().zip(&sum).map(|(a, b)| (a - b).abs()).fold(0f32, f32::max);
    assert!(err < 1e-4, "chord differs from the sum of its notes by {err}");
}

#[test]
fn a_loud_note_leaves_headroom_for_chords() {
    let x = render(project(vec![note(0.0, 8.0, 60, 1.0)], KeysState::default()), true);
    let pk = x.iter().fold(0f32, |a, b| a.max(b.abs()));
    // the recording peaks at 0.5 (−6 dBFS, −8 dBFS after its velocity curve); the channel
    // plays it 12 dB lower
    let db = 20.0 * pk.log10();
    assert!(db < -19.0 && db > -21.0, "loud note peak {db:.1} dBFS");
}

#[test]
fn a_sequenced_note_plays_its_key_release_noise_when_it_ends() {
    let x = render_with(
        project(vec![note(0.0, 4.0, 60, 0.9)], KeysState { release: Some(40.0), ..Default::default() }),
        Some(bank_with_click()),
    );
    let end = 4 * STEP;
    let before = rms(&x[end - 2400..end - 400]);
    let after = rms(&x[end + 1200..end + 2200]);
    assert!(before > 0.01, "the note sounds");
    // 25 ms after the key: the tone is fading fast (40 ms release) but the click sounds, at its own pitch
    assert!(after > 0.003, "release noise {after} (note {before})");
    let f = freq(&x[end + 1600..end + 2350]);
    assert!((f - 1000.0).abs() < 100.0, "the click plays at its own pitch, got {f:.0} Hz");
    // the instrument without release regions plays nothing after the release
    let plain = render(project(vec![note(0.0, 4.0, 60, 0.9)], KeysState { release: Some(40.0), ..Default::default() }), true);
    assert!(rms(&plain[end + 4800..end + 6000]) < 1e-4);
}

#[test]
fn round_robins_alternate_on_repeated_notes() {
    let b = Arc::new(
        bank_from(
            "<group> seq_length=2 key=60 <region> sample=a seq_position=1 <region> sample=b seq_position=2",
            vec![("a", tone(261.63, 0.5)), ("b", tone(523.25, 0.5))],
        )
        .unwrap(),
    );
    let x = render_with(project((0..3).map(|i| note(i as f32 * 4.0, 4.0, 60, 0.9)).collect(), KeysState::default()), Some(b));
    let f = |i: usize| freq(&x[i * 4 * STEP + 1000..i * 4 * STEP + 20000]);
    let (a, b, c) = (f(0), f(1), f(2));
    assert!((a - 261.63).abs() < 5.0 && (b - 523.25).abs() < 8.0 && (c - 261.63).abs() < 5.0, "{a} {b} {c}");
}

#[test]
fn an_envelope_with_a_decay_lets_the_note_die_away() {
    let b = Arc::new(
        bank_from("<region> sample=a key=60 ampeg_attack=0.05 ampeg_decay=0.3 ampeg_sustain=0", vec![("a", tone(261.63, 0.5))]).unwrap(),
    );
    let x = render_with(project(vec![note(0.0, 16.0, 60, 1.0)], KeysState::default()), Some(b));
    // rising through the attack, loud after it, gone well before the note ends
    assert!(rms(&x[0..480]) < rms(&x[2400..4800]));
    assert!(rms(&x[2400..4800]) > 0.02);
    assert!(rms(&x[48000..60000]) < 1e-4);
}

#[test]
fn a_looped_region_sustains_past_its_recording() {
    let b = Arc::new(
        bank_from("<region> sample=a key=60 loop_mode=loop_continuous loop_start=0 loop_end=4799", vec![("a", tone_for(1000.0, 0.5, 0.1))])
            .unwrap(),
    );
    let x = render_with(project(vec![note(0.0, 16.0, 60, 1.0)], KeysState::default()), Some(b));
    // the recording is 0.1 s; the loop keeps it going for the whole note
    assert!(rms(&x[48000..60000]) > 0.02);
}

#[test]
fn a_slide_note_glides_a_keys_note() {
    // C4 held for 16 steps; a slide note to D#4 at step 4 lasting 4 steps
    let mut notes = vec![note(0.0, 16.0, 60, 0.9)];
    notes.push(Note { slide: true, ..note(4.0, 4.0, 63, 0.9) });
    let x = render(project(notes, KeysState::default()), true);
    let before = freq(&x[1000..4 * STEP]);
    let after = freq(&x[9 * STEP..15 * STEP]);
    assert!((before - 261.63).abs() < 4.0, "starts on C4, got {before:.1}");
    assert!((after - 311.13).abs() < 5.0, "glides to D#4, got {after:.1}");
}

#[test]
fn the_articulation_picks_its_regions_and_a_keyswitch_note_switches_live() {
    let mapping = "<global> sw_lokey=24 sw_hikey=25 sw_default=24
        <master> sw_last=24 sw_label=Arco <region> sample=arco lokey=48 hikey=72 pitch_keycenter=60
        <master> sw_last=25 sw_label=Pizzicato <region> sample=pizz lokey=48 hikey=72 pitch_keycenter=60";
    let b = || Arc::new(bank_from(mapping, vec![("arco", tone(261.63, 0.5)), ("pizz", tone(523.25, 0.5))]).unwrap());
    let f = |keys: KeysState, notes: Vec<Note>| {
        let x = render_with(project(notes, keys), Some(b()));
        (freq(&x[1000..3 * STEP]), x)
    };
    // the instrument's default, then the channel's choice
    assert!((f(KeysState::default(), vec![note(0.0, 4.0, 60, 0.9)]).0 - 261.63).abs() < 4.0);
    let pizz = KeysState { articulation: Some(25), ..Default::default() };
    assert!((f(pizz, vec![note(0.0, 4.0, 60, 0.9)]).0 - 523.25).abs() < 8.0);
    // a keyswitch note before the note switches it and makes no sound of its own
    let (_, x) = f(KeysState::default(), vec![note(0.0, 1.0, 25, 0.9), note(4.0, 4.0, 60, 0.9)]);
    assert!(rms(&x[0..3 * STEP]) < 1e-6, "the keyswitch is silent");
    let hz = freq(&x[4 * STEP + 1000..7 * STEP]);
    assert!((hz - 523.25).abs() < 8.0, "switched to pizzicato, got {hz:.1}");
}

#[test]
fn an_articulation_the_instrument_lacks_is_refused() {
    let mut p = project(vec![], KeysState::default());
    let ch = p.channels.iter().find(|c| c.id == CH).unwrap().clone();
    let keys = KeysState { articulation: Some(26), ..Default::default() };
    assert!(ops::apply(&mut p, &Op::PutChannel { channel: Channel { src: ChannelSrc::Keys { keys }, ..ch.clone() }, index: None }).is_err());
    let keys = KeysState { instrument: "strings".into(), articulation: Some(26), ..Default::default() };
    assert!(ops::apply(&mut p, &Op::PutChannel { channel: Channel { src: ChannelSrc::Keys { keys }, ..ch }, index: None }).is_ok());
    let names: Vec<&str> = wave_engine::keys::articulations("strings").iter().map(|a| a.name.as_str()).collect();
    assert_eq!(names, ["Sustain", "Tremolo", "Pizzicato", "Spiccato"]);
}

#[test]
fn auto_articulation_plays_short_notes_spiccato_and_long_ones_arco() {
    // the strings' auto rule: under 0.13 s the short articulation (key 27), else the long (24)
    let mapping = "<global> sw_lokey=24 sw_hikey=27 sw_default=24
        <master> sw_last=24 <region> sample=arco lokey=48 hikey=72 pitch_keycenter=60
        <master> sw_last=27 <region> sample=spic lokey=48 hikey=72 pitch_keycenter=60";
    let b = Arc::new(bank_from(mapping, vec![("arco", tone(261.63, 0.5)), ("spic", tone(523.25, 0.5))]).unwrap());
    let render_strings = |keys: KeysState, notes: Vec<Note>| {
        let e = Engine::new(project(notes, keys), Arc::new(NoPlugins));
        e.set_keys_bank("strings", Some(b.clone()));
        e.render(0.0, 4.0, 0.0, SR).unwrap().as_chunks::<2>().0.iter().map(|f| f[0]).collect::<Vec<f32>>()
    };
    let strings = KeysState { instrument: "strings".into(), ..Default::default() };
    // a sixteenth at 128 BPM is 0.117 s: short; four sixteenths are 0.47 s: long
    let short = render_strings(strings.clone(), vec![note(0.0, 1.0, 60, 0.9)]);
    let long = render_strings(strings.clone(), vec![note(0.0, 4.0, 60, 0.9)]);
    assert!((freq(&short[200..STEP - 200]) - 523.25).abs() < 15.0, "short note: spiccato");
    assert!((freq(&long[200..3 * STEP]) - 261.63).abs() < 5.0, "long note: arco");
    // a chosen articulation wins over the auto one
    let arco = KeysState { articulation: Some(24), ..strings };
    let x = render_strings(arco, vec![note(0.0, 1.0, 60, 0.9)]);
    assert!((freq(&x[200..STEP - 200]) - 261.63).abs() < 15.0);
}
