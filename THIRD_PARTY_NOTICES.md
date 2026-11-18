# Third-party notices

Synkopa is licensed AGPL-3.0-only (see [`LICENSE`](LICENSE)). It links or bundles the following components whose licences differ.

## LAME (MP3 encoding)

MP3 export uses the [`mp3lame-encoder`](https://crates.io/crates/mp3lame-encoder) Rust bindings
(LGPL-3.0) over [`mp3lame-sys`](https://crates.io/crates/mp3lame-sys), which vendors the
[LAME](https://lame.sourceforge.io/) MP3 encoder source and is built and linked statically.

LAME is Copyright (c) 1999-2017 The LAME Project and earlier authors, and is distributed under the
GNU Lesser General Public License, version 2.0 (<https://www.gnu.org/licenses/old-licenses/lgpl-2.0.html>).
The complete LAME source is available at <https://lame.sourceforge.io/> and, in the exact version
used, in the `mp3lame-sys` crate on crates.io.

LGPL relinking: Synkopa is open source and ships its full source, so you may rebuild the application
against a modified LAME (`cargo build` with a patched `mp3lame-sys`). Patents on MP3 have expired.
This notice is not legal advice; check the licence terms before redistributing a binary.

## Standard library samples

The standard library that ships inside Synkopa (`crates/engine/library`) is made from
recordings released to the public domain under
[CC0-1.0](https://creativecommons.org/publicdomain/zero/1.0/), except one under the
[Unlicense](https://unlicense.org/) (listed below):

* files named `*-808*`: a Roland TR-808 sampled by Michael Fischer (Technopolis), 1994, via
  the [TidalCycles sound library](https://github.com/tidalcycles/sounds-tr808-fischer);
* the orchestra hits and stabs, the string, brass, flute and timpani notes and `zap.flac`:
  [VSCO 2 Community Edition](https://github.com/sgossner/VSCO-2-CE) by Versilian Studios LLC
  (the hits and stabs are mixes of several of its recordings);
* the files listed below: the authors and sources named there (Freesound uploads, the
  Stargate DAW sample pack, MckAudio's TR-8 recordings, the Yale Open Music Initiative sample
  pack and Jeff Learman's SteelDrum);
* all other files: the
  [Versilian Community Sample Library](https://github.com/sgossner/VCSL) by Versilian Studios LLC.

The files are edited (silence trimmed, tails faded, peaks normalised, pitched ones tuned,
`bass-808-drive.flac` saturated, re-encoded as FLAC) by
`scripts/make-factory-library.py`; the exact source of each is pinned in `assets-manifest.json`.
CC0 asks for no credit; it is given here anyway.

The loops (`loop-*.ogg`, tempo and key in their names) and the other Freesound files are made
from Freesound's public high-quality MP3 previews and stored as Ogg Vorbis (loops) or FLAC.

<!-- factory-credits:begin -->
* `kick-trap.flac`: "Kick - Proper Bottom.wav" by TechGeekMusic on [Freesound](https://freesound.org/s/353396/), [CC0-1.0](https://creativecommons.org/publicdomain/zero/1.0/) (from its high-quality preview)
* `kick-trap-knock.flac`: "808 Kick (wav format)" by Elektronikz on [Freesound](https://freesound.org/s/190942/), [CC0-1.0](https://creativecommons.org/publicdomain/zero/1.0/) (from its high-quality preview)
* `kick-trap-boom.flac`: "trappy kick" by OllieOllie on [Freesound](https://freesound.org/s/240893/), [CC0-1.0](https://creativecommons.org/publicdomain/zero/1.0/) (from its high-quality preview)
* `kick-trap-rack.flac`: "Kick (19).wav" by Rippleys_Circus on [Freesound](https://freesound.org/s/542893/), [CC0-1.0](https://creativecommons.org/publicdomain/zero/1.0/) (from its high-quality preview)
* `kick-trap-knock-2.flac`: "Trap Kick 2" by muffin3k on [Freesound](https://freesound.org/s/508909/), [CC0-1.0](https://creativecommons.org/publicdomain/zero/1.0/) (from its high-quality preview)
* `kick-trap-clip.flac`: "Kick (4).wav" by Rippleys_Circus on [Freesound](https://freesound.org/s/542899/), [CC0-1.0](https://creativecommons.org/publicdomain/zero/1.0/) (from its high-quality preview)
* `kick-trap-hard.flac`: "Gucci Kick.wav" by Nilzner on [Freesound](https://freesound.org/s/421108/), [CC0-1.0](https://creativecommons.org/publicdomain/zero/1.0/) (from its high-quality preview)
* `kick-trap-punch.flac`: "JH Trap Kick (F#).wav" by Johnnie_Holiday on [Freesound](https://freesound.org/s/666823/), [CC0-1.0](https://creativecommons.org/publicdomain/zero/1.0/) (from its high-quality preview)
* `kick-trap-top.flac`: "Trap Kick 1" by muffin3k on [Freesound](https://freesound.org/s/501553/), [CC0-1.0](https://creativecommons.org/publicdomain/zero/1.0/) (from its high-quality preview)
* `kick-trap-phonk.flac`: "Phonk Kick 4" by CAT-FOX_ALEX on [Freesound](https://freesound.org/s/832660/), [CC0-1.0](https://creativecommons.org/publicdomain/zero/1.0/) (from its high-quality preview)
* `kick-trap-tight.flac`: "Kick (11).wav" by Rippleys_Circus on [Freesound](https://freesound.org/s/542877/), [CC0-1.0](https://creativecommons.org/publicdomain/zero/1.0/) (from its high-quality preview)
* `kick-trap-bounce.flac`: "Short Kick" by nobody0101 on [Freesound](https://freesound.org/s/717631/), [CC0-1.0](https://creativecommons.org/publicdomain/zero/1.0/) (from its high-quality preview)
* `kick-trap-stomp.flac`: "JH Kick 6 (F 100 bpm).wav" by Johnnie_Holiday on [Freesound](https://freesound.org/s/691738/), [CC0-1.0](https://creativecommons.org/publicdomain/zero/1.0/) (from its high-quality preview)
* `kick-trap-clean.flac`: "Rnb Kick 5" by CAT-FOX_ALEX on [Freesound](https://freesound.org/s/866443/), [CC0-1.0](https://creativecommons.org/publicdomain/zero/1.0/) (from its high-quality preview)
* `kick-trap-brick.flac`: "Phonk Kick 1" by CAT-FOX_ALEX on [Freesound](https://freesound.org/s/836743/), [CC0-1.0](https://creativecommons.org/publicdomain/zero/1.0/) (from its high-quality preview)
* `kick-trap-heavy.flac`: "kick_uppercut.wav" by harrisonlace on [Freesound](https://freesound.org/s/704245/), [CC0-1.0](https://creativecommons.org/publicdomain/zero/1.0/) (from its high-quality preview)
* `kick-trap-dark.flac`: "Kick (7).wav" by Rippleys_Circus on [Freesound](https://freesound.org/s/542889/), [CC0-1.0](https://creativecommons.org/publicdomain/zero/1.0/) (from its high-quality preview)
* `snare-trap.flac`: "Trap Snare 2" by muffin3k on [Freesound](https://freesound.org/s/517297/), [CC0-1.0](https://creativecommons.org/publicdomain/zero/1.0/) (from its high-quality preview)
* `snare-trap-crisp.flac`: "Custom Mini Lex Trap Snare - Nova Sound" by NovaSoundTechnology on [Freesound](https://freesound.org/s/347136/), [CC0-1.0](https://creativecommons.org/publicdomain/zero/1.0/) (from its high-quality preview)
* `snare-tr8.flac`: `001_808_Snare_Drum_1.wav` by MckAudio, recorded from a Roland TR-8 ([source](https://github.com/MckAudio/MckSamplePacks/blob/5db40e8fe26785c256845a5bb38921b2654a887f/TR8/SD/001_808_Snare_Drum_1.wav)), [CC0-1.0](https://creativecommons.org/publicdomain/zero/1.0/)
* `rimshot-tight.flac`: "Rimshot (sweet)" by Sajmund on [Freesound](https://freesound.org/s/132418/), [CC0-1.0](https://creativecommons.org/publicdomain/zero/1.0/) (from its high-quality preview)
* `snare-trap-punch.flac`: "Punchy Snare" by WillOlis on [Freesound](https://freesound.org/s/240434/), [CC0-1.0](https://creativecommons.org/publicdomain/zero/1.0/) (from its high-quality preview)
* `snare-trap-tight.flac`: "Trap Snare 1" by nbmusic098 on [Freesound](https://freesound.org/s/399747/), [CC0-1.0](https://creativecommons.org/publicdomain/zero/1.0/) (from its high-quality preview)
* `snare-trap-hard.flac`: "Punchy Snare 4 (Small Tail).wav" by Musictart on [Freesound](https://freesound.org/s/169950/), [CC0-1.0](https://creativecommons.org/publicdomain/zero/1.0/) (from its high-quality preview)
* `snare-trap-sharp.flac`: "Snare (10).wav" by Rippleys_Circus on [Freesound](https://freesound.org/s/542850/), [CC0-1.0](https://creativecommons.org/publicdomain/zero/1.0/) (from its high-quality preview)
* `snare-trap-classic.flac`: "Fly on the Wall Snare" by adh.dreaming on [Freesound](https://freesound.org/s/632704/), [CC0-1.0](https://creativecommons.org/publicdomain/zero/1.0/) (from its high-quality preview)
* `snare-trap-layered.flac`: "Snare (1).wav" by Rippleys_Circus on [Freesound](https://freesound.org/s/542851/), [CC0-1.0](https://creativecommons.org/publicdomain/zero/1.0/) (from its high-quality preview)
* `snare-trap-clean.flac`: "Snare (21).wav" by Rippleys_Circus on [Freesound](https://freesound.org/s/542866/), [CC0-1.0](https://creativecommons.org/publicdomain/zero/1.0/) (from its high-quality preview)
* `snare-trap-smack.flac`: "Snare (22).wav" by Rippleys_Circus on [Freesound](https://freesound.org/s/542873/), [CC0-1.0](https://creativecommons.org/publicdomain/zero/1.0/) (from its high-quality preview)
* `snare-trap-clap.flac`: "DR Snare 058" by deadrobotmusic on [Freesound](https://freesound.org/s/577235/), [CC0-1.0](https://creativecommons.org/publicdomain/zero/1.0/) (from its high-quality preview)
* `snare-trap-round.flac`: "DR Snare 112" by deadrobotmusic on [Freesound](https://freesound.org/s/577207/), [CC0-1.0](https://creativecommons.org/publicdomain/zero/1.0/) (from its high-quality preview)
* `snare-trap-snap.flac`: "DR Snare 102" by deadrobotmusic on [Freesound](https://freesound.org/s/577226/), [CC0-1.0](https://creativecommons.org/publicdomain/zero/1.0/) (from its high-quality preview)
* `snare-trap-dirty.flac`: "Dirty Snare.wav" by alexthegr81 on [Freesound](https://freesound.org/s/320870/), [CC0-1.0](https://creativecommons.org/publicdomain/zero/1.0/) (from its high-quality preview)
* `snare-trap-rage.flac`: "CRDN_UpgrSn_009.wav" by choomaque-crispydinner on [Freesound](https://freesound.org/s/252743/), [CC0-1.0](https://creativecommons.org/publicdomain/zero/1.0/) (from its high-quality preview)
* `snare-trap-low.flac`: "Low Snare Hit" by eryps on [Freesound](https://freesound.org/s/355068/), [CC0-1.0](https://creativecommons.org/publicdomain/zero/1.0/) (from its high-quality preview)
* `clap-snare.flac`: "Clap Snare" by eryps on [Freesound](https://freesound.org/s/355065/), [CC0-1.0](https://creativecommons.org/publicdomain/zero/1.0/) (from its high-quality preview)
* `clap-trap-crisp.flac`: "clap_caviar.wav" by harrisonlace on [Freesound](https://freesound.org/s/647745/), [CC0-1.0](https://creativecommons.org/publicdomain/zero/1.0/) (from its high-quality preview)
* `clap-tr8.flac`: `017_808_Hand_Clap.wav` by MckAudio, recorded from a Roland TR-8 ([source](https://github.com/MckAudio/MckSamplePacks/blob/5db40e8fe26785c256845a5bb38921b2654a887f/TR8/PERC/017_808_Hand_Clap.wav)), [CC0-1.0](https://creativecommons.org/publicdomain/zero/1.0/)
* `clap-trap-punch.flac`: "Diamond_Clap_05.wav" by Stumber on [Freesound](https://freesound.org/s/199266/), [CC0-1.0](https://creativecommons.org/publicdomain/zero/1.0/) (from its high-quality preview)
* `clap-trap-warm.flac`: "Clap  (7).wav" by Rippleys_Circus on [Freesound](https://freesound.org/s/542834/), [CC0-1.0](https://creativecommons.org/publicdomain/zero/1.0/) (from its high-quality preview)
* `clap-trap-thick.flac`: "Anu Clap 01.wav" by pomeroyjoshua on [Freesound](https://freesound.org/s/513415/), [CC0-1.0](https://creativecommons.org/publicdomain/zero/1.0/) (from its high-quality preview)
* `clap-trap-layered.flac`: "Clap  (9).wav" by Rippleys_Circus on [Freesound](https://freesound.org/s/542837/), [CC0-1.0](https://creativecommons.org/publicdomain/zero/1.0/) (from its high-quality preview)
* `clap-trap-tight.flac`: "Snap Layered 05.wav" by JoshuaPomeroy on [Freesound](https://freesound.org/s/532248/), [CC0-1.0](https://creativecommons.org/publicdomain/zero/1.0/) (from its high-quality preview)
* `clap-trap-bright.flac`: "Clap  (2).wav" by Rippleys_Circus on [Freesound](https://freesound.org/s/542831/), [CC0-1.0](https://creativecommons.org/publicdomain/zero/1.0/) (from its high-quality preview)
* `clap-trap-hard.flac`: "Clap 1" by CAT-FOX_ALEX on [Freesound](https://freesound.org/s/828843/), [CC0-1.0](https://creativecommons.org/publicdomain/zero/1.0/) (from its high-quality preview)
* `clap-trap-short.flac`: "Anu Clap 09.wav" by pomeroyjoshua on [Freesound](https://freesound.org/s/513421/), [CC0-1.0](https://creativecommons.org/publicdomain/zero/1.0/) (from its high-quality preview)
* `clap-trap-gritty.flac`: "Anu Clap 03.wav" by pomeroyjoshua on [Freesound](https://freesound.org/s/513413/), [CC0-1.0](https://creativecommons.org/publicdomain/zero/1.0/) (from its high-quality preview)
* `clap-trap-sharp.flac`: "Anu Clap 05.wav" by pomeroyjoshua on [Freesound](https://freesound.org/s/513419/), [CC0-1.0](https://creativecommons.org/publicdomain/zero/1.0/) (from its high-quality preview)
* `clap-trap-click.flac`: "Snap Layered 08.wav" by JoshuaPomeroy on [Freesound](https://freesound.org/s/532245/), [CC0-1.0](https://creativecommons.org/publicdomain/zero/1.0/) (from its high-quality preview)
* `clap-trap-clean.flac`: "Snap Layered 07.wav" by JoshuaPomeroy on [Freesound](https://freesound.org/s/532246/), [CC0-1.0](https://creativecommons.org/publicdomain/zero/1.0/) (from its high-quality preview)
* `finger-snap.flac`: "Single Finger Snap" by vckhaze on [Freesound](https://freesound.org/s/332604/), [CC0-1.0](https://creativecommons.org/publicdomain/zero/1.0/) (from its high-quality preview)
* `hat-trap-tick.flac`: "Hat_04.wav" by CBeeching on [Freesound](https://freesound.org/s/75037/), [CC0-1.0](https://creativecommons.org/publicdomain/zero/1.0/) (from its high-quality preview)
* `hat-x0x.flac`: `x0xproc2-hatclsd.wav` by Kevin B. (Fugue State Audio), Stargate DAW sample pack ([source](https://github.com/stargatedaw/stargate-sample-pack/blob/dbfd6ec52d4ed53b60bdbea5fc6adf295127c027/stargate-sample-pack/fugue-state-audio/drums/hihats/x0xproc2-hatclsd.wav)), [CC0-1.0](https://creativecommons.org/publicdomain/zero/1.0/)
* `hat-tr8-closed.flac`: `004_808_Closed_HiHat_Short.wav` by MckAudio, recorded from a Roland TR-8 ([source](https://github.com/MckAudio/MckSamplePacks/blob/5db40e8fe26785c256845a5bb38921b2654a887f/TR8/HATS/004_808_Closed_HiHat_Short.wav)), [CC0-1.0](https://creativecommons.org/publicdomain/zero/1.0/)
* `hat-tr8-open.flac`: `001_808_Open_HiHat.wav` by MckAudio, recorded from a Roland TR-8 ([source](https://github.com/MckAudio/MckSamplePacks/blob/5db40e8fe26785c256845a5bb38921b2654a887f/TR8/HATS/001_808_Open_HiHat.wav)), [CC0-1.0](https://creativecommons.org/publicdomain/zero/1.0/)
* `hat-open-choked.flac`: "short open.aif" by zinzan_101 on [Freesound](https://freesound.org/s/91791/), [CC0-1.0](https://creativecommons.org/publicdomain/zero/1.0/) (from its high-quality preview)
* `hat-open-zildjian.flac`: "HellCat Hi Hats" by tboneaudio on [Freesound](https://freesound.org/s/816813/), [CC0-1.0](https://creativecommons.org/publicdomain/zero/1.0/) (from its high-quality preview)
* `hat-trap-punch.flac`: "505 CLOSEDHH.wav" by kbeezy88 on [Freesound](https://freesound.org/s/506452/), [CC0-1.0](https://creativecommons.org/publicdomain/zero/1.0/) (from its high-quality preview)
* `hat-trap-crisp.flac`: "Hi-Hat Closed 10.mp3" by blakengouda on [Freesound](https://freesound.org/s/509965/), [CC0-1.0](https://creativecommons.org/publicdomain/zero/1.0/) (from its high-quality preview)
* `hat-trap-sharp.flac`: "Hi-Hat Closed 6.mp3" by blakengouda on [Freesound](https://freesound.org/s/509968/), [CC0-1.0](https://creativecommons.org/publicdomain/zero/1.0/) (from its high-quality preview)
* `hat-trap-bright.flac`: "Hi-Hat Closed 9.mp3" by blakengouda on [Freesound](https://freesound.org/s/509971/), [CC0-1.0](https://creativecommons.org/publicdomain/zero/1.0/) (from its high-quality preview)
* `hat-trap-tight.flac`: "Hihat 8.wav" by Rippleys_Circus on [Freesound](https://freesound.org/s/542802/), [CC0-1.0](https://creativecommons.org/publicdomain/zero/1.0/) (from its high-quality preview)
* `hat-trap-snap.flac`: "Hihat 2.wav" by Rippleys_Circus on [Freesound](https://freesound.org/s/542796/), [CC0-1.0](https://creativecommons.org/publicdomain/zero/1.0/) (from its high-quality preview)
* `hat-trap-cut.flac`: "HH Cl 02.wav" by pomeroyjoshua on [Freesound](https://freesound.org/s/513363/), [CC0-1.0](https://creativecommons.org/publicdomain/zero/1.0/) (from its high-quality preview)
* `hat-trap-classic.flac`: "HH Cl 06.wav" by pomeroyjoshua on [Freesound](https://freesound.org/s/513367/), [CC0-1.0](https://creativecommons.org/publicdomain/zero/1.0/) (from its high-quality preview)
* `hat-trap-metal.flac`: "DR Hi Hat 049" by deadrobotmusic on [Freesound](https://freesound.org/s/588131/), [CC0-1.0](https://creativecommons.org/publicdomain/zero/1.0/) (from its high-quality preview)
* `hat-trap-open-smooth.flac`: "Hi-Hat Open 1.mp3" by blakengouda on [Freesound](https://freesound.org/s/509984/), [CC0-1.0](https://creativecommons.org/publicdomain/zero/1.0/) (from its high-quality preview)
* `hat-trap-open-bright.flac`: "Hat Open.wav" by PopoMagoo on [Freesound](https://freesound.org/s/612837/), [CC0-1.0](https://creativecommons.org/publicdomain/zero/1.0/) (from its high-quality preview)
* `hat-trap-open-short.flac`: "HH Op 03.wav" by pomeroyjoshua on [Freesound](https://freesound.org/s/513383/), [CC0-1.0](https://creativecommons.org/publicdomain/zero/1.0/) (from its high-quality preview)
* `tambourine-hit.flac`: "Tambourine_Single_Hit_1.wav" by radiopassiveboy on [Freesound](https://freesound.org/s/219266/), [CC0-1.0](https://creativecommons.org/publicdomain/zero/1.0/) (from its high-quality preview)
* `egg-shaker.flac`: "Egg Shaker - 1 Throw" by connermusician on [Freesound](https://freesound.org/s/199823/), [CC0-1.0](https://creativecommons.org/publicdomain/zero/1.0/) (from its high-quality preview)
* `bass-808-zay.flac`: "Rough And Tumble - C" by James_KuKu on [Freesound](https://freesound.org/s/729513/), [CC0-1.0](https://creativecommons.org/publicdomain/zero/1.0/) (from its high-quality preview)
* `bass-808-oak.flac`: "Oak - C" by James_KuKu on [Freesound](https://freesound.org/s/729507/), [CC0-1.0](https://creativecommons.org/publicdomain/zero/1.0/) (from its high-quality preview)
* `bass-808-opium.flac`: "Poster - C" by James_KuKu on [Freesound](https://freesound.org/s/729512/), [CC0-1.0](https://creativecommons.org/publicdomain/zero/1.0/) (from its high-quality preview)
* `bass-808-mafia.flac`: "Outer Edge - C" by James_KuKu on [Freesound](https://freesound.org/s/729508/), [CC0-1.0](https://creativecommons.org/publicdomain/zero/1.0/) (from its high-quality preview)
* `bass-808-clip.flac`: "Chill Blaster - C" by James_KuKu on [Freesound](https://freesound.org/s/729468/), [CC0-1.0](https://creativecommons.org/publicdomain/zero/1.0/) (from its high-quality preview)
* `bass-808-metro.flac`: "Heartless - C" by James_KuKu on [Freesound](https://freesound.org/s/729491/), [CC0-1.0](https://creativecommons.org/publicdomain/zero/1.0/) (from its high-quality preview)
* `bass-808-pierre.flac`: "Box - C" by James_KuKu on [Freesound](https://freesound.org/s/729464/), [CC0-1.0](https://creativecommons.org/publicdomain/zero/1.0/) (from its high-quality preview)
* `bass-808-roar.flac`: "Gucci Gang - C" by James_KuKu on [Freesound](https://freesound.org/s/729490/), [CC0-1.0](https://creativecommons.org/publicdomain/zero/1.0/) (from its high-quality preview)
* `bass-808-rage.flac`: "Fort Night - C" by James_KuKu on [Freesound](https://freesound.org/s/729484/), [CC0-1.0](https://creativecommons.org/publicdomain/zero/1.0/) (from its high-quality preview)
* `bass-808-drill.flac`: "Drill 808 bass (JH).wav" by Johnnie_Holiday on [Freesound](https://freesound.org/s/679775/), [CC0-1.0](https://creativecommons.org/publicdomain/zero/1.0/) (from its high-quality preview)
* `bass-808-ctm.flac`: "JH 808 2 C (1 sec).wav" by Johnnie_Holiday on [Freesound](https://freesound.org/s/679780/), [CC0-1.0](https://creativecommons.org/publicdomain/zero/1.0/) (from its high-quality preview)
* `bass-808-cuff.flac`: "Cuff 808 [1lovewav]" by 1LOVE on [Freesound](https://freesound.org/s/635285/), [CC0-1.0](https://creativecommons.org/publicdomain/zero/1.0/) (from its high-quality preview)
* `bass-808-pure.flac`: "JH 808 3 C (2 sec).wav" by Johnnie_Holiday on [Freesound](https://freesound.org/s/679781/), [CC0-1.0](https://creativecommons.org/publicdomain/zero/1.0/) (from its high-quality preview)
* `bass-808-plugg.flac`: "JH 808 1 C (0.5 sec).wav" by Johnnie_Holiday on [Freesound](https://freesound.org/s/679779/), [CC0-1.0](https://creativecommons.org/publicdomain/zero/1.0/) (from its high-quality preview)
* `bass-808-tube.flac`: "Drop - C" by James_KuKu on [Freesound](https://freesound.org/s/729480/), [CC0-1.0](https://creativecommons.org/publicdomain/zero/1.0/) (from its high-quality preview)
* `bass-808-sorrow.flac`: "Sorrow State zay 808.wav" by tkky on [Freesound](https://freesound.org/s/514880/), [CC0-1.0](https://creativecommons.org/publicdomain/zero/1.0/) (from its high-quality preview)
* `bass-808-sub.flac`: "XXX - C" by James_KuKu on [Freesound](https://freesound.org/s/729532/), [CC0-1.0](https://creativecommons.org/publicdomain/zero/1.0/) (from its high-quality preview)
* `bass-808-mpc.flac`: "MPC_808_F1.aif" by kbeezy88 on [Freesound](https://freesound.org/s/501761/), [CC0-1.0](https://creativecommons.org/publicdomain/zero/1.0/) (from its high-quality preview)
* `bass-808-sine.flac`: `sdbkit-sub-a.wav` by Kevin B. (Fugue State Audio), Stargate DAW sample pack ([source](https://github.com/stargatedaw/stargate-sample-pack/blob/dbfd6ec52d4ed53b60bdbea5fc6adf295127c027/stargate-sample-pack/fugue-state-audio/drums/kicks/sdbkit-sub-a.wav)), [CC0-1.0](https://creativecommons.org/publicdomain/zero/1.0/)
* `bass-808-tr8.flac`: `008_808_Bass_Drum_Long_2.wav` by MckAudio, recorded from a Roland TR-8 ([source](https://github.com/MckAudio/MckSamplePacks/blob/5db40e8fe26785c256845a5bb38921b2654a887f/TR8/BD/008_808_Bass_Drum_Long_2.wav)), [CC0-1.0](https://creativecommons.org/publicdomain/zero/1.0/)
* `bass-808-spin.flac`: "808_dalmation_C.wav" by harrisonlace on [Freesound](https://freesound.org/s/646403/), [CC0-1.0](https://creativecommons.org/publicdomain/zero/1.0/) (from its high-quality preview)
* `choir-shot.flac`: "Choir One-Shot" by majoranman on [Freesound](https://freesound.org/s/240745/), [CC0-1.0](https://creativecommons.org/publicdomain/zero/1.0/) (from its high-quality preview)
* `steel-drum.flac`: `SteelDrum_072__C5_3-2.flac` by Jeff Learman, SteelDrum ([source](https://github.com/sfzinstruments/jlearman.SteelDrum/blob/dc15a36ad69a43b0d240fcbd0cc78fbc61b2b47e/flac/SteelDrum_072__C5_3-2.flac)), [Unlicense](https://unlicense.org/)
* `cinematic-impact.flac`: "Cinematic Impact" by a deleted account on [Freesound](https://freesound.org/s/177242/), [CC0-1.0](https://creativecommons.org/publicdomain/zero/1.0/) (from its high-quality preview)
* `sub-drop.flac`: "Sub Drop Smooth.wav" by newagesoup on [Freesound](https://freesound.org/s/428073/), [CC0-1.0](https://creativecommons.org/publicdomain/zero/1.0/) (from its high-quality preview)
* `downlifter-dark.flac`: `OMI_fx_downlifter_melodic.wav` by Yale Open Music Initiative, Sample Pack Vol. 1 ([source](https://github.com/Yale-OMI/sample-pack-vol-1/blob/9ad3da067dff97aa4bd8588a97ee0419b02c0acc/OMI_FX/OMI_fx_downlifter_melodic.wav)), [CC0-1.0](https://creativecommons.org/publicdomain/zero/1.0/)
* `riser-drone.flac`: "Riser" by Rizzard on [Freesound](https://freesound.org/s/561207/), [CC0-1.0](https://creativecommons.org/publicdomain/zero/1.0/) (from its high-quality preview)
* `loop-keys-sinister-148-cmin.ogg`: "SOS Dark Piano Loop [148bpm] [C Minor]" by deadrobotmusic on [Freesound](https://freesound.org/s/574346/), [CC0-1.0](https://creativecommons.org/publicdomain/zero/1.0/) (from its high-quality preview)
* `loop-rhodes-suspense-70-csharpmin.ogg`: "DSV1_SuspensfulRhodes02_C#_70bpm.wav" by Djeuphoria on [Freesound](https://freesound.org/s/428292/), [CC0-1.0](https://creativecommons.org/publicdomain/zero/1.0/) (from its high-quality preview)
* `loop-rhodes-dream-160.ogg`: "Soft Piano Loop 160BPM" by Orboram on [Freesound](https://freesound.org/s/663687/), [CC0-1.0](https://creativecommons.org/publicdomain/zero/1.0/) (from its high-quality preview)
* `loop-piano-emotional-80-amin.ogg`: "Emotional Piano 005 (Am-80)" by Xinematix on [Freesound](https://freesound.org/s/491285/), [CC0-1.0](https://creativecommons.org/publicdomain/zero/1.0/) (from its high-quality preview)
* `loop-piano-arp-165-ebmin.ogg`: "Trap Melody Loop #5 Ebmin 165 BPM" by holizna on [Freesound](https://freesound.org/s/852268/), [CC0-1.0](https://creativecommons.org/publicdomain/zero/1.0/) (from its high-quality preview)
* `loop-pluck-eerie-140-amin.ogg`: "LCCPTL_140_Synth_Am_ThinPluck.wav" by Loop_Cult on [Freesound](https://freesound.org/s/484645/), [CC0-1.0](https://creativecommons.org/publicdomain/zero/1.0/) (from its high-quality preview)
* `loop-guitar-acoustic-129-fsharpmin.ogg`: "guitar_loop_heatwave_129_F#min.wav" by harrisonlace on [Freesound](https://freesound.org/s/697731/), [CC0-1.0](https://creativecommons.org/publicdomain/zero/1.0/) (from its high-quality preview)
* `loop-guitar-nylon-148-fmin.ogg`: "guitar [fmin 148 bpm].wav" by skratan on [Freesound](https://freesound.org/s/474961/), [CC0-1.0](https://creativecommons.org/publicdomain/zero/1.0/) (from its high-quality preview)
* `loop-pad-cinematic-110-ebmin.ogg`: "Blade runner pads (Free)" by urmom123543 on [Freesound](https://freesound.org/s/521664/), [CC0-1.0](https://creativecommons.org/publicdomain/zero/1.0/) (from its high-quality preview)
* `loop-flute-eastern-130-cmin.ogg`: "Arabic Flute 04 130BPM" by DYEKHO on [Freesound](https://freesound.org/s/713120/), [CC0-1.0](https://creativecommons.org/publicdomain/zero/1.0/) (from its high-quality preview)
* `loop-vox-dark-120-dmin.ogg`: "Drk Vox Loop [120bpm] [D Minor]" by deadrobotmusic on [Freesound](https://freesound.org/s/703686/), [CC0-1.0](https://creativecommons.org/publicdomain/zero/1.0/) (from its high-quality preview)
<!-- factory-credits:end -->

## Salamander Grand Piano (Synkopa Keys)

The Synkopa Keys piano plays recordings from the
[Salamander Grand Piano V3](https://github.com/sfzinstruments/SalamanderGrandPiano) by
Alexander Holm, licensed [CC-BY-3.0](https://creativecommons.org/licenses/by/3.0/). Installers
ship 90 of its recordings, modified: each is cut to 9 seconds with a fade-out (the longest a
note rings in Synkopa) and stored as 16-bit FLAC (see `docs/keys.md`). The in-app download installs
the original files unmodified.
Music made with Synkopa Keys contains these recordings: credit them as the licence asks.
