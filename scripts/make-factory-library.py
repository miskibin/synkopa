#!/usr/bin/env python3
"""Builds the factory sample library that ships inside the app.

Every pick is a recording free to redistribute (CC0, or the Unlicense for the steel drum),
pinned: a file of a kit in assets-manifest.json (`<kit id>/<file>`), a path in one of the
repositories of SOURCES at a pinned commit (`vcsl:<path>` …), or a Freesound sound by its id
(`fs:<id>`, its public high-quality preview, see FREESOUND), whose SHA-256 is pinned under
`factory` in the same manifest. The script downloads what it needs
into a cache and refuses a file that does not match its pin. For every pick it trims
silence before the hit and the dead tail, fades the end, normalises the peak to -1 dBFS and
writes 16-bit FLAC. A loop (option `loop`) is instead cut to a whole number of bars at its
tempo, from its first sample and without fades, so it repeats in time, and stored as Ogg
Vorbis: the Freesound loops are lossy previews already, and FLAC would make them five times
bigger for nothing. A pick can also be a list of layers, each tuned to a key and mixed (the
orchestra hits). It also writes the table the engine embeds
(crates/engine/src/factory_files.rs) and the credits of the files by other authors than
Versilian and the TR-808 set (THIRD_PARTY_NOTICES.md, between the `factory-credits` marks).

    python3 scripts/make-factory-library.py            # rebuild
    python3 scripts/make-factory-library.py --pin      # after adding picks: record their hashes

A file whose audio did not change is left as it is, so a rebuild with another ffmpeg does not
rewrite the whole folder (an Ogg file is rewritten only when its source or options changed:
its tag names them). Needs ffmpeg (with libvorbis), curl and numpy.
"""
import argparse, hashlib, json, os, subprocess, sys, urllib.parse
import numpy as np

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
OUT = os.path.join(ROOT, "crates", "engine", "library")
MANIFEST = os.path.join(ROOT, "assets-manifest.json")
SR = 44100

# Repositories picks are taken from directly (the manifest's kits pin their own): repo, commit,
# licence, and for the credits who made the recordings (None: credited in the notices' text).
SOURCES = {
    "808": ("tidalcycles/sounds-tr808-fischer", "85fbecf1bec32553395625ea659e2a56dfd7c0e1", "CC0-1.0", None),
    "vcsl": ("sgossner/VCSL", "c1ea7bcc3c7309650ab0da9d15c9cd1fbc4a4c7e", "CC0-1.0", None),
    "vsco": ("sgossner/VSCO-2-CE", "440300901dfe9275fd84e0b7763af1f8443ae62e", "CC0-1.0", None),
    "stargate": ("stargatedaw/stargate-sample-pack", "dbfd6ec52d4ed53b60bdbea5fc6adf295127c027", "CC0-1.0", "Kevin B. (Fugue State Audio), Stargate DAW sample pack"),
    "mck": ("MckAudio/MckSamplePacks", "5db40e8fe26785c256845a5bb38921b2654a887f", "CC0-1.0", "MckAudio, recorded from a Roland TR-8"),
    "omi": ("Yale-OMI/sample-pack-vol-1", "9ad3da067dff97aa4bd8588a97ee0419b02c0acc", "CC0-1.0", "Yale Open Music Initiative, Sample Pack Vol. 1"),
    "steeldrum": ("sfzinstruments/jlearman.SteelDrum", "dc15a36ad69a43b0d240fcbd0cc78fbc61b2b47e", "Unlicense", "Jeff Learman, SteelDrum"),
}
LICENSE_URL = {"CC0-1.0": "https://creativecommons.org/publicdomain/zero/1.0/", "Unlicense": "https://unlicense.org/"}
# Freesound sounds released under CC0 (each one's page shows the licence): id -> (high-quality
# preview, uploader, title). The preview is public, the original needs a login; its SHA-256 is
# pinned in the manifest like any other source file.
FREESOUND = {
    "514880": ("https://cdn.freesound.org/previews/514/514880_3173681-hq.mp3", "tkky", "Sorrow State zay 808.wav"),
    "729532": ("https://cdn.freesound.org/previews/729/729532_15200476-hq.mp3", "James_KuKu", "XXX - C"),
    "501761": ("https://cdn.freesound.org/previews/501/501761_4600366-hq.mp3", "kbeezy88", "MPC_808_F1.aif"),
    "353396": ("https://cdn.freesound.org/previews/353/353396_6397561-hq.mp3", "TechGeekMusic", "Kick - Proper Bottom.wav"),
    "190942": ("https://cdn.freesound.org/previews/190/190942_2661752-hq.mp3", "Elektronikz", "808 Kick (wav format)"),
    "240893": ("https://cdn.freesound.org/previews/240/240893_4206277-hq.mp3", "OllieOllie", "trappy kick"),
    "517297": ("https://cdn.freesound.org/previews/517/517297_10137963-hq.mp3", "muffin3k", "Trap Snare 2"),
    "355065": ("https://cdn.freesound.org/previews/355/355065_1973065-hq.mp3", "eryps", "Clap Snare"),
    "75037": ("https://cdn.freesound.org/previews/75/75037_1151436-hq.mp3", "CBeeching", "Hat_04.wav"),
    "91791": ("https://cdn.freesound.org/previews/91/91791_1491361-hq.mp3", "zinzan_101", "short open.aif"),
    "816813": ("https://cdn.freesound.org/previews/816/816813_5511175-hq.mp3", "tboneaudio", "HellCat Hi Hats"),
    "132418": ("https://cdn.freesound.org/previews/132/132418_2412414-hq.mp3", "Sajmund", "Rimshot (sweet)"),
    "219266": ("https://cdn.freesound.org/previews/219/219266_170215-hq.mp3", "radiopassiveboy", "Tambourine_Single_Hit_1.wav"),
    "199823": ("https://cdn.freesound.org/previews/199/199823_3485902-hq.mp3", "connermusician", "Egg Shaker - 1 Throw"),
    "332604": ("https://cdn.freesound.org/previews/332/332604_167075-hq.mp3", "vckhaze", "Single Finger Snap"),
    "177242": ("https://cdn.freesound.org/previews/177/177242_3277771-hq.mp3", "a deleted account", "Cinematic Impact"),
    "428073": ("https://cdn.freesound.org/previews/428/428073_4067257-hq.mp3", "newagesoup", "Sub Drop Smooth.wav"),
    "561207": ("https://cdn.freesound.org/previews/561/561207_10825267-hq.mp3", "Rizzard", "Riser"),
    "240745": ("https://cdn.freesound.org/previews/240/240745_3215867-hq.mp3", "majoranman", "Choir One-Shot"),
    "574346": ("https://cdn.freesound.org/previews/574/574346_11532701-hq.mp3", "deadrobotmusic", "SOS Dark Piano Loop [148bpm] [C Minor]"),
    "428292": ("https://cdn.freesound.org/previews/428/428292_1117259-hq.mp3", "Djeuphoria", "DSV1_SuspensfulRhodes02_C#_70bpm.wav"),
    "663687": ("https://cdn.freesound.org/previews/663/663687_9959691-hq.mp3", "Orboram", "Soft Piano Loop 160BPM"),
    "491285": ("https://cdn.freesound.org/previews/491/491285_4882128-hq.mp3", "Xinematix", "Emotional Piano 005 (Am-80)"),
    "484645": ("https://cdn.freesound.org/previews/484/484645_10392137-hq.mp3", "Loop_Cult", "LCCPTL_140_Synth_Am_ThinPluck.wav"),
    "697731": ("https://cdn.freesound.org/previews/697/697731_9839964-hq.mp3", "harrisonlace", "guitar_loop_heatwave_129_F#min.wav"),
    "474961": ("https://cdn.freesound.org/previews/474/474961_10100847-hq.mp3", "skratan", "guitar [fmin 148 bpm].wav"),
    "521664": ("https://cdn.freesound.org/previews/521/521664_10327019-hq.mp3", "urmom123543", "Blade runner pads (Free)"),
    "713120": ("https://cdn.freesound.org/previews/713/713120_14632469-hq.mp3", "DYEKHO", "Arabic Flute 04 130BPM"),
    "852268": ("https://cdn.freesound.org/previews/852/852268_12574855-hq.mp3", "holizna", "Trap Melody Loop #5 Ebmin 165 BPM"),
    "646403": ("https://cdn.freesound.org/previews/646/646403_9839964-hq.mp3", "harrisonlace", "808_dalmation_C.wav"),
    "347136": ("https://cdn.freesound.org/previews/347/347136_369949-hq.mp3", "NovaSoundTechnology", "Custom Mini Lex Trap Snare - Nova Sound"),
    "647745": ("https://cdn.freesound.org/previews/647/647745_9839964-hq.mp3", "harrisonlace", "clap_caviar.wav"),
    "703686": ("https://cdn.freesound.org/previews/703/703686_11532701-hq.mp3", "deadrobotmusic", "Drk Vox Loop [120bpm] [D Minor]"),
}
I = "vcsl:Idiophones/Struck Idiophones/"
M = "vcsl:Membranophones/Struck Membranophones/"
VS = "vsco:Strings/"
VB = "vsco:Brass/"

# Layers of the orchestra hits: (source, pitch class it is tuned to, gain dB). The octave is
# the recording's own; the nearest key of that pitch class is used.
def hit(third):
    """Strings and brass on C and G over four octaves, timpani under them; `third` (3 = minor,
    4 = major, None = open fifths) is added in the middle."""
    L = [
        (VS + "Solo Contrabass/Spic/BKCtbss_Spic_C1_v3_rr1.wav", 0, -3),
        (VS + "Cello Section/spic/spic_C1_v2_RR1.wav", 0, -2),
        (VS + "Cello Section/spic/spic_G1_v2_RR1.wav", 7, -6),
        (VS + "Cello Section/spic/spic_C3_v2_RR1.wav", 0, -3),
        (VS + "Viola Section/spic/Violas_spic_G2_v2_rr1.wav", 7, -4),
        (VS + "Viola Section/spic/Violas_spic_C4_v2_rr1.wav", 0, -3),
        (VS + "Violin Section/Spic/VlnEns_Spic_G2_v2_rr1.wav", 7, -4),
        (VS + "Violin Section/Spic/VlnEns_Spic_C4_v2_rr1.wav", 0, -2),
        (VS + "Violin Section/Spic/VlnEns_Spic_G4_v2_rr1.wav", 7, -4),
        (VS + "Violin Section/Spic/VlnEns_Spic_D5_v2_rr1.wav", 0, -5),
        (VB + "Tuba/stac/Tuba3_stac_D2_v2_rr1_Sum.wav", 0, -5),
        (VB + "Tenor Trombone/stac/tenortbn_stac_D3_v4_rr1.wav", 0, -5),
        (VB + "Tenor Trombone/stac/tenortbn_stac_F2_v4_rr1.wav", 7, -7),
        (VB + "F Horn/stac/MOHorn_stac_C3_v3_rr1.wav", 0, -5),
        (VB + "F Horn/stac/MOHorn_stac_F2_v3_rr1.wav", 7, -7),
        (VB + "Trumpet/stac/Sum_SHTrumpet_stac_C3_v3_rr1.wav", 0, -5),
        (VB + "Trumpet/stac/Sum_SHTrumpet_stac_G3_v3_rr1.wav", 7, -7),
        (VB + "Trumpet/stac/Sum_SHTrumpet_stac_C5_v3_rr1.wav", 0, -8),
        ("vsco:Percussion/Timpani/Timpani2_Hit_v4_rr1_Sum.wav", 0, -6),
    ]
    if third is not None:
        L += [
            (VS + "Viola Section/spic/Violas_spic_E4_v2_rr1.wav", third, -4),
            (VS + "Violin Section/Spic/VlnEns_Spic_E4_v2_rr1.wav", third, -4),
            (VB + "F Horn/stac/MOHorn_stac_D4_v3_rr1.wav", third, -7),
            (VB + "Trumpet/stac/Sum_SHTrumpet_stac_D#3_v3_rr1.wav", third, -7),
        ]
    return L

STRINGS_STAB = [
    (VS + "Solo Contrabass/Spic/BKCtbss_Spic_C1_v3_rr1.wav", 0, -3),
    (VS + "Cello Section/spic/spic_C1_v2_RR1.wav", 0, 0),
    (VS + "Cello Section/spic/spic_C3_v2_RR1.wav", 0, -2),
    (VS + "Viola Section/spic/Violas_spic_C4_v2_rr1.wav", 0, -1),
    (VS + "Violin Section/Spic/VlnEns_Spic_C4_v2_rr1.wav", 0, 0),
    (VS + "Violin Section/Spic/VlnEns_Spic_D5_v2_rr1.wav", 0, -4),
]
BRASS_STAB = [
    (VB + "Tuba/stac/Tuba3_stac_D2_v2_rr1_Sum.wav", 0, -2),
    (VB + "Tenor Trombone/stac/tenortbn_stac_D3_v4_rr1.wav", 0, -1),
    (VB + "F Horn/stac/MOHorn_stac_C3_v3_rr1.wav", 0, -1),
    (VB + "Trumpet/stac/Sum_SHTrumpet_stac_C3_v3_rr1.wav", 0, 0),
    (VB + "Trumpet/stac/Sum_SHTrumpet_stac_C5_v3_rr1.wav", 0, -5),
]
PIZZ = [
    (VS + "Solo Contrabass/Pizz/BKCtbss_Pizz_C1_v3_rr1.wav", 0, -3),
    (VS + "Cello Section/pizzT/pizzT_C3_v2_RR1.wav", 0, 0),
    (VS + "Violin Section/Pizz/VlnEns_Pizz_C4_v2_rr1.wav", 0, -1),
]

# (output file, display name, group, source, max seconds, root key[, options])
# root key None = measured (`pitch`). Options: tune (resample so the note is exactly on the
# root key given), soft (keep a quiet start: swells), drive (dB into a tanh saturator),
# loop ((bpm, bars): a loop of that many bars, see the module comment; max seconds unused).
T = {"tune": True}
SOFT = {"soft": True}
SG = "stargate:stargate-sample-pack/fugue-state-audio/drums/"
TR8 = "mck:TR8/"


def loop(bpm, bars):
    return {"loop": (bpm, bars)}
PICKS = [
    ("kick-808.flac", "808 Kick", "Kicks", "808-kit/kick-tone50-decay50.wav", 1.2, 60),
    ("kick-808-short.flac", "808 Kick Short", "Kicks", "808-kit/kick-tone00-decay25.wav", 0.5, 60),
    ("kick-808-tight.flac", "808 Kick Tight", "Kicks", "808-kit/kick-tone50-decay00.wav", 0.25, 60),
    ("kick-808-bright.flac", "808 Kick Bright", "Kicks", "808-kit/kick-tone100-decay25.wav", 0.5, 60),
    ("kick-studio.flac", "Studio Kick", "Kicks", "studio-drums/kick-v4.wav", 0.9, 60),
    ("kick-studio-soft.flac", "Studio Kick Soft", "Kicks", "studio-drums/kick-v3.wav", 0.9, 60),
    ("kick-cajon.flac", "Cajon Kick", "Kicks", "percussion-fx/cajon-bass-v3.wav", 0.9, 60),
    ("kick-808-long.flac", "808 Kick Long", "Kicks", "808:bd8/BD5075.WAV", 1.5, 60),
    ("kick-808-boom.flac", "808 Kick Boom", "Kicks", "808:bd8/BD0010.WAV", 2.0, 60),
    ("kick-808-deep.flac", "808 Kick Deep", "Kicks", "808:bd8/BD0075.WAV", 1.5, 60),
    ("kick-808-punch.flac", "808 Kick Punch", "Kicks", "808:bd8/BD7550.WAV", 1.2, 60),
    ("kick-808-click.flac", "808 Kick Click", "Kicks", "808:bd8/BD1000.WAV", 0.25, 60),
    ("kick-trap.flac", "Trap Kick", "Kicks", "fs:353396", 0.8, 60),
    ("kick-trap-knock.flac", "Trap Kick Knock", "Kicks", "fs:190942", 1.0, 60),
    ("kick-trap-boom.flac", "Trap Kick Boom", "Kicks", "fs:240893", 1.6, 60),
    ("snare-808.flac", "808 Snare", "Snares", "808-kit/snare-tone50-snap75.wav", 0.5, 60),
    ("snare-808-bright.flac", "808 Snare Bright", "Snares", "808-kit/snare-tone100-snap75.wav", 0.5, 60),
    ("snare-808-dark.flac", "808 Snare Dark", "Snares", "808-kit/snare-tone00-snap75.wav", 0.5, 60),
    ("rimshot-808.flac", "808 Rimshot", "Snares", "808-kit/rimshot.wav", 0.25, 60),
    ("snare-studio.flac", "Studio Snare", "Snares", "studio-drums/snare-v4.wav", 1.2, 60),
    ("snare-studio-2.flac", "Studio Snare 2", "Snares", "studio-drums/snare2-v3.wav", 1.0, 60),
    ("sidestick-studio.flac", "Studio Side Stick", "Snares", "studio-drums/side-stick.wav", 0.6, 60),
    ("cajon-slap.flac", "Cajon Slap", "Snares", "percussion-fx/cajon-slap-v2.wav", 0.8, 60),
    ("snare-808-soft.flac", "808 Snare Soft", "Snares", "808:sd8/SD5000.WAV", 0.5, 60),
    ("snare-808-snappy.flac", "808 Snare Snappy", "Snares", "808:sd8/SD5010.WAV", 0.5, 60),
    ("snare-808-low.flac", "808 Snare Low", "Snares", "808:sd8/SD2550.WAV", 0.5, 60),
    ("snare-808-tight.flac", "808 Snare Tight", "Snares", "808:sd8/SD7525.WAV", 0.5, 60),
    ("snare-studio-3.flac", "Studio Snare 3", "Snares", M + "Snare Drum, Modern 3/Snare4_HitSN_v5_rr1_Mid.wav", 1.0, 60),
    ("snare-studio-off.flac", "Studio Snare Wires Off", "Snares", M + "Snare Drum, Modern 1/Snare2_HitNS_v6_rr1_Mid.wav", 1.0, 60),
    ("rimshot-studio.flac", "Studio Rimshot", "Snares", M + "Snare Drum, Modern 3/Snare4_rimshot_v4_rr1_Mid.wav", 1.0, 60),
    ("crossstick-studio.flac", "Studio Cross Stick", "Snares", M + "Snare Drum, Modern 3/Snare4_Xstick_v2_rr1_Mid.wav", 0.6, 60),
    ("snare-rope.flac", "Rope Snare", "Snares", M + "Snare Drum, Rope Tension/Hi/RopeSnare_hi_sn_Main_vl4_rr1.wav", 1.2, 60),
    ("snare-field.flac", "Field Drum", "Snares", M + "Snare Drum, Rope Tension/Low/RopeSnare_low_sn_Main_vl3_rr1.wav", 1.4, 60),
    ("snare-trap.flac", "Trap Snare", "Snares", "fs:517297", 0.5, 60),
    ("snare-trap-crisp.flac", "Trap Snare Crisp", "Snares", "fs:347136", 0.6, 60),
    ("snare-tr8.flac", "TR-8 Snare", "Snares", TR8 + "SD/001_808_Snare_Drum_1.wav", 0.6, 60),
    ("rimshot-tight.flac", "Rimshot Tight", "Snares", "fs:132418", 1.1, 60),
    ("clap-808.flac", "808 Clap", "Claps", "808-kit/clap.wav", 1.0, 60),
    ("clap-hand.flac", "Hand Clap", "Claps", "percussion-fx/clap-v4.wav", 0.45, 60),
    ("clap-hand-soft.flac", "Hand Clap Soft", "Claps", "percussion-fx/clap-v3.wav", 0.6, 60),
    ("clap-group.flac", "Group Clap", "Claps", I + "Claps/Clap_rr1.wav", 0.7, 60),
    ("clap-group-2.flac", "Group Clap 2", "Claps", I + "Claps/Clap_rr4.wav", 0.7, 60),
    ("whip.flac", "Whip Crack", "Claps", I + "Slapstick/slapstick_rr1.wav", 0.6, 60),
    ("clap-snare.flac", "Clap Snare", "Claps", "fs:355065", 0.7, 60),
    ("clap-trap-crisp.flac", "Trap Clap Crisp", "Claps", "fs:647745", 0.4, 60),
    ("clap-tr8.flac", "TR-8 Clap", "Claps", TR8 + "PERC/017_808_Hand_Clap.wav", 0.8, 60),
    ("finger-snap.flac", "Finger Snap", "Claps", "fs:332604", 0.4, 60),
    ("hat-808-closed.flac", "808 Closed Hat", "Hats", "808-kit/hat-closed.wav", 0.25, 60),
    ("hat-808-open.flac", "808 Open Hat", "Hats", "808-kit/hat-open-decay75.wav", 0.75, 60),
    ("hat-808-pedal.flac", "808 Pedal Hat", "Hats", "808-kit/hat-open-decay00.wav", 0.25, 60),
    ("hat-studio-closed.flac", "Studio Closed Hat", "Hats", "studio-drums/hat-closed-v4.wav", 0.5, 60),
    ("hat-studio-open.flac", "Studio Open Hat", "Hats", "studio-drums/hat-open.wav", 2.5, 60),
    ("hat-studio-pedal.flac", "Studio Pedal Hat", "Hats", "studio-drums/hat-pedal.wav", 0.5, 60),
    ("hat-808-open-short.flac", "808 Open Hat Short", "Hats", "808:oh8/OH25.WAV", 0.4, 60),
    ("hat-808-open-long.flac", "808 Open Hat Long", "Hats", "808:oh8/OH10.WAV", 1.5, 60),
    ("hat-studio-soft.flac", "Studio Closed Hat Soft", "Hats", I + "Hi-Hat Cymbal/HiHat_HitC_v2_rr1_Mid.wav", 0.5, 60),
    ("hat-studio-loose.flac", "Studio Loose Hat", "Hats", I + "Hi-Hat Cymbal/HiHat_HitLoose_rr1_Mid.wav", 1.2, 60),
    ("hat-studio-openclose.flac", "Studio Hat Open-Close", "Hats", I + "Hi-Hat Cymbal/HiHat_HitOC_rr5_Mid.wav", 1.2, 60),
    ("hat-trap-tick.flac", "Trap Hat Tick", "Hats", "fs:75037", 0.3, 60),
    ("hat-x0x.flac", "x0x Hat", "Hats", SG + "hihats/x0xproc2-hatclsd.wav", 0.2, 60),
    ("hat-tr8-closed.flac", "TR-8 Closed Hat", "Hats", TR8 + "HATS/004_808_Closed_HiHat_Short.wav", 0.3, 60),
    ("hat-tr8-open.flac", "TR-8 Open Hat", "Hats", TR8 + "HATS/001_808_Open_HiHat.wav", 1.0, 60),
    ("hat-open-choked.flac", "Open Hat Choked", "Hats", "fs:91791", 0.4, 60),
    ("hat-open-zildjian.flac", "Open Hat Zildjian", "Hats", "fs:816813", 1.6, 60),
    ("cymbal-808.flac", "808 Cymbal", "Cymbals", "808-kit/cymbal-tone50-decay50.wav", 2.5, 60),
    ("crash-studio.flac", "Studio Crash", "Cymbals", "studio-drums/crash-v3.wav", 5.0, 60),
    ("ride-studio.flac", "Studio Ride", "Cymbals", "studio-drums/ride-v3.wav", 3.0, 60),
    ("ride-bell-studio.flac", "Studio Ride Bell", "Cymbals", "studio-drums/ride-bell-v2.wav", 2.5, 60),
    ("cymbal-808-short.flac", "808 Cymbal Short", "Cymbals", "808:cy8/CY5000.WAV", 1.0, 60),
    ("cymbal-808-bright.flac", "808 Cymbal Bright", "Cymbals", "808:cy8/CY1050.WAV", 2.5, 60),
    ("cymbal-suspended.flac", "Suspended Cymbal", "Cymbals", I + "Suspended Cymbal 1/susCymb1_hit_fff1.wav", 4.0, 60),
    ("cymbal-bell.flac", "Cymbal Bell", "Cymbals", I + "Suspended Cymbal 1/susCymb1_hit_bell_fff1.wav", 3.0, 60),
    ("crash-choke.flac", "Crash Choke", "Cymbals", I + "Clash Cymbals 1/cymbal_crash1_short1.wav", 1.5, 60),
    ("finger-cymbals.flac", "Finger Cymbals", "Cymbals", I + "Finger Cymbals/Fing_Cymb.wav", 2.0, 60),
    ("tom-808-low.flac", "808 Low Tom", "Toms", "808-kit/tom-low.wav", 1.0, 60),
    ("tom-808-mid.flac", "808 Mid Tom", "Toms", "808-kit/tom-mid.wav", 1.0, 60),
    ("tom-808-high.flac", "808 High Tom", "Toms", "808-kit/tom-high.wav", 1.0, 60),
    ("tom-studio-low.flac", "Studio Low Tom", "Toms", "studio-drums/tom-low-v3.wav", 1.8, 60),
    ("tom-studio-high.flac", "Studio High Tom", "Toms", "studio-drums/tom-high-v3.wav", 1.6, 60),
    ("tom-808-deep.flac", "808 Low Tom Deep", "Toms", "808:lt8/LT00.WAV", 1.0, 60),
    ("tom-808-bright.flac", "808 High Tom Bright", "Toms", "808:ht8/HT10.WAV", 1.0, 60),
    ("tom-studio-rim.flac", "Studio Tom Rim", "Toms", M + "Tom 1/TomH_rimS_v4_rr1_Mid.wav", 1.2, 60),
    ("tom-studio-floor.flac", "Studio Floor Tom", "Toms", M + "Tom 2/Mallet/TomL_HitM_v4_rr1_Mid.wav", 1.8, 60),
    ("cowbell-808.flac", "808 Cowbell", "Percussion", "808-kit/cowbell.wav", 0.8, 60),
    ("conga-808-high.flac", "808 High Conga", "Percussion", "808-kit/conga-high.wav", 0.5, 60),
    ("conga-808-low.flac", "808 Low Conga", "Percussion", "808-kit/conga-low.wav", 0.5, 60),
    ("maracas-808.flac", "808 Maracas", "Percussion", "808-kit/maracas.wav", 0.25, 60),
    ("claves-808.flac", "808 Claves", "Percussion", "808-kit/claves.wav", 0.25, 60),
    ("shaker.flac", "Shaker", "Percussion", "percussion-fx/shaker-large.wav", 0.7, 60),
    ("tambourine.flac", "Tambourine", "Percussion", "percussion-fx/tambourine-v2.wav", 1.0, 60),
    ("bongo-high.flac", "High Bongo", "Percussion", "percussion-fx/bongo-high-v3.wav", 0.8, 60),
    ("bongo-low.flac", "Low Bongo", "Percussion", "percussion-fx/bongo-low-v3.wav", 0.9, 60),
    ("woodblock.flac", "Woodblock", "Percussion", "percussion-fx/woodblock-low-v2.wav", 0.5, 60),
    ("triangle.flac", "Triangle", "Percussion", "percussion-fx/triangle-open-v2.wav", 3.0, 60),
    ("cabasa.flac", "Cabasa", "Percussion", "percussion-fx/cabasa.wav", 0.6, 60),
    ("conga-808-mid.flac", "808 Mid Conga", "Percussion", "808:mc8/MC50.WAV", 0.5, 60),
    ("conga.flac", "Conga", "Percussion", M + "Conga/Conga_HitN_v3_rr1_Sum.wav", 1.0, 60),
    ("conga-quinto.flac", "Quinto", "Percussion", M + "Conga/Quinto_HitN_v3_rr1_Sum.wav", 1.0, 60),
    ("conga-tumba.flac", "Tumba", "Percussion", M + "Conga/Tumba_HitN_v4_rr1_Sum.wav", 1.2, 60),
    ("darbuka-dum.flac", "Darbuka Dum", "Percussion", M + "Darbuka/Darbuka_1_hit_vl2_rr1.wav", 1.0, 60),
    ("darbuka-tek.flac", "Darbuka Tek", "Percussion", M + "Darbuka/Darbuka_4_hit_vl2_rr1.wav", 0.8, 60),
    ("frame-drum.flac", "Frame Drum", "Percussion", M + "Frame Drum/HDrumL_Hit_v3_rr1_Sum.wav", 1.5, 60),
    ("frame-drum-muted.flac", "Frame Drum Muted", "Percussion", M + "Frame Drum/HDrumS_HitMuted_v3_rr1_Sum.wav", 0.8, 60),
    ("agogo-high.flac", "Agogo High", "Percussion", I + "Agogo Bells/Agogo_High_v3_rr1_Mid.wav", 1.0, 60),
    ("agogo-low.flac", "Agogo Low", "Percussion", I + "Agogo Bells/Agogo_Low_v2_rr1_Mid.wav", 1.0, 60),
    ("cowbell.flac", "Cowbell", "Percussion", I + "Cowbells/Cowbell1_Normal_v3_rr1_Mid.wav", 1.0, 60),
    ("cowbell-muted.flac", "Cowbell Muted", "Percussion", I + "Cowbells/Cowbell1_Muted_v3_rr1_Mid.wav", 0.5, 60),
    ("guiro.flac", "Guiro", "Percussion", I + "Guiro/Guiro_Med_rr1_Mid.wav", 1.0, 60),
    ("guiro-short.flac", "Guiro Short", "Percussion", I + "Guiro/Guiro_Hit_rr1_Mid.wav", 0.5, 60),
    ("vibraslap.flac", "Vibraslap", "Percussion", I + "Vibraslap/Legacy/vibraslap_rr1.wav", 1.5, 60),
    ("sleigh-bells.flac", "Sleigh Bells", "Percussion", I + "Sleigh Bells/Sleighbells_Hit_rr1_Mid.wav", 1.2, 60),
    ("shaker-small.flac", "Shaker Small", "Percussion", I + "Shaker, Small/Mid_ShakerHighFaster_Down_rr1.wav", 0.5, 60),
    ("log-drum-high.flac", "Log Drum High", "Percussion", I + "Slit Drum/LogDrumHi_MedM_v3_rr1_Sum.wav", 1.0, 60),
    ("log-drum-low.flac", "Log Drum Low", "Percussion", I + "Slit Drum/LogDrumLo_MedM_v3_rr1_Sum.wav", 1.0, 60),
    ("anvil.flac", "Anvil", "Percussion", I + "Anvil/Anvil_Hit1_v3_rr1_Mid.wav", 1.5, 60),
    ("brake-drum.flac", "Brake Drum", "Percussion", I + "Brake Drum/BrakeDrum1_Hammer_v3_rr1_Mid.wav", 2.0, 60),
    ("tambourine-hit.flac", "Tambourine Hit", "Percussion", "fs:219266", 1.2, 60),
    ("egg-shaker.flac", "Egg Shaker", "Percussion", "fs:199823", 0.5, 60),
    # tonal: the root key is measured (see `pitch`) so the piano plays them in tune
    ("bass-808.flac", "808 Bass", "Tonal", "808-bass/bass-tone100-decay100.wav", 3.0, None),
    ("bass-808-short.flac", "808 Bass Short", "Tonal", "808-bass/bass-tone75-decay75.wav", 1.2, None),
    ("marimba.flac", "Marimba", "Tonal", "marimba/marimba-C4-loud.wav", 2.5, 60),
    ("marimba-low.flac", "Marimba Low", "Tonal", "marimba/marimba-C2-loud.wav", 3.5, 36),
    ("glockenspiel.flac", "Glockenspiel", "Tonal", "glockenspiel/glockenspiel-C6-loud.wav", 3.0, 84),
    ("bass-808-deep.flac", "808 Bass Deep", "Tonal", "808:bd8/BD0010.WAV", 3.0, None),
    ("bass-808-mid.flac", "808 Bass Mid", "Tonal", "808:bd8/BD5010.WAV", 3.0, None),
    ("bass-808-punch.flac", "808 Bass Punch", "Tonal", "808:bd8/BD7575.WAV", 1.5, None),
    ("bass-808-drive.flac", "808 Bass Drive", "Tonal", "808-bass/bass-tone100-decay100.wav", 3.0, None, {"drive": 14}),
    ("vibraphone.flac", "Vibraphone", "Tonal", I + "Vibraphone/Hard Mallets/Vibes_hard_C3_v3_rr1_Main.wav", 4.0, 60, T),
    ("vibraphone-high.flac", "Vibraphone High", "Tonal", I + "Vibraphone/Hard Mallets/Vibes_hard_C5_v3_rr1_Main.wav", 3.0, 84, T),
    ("xylophone.flac", "Xylophone", "Tonal", I + "Xylophone/Hard Mallets/Xylo_Hard_C5_ff_01_far.wav", 1.5, 84, T),
    ("balafon.flac", "Balafon", "Tonal", I + "Balafon/Hard Mallet/EthnicXylo_hardM_C4_vl3_rr1_Mid.wav", 1.5, 72, T),
    ("kalimba.flac", "Kalimba", "Tonal", "vcsl:Idiophones/Plucked Idiophones/Kalimba, Kenya/Mbira6_Normal_MainSpirit_B3_k3_vl3_rr2.wav", 3.0, 71, T),
    ("hand-chime.flac", "Hand Chime", "Tonal", I + "Hand Chimes/sus_C5_r01_main.wav", 4.0, 84, T),
    ("harpsichord.flac", "Harpsichord", "Tonal", "vcsl:Chordophones/Zithers/Harpsichord, French/Sustains/Harpsi2_Normal_C3_rr1_Main.wav", 3.0, 60, T),
    ("upright-piano.flac", "Upright Piano", "Tonal", "vcsl:Chordophones/Zithers/Upright Piano, Yamaha/Sustains/Upright1_Sus_C3_vl3_rr1.wav", 4.0, 60, T),
    ("fm-piano.flac", "FM Piano", "Tonal", "vcsl:Electrophones/TX81Z/FM Piano/FMPiano_C3_vl3.wav", 3.0, 48, T),
    ("fm-clav.flac", "FM Clav", "Tonal", "vcsl:Electrophones/TX81Z/Clavisynth/Clavisynth_C2_vl3.wav", 2.0, 48, T),
    ("pipe-organ.flac", "Pipe Organ", "Tonal", "vcsl:Aerophones/Edge-blown Aerophones/Pipe Organ/Loud/Rode_Man3Open_C3.wav", 3.0, 60, T),
    ("tenor-sax.flac", "Tenor Sax Staccato", "Tonal", "vcsl:Aerophones/Reed Aerophones/Tenor Saxophone/Staccato/BrettTenor_Staccato_Main_C3_vl2_rr4.wav", 1.5, 60, T),
    ("bass-808-sorrow.flac", "808 Bass Sorrow", "Tonal", "fs:514880", 3.0, None),
    ("bass-808-sub.flac", "808 Bass Sub", "Tonal", "fs:729532", 2.5, None),
    ("bass-808-mpc.flac", "808 Bass MPC", "Tonal", "fs:501761", 1.0, None),
    ("bass-808-sine.flac", "808 Bass Sine", "Tonal", SG + "kicks/sdbkit-sub-a.wav", 2.0, None),
    ("bass-808-tr8.flac", "808 Bass TR-8", "Tonal", TR8 + "BD/008_808_Bass_Drum_Long_2.wav", 5.0, None),
    ("bass-808-spin.flac", "808 Bass Spin", "Tonal", "fs:646403", 2.0, None),
    ("choir-shot.flac", "Choir Shot", "Tonal", "fs:240745", 2.0, 68, T),
    ("steel-drum.flac", "Steel Drum", "Tonal", "steeldrum:flac/SteelDrum_072__C5_3-2.flac", 2.5, 72, T),
    # orchestra: single notes are tuned, the stabs and hits are layers on C
    ("orch-hit.flac", "Orchestra Hit", "Orchestra", hit(None), 2.5, 60),
    ("orch-hit-minor.flac", "Orchestra Hit Minor", "Orchestra", hit(3), 2.5, 60),
    ("orch-hit-major.flac", "Orchestra Hit Major", "Orchestra", hit(4), 2.5, 60),
    ("strings-stab.flac", "Strings Stab", "Orchestra", STRINGS_STAB, 2.0, 60),
    ("brass-stab.flac", "Brass Stab", "Orchestra", BRASS_STAB, 2.0, 60),
    ("pizzicato.flac", "Pizzicato", "Orchestra", PIZZ, 2.0, 60),
    ("strings-tremolo.flac", "Strings Tremolo", "Orchestra", VS + "Violin Section/Trem/VlnEns_Trem_C4_v2.wav", 3.0, 72, T),
    ("cello-spiccato.flac", "Cello Spiccato", "Orchestra", VS + "Cello Section/spic/spic_C3_v2_RR1.wav", 1.5, 60, T),
    ("contrabass-pizz.flac", "Contrabass Pizzicato", "Orchestra", VS + "Solo Contrabass/Pizz/BKCtbss_Pizz_C1_v3_rr1.wav", 2.5, 36, T),
    ("trumpet-staccato.flac", "Trumpet Staccato", "Orchestra", VB + "Trumpet/stac/Sum_SHTrumpet_stac_C3_v3_rr1.wav", 1.5, 60, T),
    ("horn-staccato.flac", "Horn Staccato", "Orchestra", VB + "F Horn/stac/MOHorn_stac_C3_v3_rr1.wav", 1.5, 60, T),
    ("flute-staccato.flac", "Flute Staccato", "Orchestra", "vsco:Woodwinds/Flute/stac/LDFlute_stac_C5_v3_rr1.wav", 1.5, 84, T),
    ("harp.flac", "Harp", "Orchestra", "vcsl:Chordophones/Composite Chordophones/Concert Harp/KSHarp_C3_f2.wav", 4.0, 48, T),
    ("harp-high.flac", "Harp High", "Orchestra", "vcsl:Chordophones/Composite Chordophones/Concert Harp/KSHarp_C5_f1.wav", 3.0, 72, T),
    ("tubular-bell.flac", "Tubular Bell", "Orchestra", I + "Tubular Bells 1/chimes_C4_ff_rr2.wav", 4.0, 60, {"pitched": True}),
    ("timpani-low.flac", "Timpani Low", "Orchestra", "vsco:Percussion/Timpani/Timpani2_Hit_v4_rr1_Sum.wav", 4.0, 47, T),
    ("timpani-high.flac", "Timpani High", "Orchestra", "vsco:Percussion/Timpani/Timpani4_Hit_v4_rr1_Sum.wav", 4.0, 49, T),
    ("concert-bass-drum.flac", "Concert Bass Drum", "Orchestra", M + "Bass Drum 1/BDrumNew_hit_v7_rr1_Sum.wav", 3.5, 60),
    ("orch-crash.flac", "Orchestral Crash", "Orchestra", I + "Clash Cymbals 1/cymbal_crash1_ff2.wav", 4.0, 60),
    ("gong.flac", "Gong", "Orchestra", I + "Gong 1/gong_fff.wav", 5.0, 60),
    # effects: swells keep their quiet start
    ("cymbal-swell.flac", "Cymbal Swell", "FX", I + "Suspended Cymbal 1/susCymb1_cresc_2s.wav", 5.0, 60, SOFT),
    ("cymbal-swell-long.flac", "Cymbal Swell Long", "FX", I + "Suspended Cymbal 1/susCymb1_cresc_4s.wav", 7.0, 60, SOFT),
    ("bass-drum-swell.flac", "Bass Drum Swell", "FX", M + "Bass Drum 2/bassdrum_cresc_short.wav", 4.0, 60, SOFT),
    ("timpani-roll.flac", "Timpani Roll", "FX", "vsco:Percussion/Timpani/Rolls/Timpani2_Roll_v5_rr1_Sum.wav", 3.0, 60, SOFT),
    ("snare-roll.flac", "Snare Roll", "FX", M + "Snare Drum, Modern 1/Snare2_rollSN_v5_rr2_Mid.wav", 2.5, 60, SOFT),
    ("wind-chimes-up.flac", "Wind Chimes Up", "FX", I + "Mark Trees/Legacy/windchimes_asc1.wav", 4.0, 60, SOFT),
    ("bell-tree.flac", "Bell Tree", "FX", I + "Bell Tree/Stroke/BellTree_Stroke_1_Mid.wav", 3.0, 60, SOFT),
    ("zap.flac", "Zap", "FX", "vsco:Percussion/zap1_v1.wav", 1.5, 60),
    ("cinematic-impact.flac", "Cinematic Impact", "FX", "fs:177242", 8.0, 60),
    ("sub-drop.flac", "Sub Drop", "FX", "fs:428073", 4.0, 60),
    ("downlifter-dark.flac", "Dark Downlifter", "FX", "omi:OMI_FX/OMI_fx_downlifter_melodic.wav", 6.0, 60),
    ("riser-drone.flac", "Drone Riser", "FX", "fs:561207", 2.5, 60, SOFT),
    # loops: tempo and key are in the name (the library has no fields for them)
    ("loop-keys-sinister-148-cmin.ogg", "Keys Sinister 148 Cm", "Loops", "fs:574346", 0, 60, loop(148, 16)),
    ("loop-rhodes-suspense-70-csharpmin.ogg", "Rhodes Suspense 70 C#m", "Loops", "fs:428292", 0, 60, loop(70, 4)),
    ("loop-rhodes-dream-160.ogg", "Rhodes Dream 160", "Loops", "fs:663687", 0, 60, loop(160, 8)),
    ("loop-piano-emotional-80-amin.ogg", "Piano Emotional 80 Am", "Loops", "fs:491285", 0, 60, loop(80, 4)),
    ("loop-piano-arp-165-ebmin.ogg", "Piano Arp 165 Ebm", "Loops", "fs:852268", 0, 60, loop(165, 16)),
    ("loop-pluck-eerie-140-amin.ogg", "Pluck Eerie 140 Am", "Loops", "fs:484645", 0, 60, loop(140, 4)),
    ("loop-guitar-acoustic-129-fsharpmin.ogg", "Guitar Acoustic 129 F#m", "Loops", "fs:697731", 0, 60, loop(129, 4)),
    ("loop-guitar-nylon-148-fmin.ogg", "Guitar Nylon 148 Fm", "Loops", "fs:474961", 0, 60, loop(148, 8)),
    ("loop-pad-cinematic-110-ebmin.ogg", "Pad Cinematic 110 Ebm", "Loops", "fs:521664", 0, 60, loop(110, 4)),
    ("loop-flute-eastern-130-cmin.ogg", "Flute Eastern 130 Cm", "Loops", "fs:713120", 0, 60, loop(130, 8)),
    ("loop-vox-dark-120-dmin.ogg", "Vox Dark 120 Dm", "Loops", "fs:703686", 0, 60, loop(120, 4)),
]


def manifest():
    with open(MANIFEST) as f:
        return json.load(f)


class Fetch:
    """Source files by pick name, downloaded into the cache and checked against their pins."""

    def __init__(self, cache, pin):
        self.cache, self.pin, self.m = cache, pin, manifest()
        self.kits = {k["id"]: {s["file"]: s for s in k["samples"]} for k in self.m["instruments"]}
        self.pins = {(e["source"], e["path"]): e for e in self.m.get("factory", {}).get("files", [])}
        self.used = {}

    def path(self, src):
        if ":" in src:
            key, rel = src.split(":", 1)
            if key == "fs":
                url = FREESOUND[rel][0]
            else:
                repo, commit = SOURCES[key][:2]
                url = f"https://raw.githubusercontent.com/{repo}/{commit}/{urllib.parse.quote(rel)}"
            e = self.pins.get((key, rel))
            if e is None and not self.pin:
                sys.exit(f"{src} is not pinned in assets-manifest.json; run with --pin")
            want = e and e["sha256"]
        else:
            key, rel = src.split("/", 1)
            s = self.kits[key][rel]
            url, want = s["url"], s["sha256"]
        dst = os.path.join(self.cache, key, rel)
        if not os.path.isfile(dst):
            os.makedirs(os.path.dirname(dst), exist_ok=True)
            subprocess.run(["curl", "-sSfL", "--retry", "4", "-o", dst + ".part", url], check=True)
            os.replace(dst + ".part", dst)
        with open(dst, "rb") as f:
            data = f.read()
        got = hashlib.sha256(data).hexdigest()
        if want and got != want:
            sys.exit(f"{src}: checksum mismatch (have {got}, pinned {want})")
        if ":" in src:
            self.used[(key, rel)] = {"source": key, "path": rel, "sha256": got, "bytes": len(data)}
        return dst

    def write_pins(self):
        """The `factory` section: the repositories and exactly the files the picks use."""
        self.m["factory"] = {
            "sources": {
                **{k: {"repo": f"https://github.com/{r}", "commit": c, "license": lic} for k, (r, c, lic, _) in SOURCES.items()},
                "fs": {"site": "https://freesound.org", "license": "CC0-1.0", "files": "high-quality previews of CC0 sounds"},
            },
            "files": [self.used[k] for k in sorted(self.used)],
        }
        with open(MANIFEST, "w", newline="\n") as f:
            json.dump(self.m, f, indent=2)
            f.write("\n")


def read(path, rate=SR):
    raw = subprocess.run(["ffmpeg", "-v", "error", "-i", path, "-f", "f32le", "-ac", "2", "-ar", str(rate), "-"], capture_output=True, check=True).stdout
    return np.frombuffer(raw, dtype=np.float32).reshape(-1, 2).astype(np.float64)


def pitch(x):
    """MIDI key (float) of the fundamental, by autocorrelation of the sustained part."""
    m = x.mean(axis=1)
    a = m[int(0.08 * SR):int(0.08 * SR) + 16384]
    a = a - a.mean()
    ac = np.correlate(a, a, "full")[len(a) - 1:]
    lo, hi = int(SR / 400), int(SR / 25)
    lag = lo + int(np.argmax(ac[lo:hi]))
    # parabolic refinement
    y0, y1, y2 = ac[lag - 1], ac[lag], ac[lag + 1]
    lag += 0.5 * (y0 - y2) / (y0 - 2 * y1 + y2)
    return 69 + 12 * np.log2(SR / lag / 440)


def pitch_wide(x):
    """Like `pitch` for notes up to C7 and short ones: the first strong autocorrelation peak
    after the hit, so a multiple of the period is not taken for it."""
    m = x.mean(axis=1)
    on = int(np.argmax(np.abs(m) > np.abs(m).max() * 0.1))
    a = m[on + int(0.03 * SR):on + int(0.03 * SR) + 16384]
    a = a - a.mean()
    n = len(a)
    f = np.fft.rfft(a, 2 * n)
    ac = np.fft.irfft(f * np.conj(f))[:n]
    ac = ac / (ac[0] * (1 - np.arange(n) / n) + 1e-12)  # unbiased, 1 at lag 0
    lo, hi = int(SR / 2200), min(int(SR / 28), n // 2)
    seg = ac[lo:hi]
    peaks = [i for i in range(1, len(seg) - 1) if seg[i] > seg[i - 1] and seg[i] >= seg[i + 1]]
    best = max(seg[i] for i in peaks)
    lag = lo + next(i for i in peaks if seg[i] >= 0.9 * best)
    y0, y1, y2 = ac[lag - 1], ac[lag], ac[lag + 1]
    lag += 0.5 * (y0 - y2) / (y0 - 2 * y1 + y2)
    return 69 + 12 * np.log2(SR / lag / 440)


def key_near(x, expected):
    """MIDI key (float) of the partial within 1.2 semitones of `expected`: exact where
    autocorrelation is misled by loud overtones (mallets, timpani)."""
    m = x.mean(axis=1)
    on = int(np.argmax(np.abs(m) > np.abs(m).max() * 0.1))
    a = m[on + int(0.03 * SR):on + int(0.03 * SR) + 32768]
    n = 1 << 18
    mag = np.abs(np.fft.rfft(a * np.hanning(len(a)), n))
    hz = lambda k: 440 * 2 ** ((k - 69) / 12)
    lo, hi = int(hz(expected - 1.2) * n / SR), int(hz(expected + 1.2) * n / SR) + 1
    i = lo + int(np.argmax(mag[lo:hi]))
    if i in (lo, hi - 1):
        sys.exit(f"no partial near key {expected}")
    # a section is several players a few cents apart: the centre of the energy around the peak
    peak = 69 + 12 * np.log2(i * SR / n / 440)
    j = np.arange(int(hz(peak - 0.6) * n / SR), int(hz(peak + 0.6) * n / SR) + 1)
    keys = 69 + 12 * np.log2(j * SR / n / 440)
    return float((keys * mag[j] ** 2).sum() / (mag[j] ** 2).sum())


def retuned(path, x, semis):
    """The file `semis` semitones higher: decoded at another rate and played at ours."""
    return x if abs(semis) < 0.03 else read(path, int(round(SR / 2 ** (semis / 12))))


def layers(fetch, spec):
    """Mix of layers, each moved to the nearest key of its pitch class and lined up on its hit."""
    parts = []
    for src, pc, db in spec:
        p = fetch.path(src)
        x = read(p)
        key = key_near(x, round(pitch_wide(x)))
        target = pc + 12 * round((key - pc) / 12)
        if abs(target - key) > 3.2:
            sys.exit(f"{src}: measured key {key:.2f} is too far from pitch class {pc}")
        x = retuned(p, x, target - key)
        env = np.abs(x).max(axis=1)
        x = x[max(0, int(np.argmax(env > env.max() * 10 ** (-30 / 20))) - int(0.002 * SR)):]
        parts.append(x / np.abs(x).max() * 10 ** (db / 20))
        print(f"    layer {os.path.basename(src):44} {key:6.2f} -> {target}")
    mix = np.zeros((max(len(p) for p in parts), 2))
    for p in parts:
        mix[:len(p)] += p
    # partials of many instruments meet on every C, each a few cents off: centre the sum
    off, power = zip(*(centre(mix, c) for c in (48, 60, 72)))
    semis = -float(np.dot(off, power) / sum(power))
    print(f"    mix moved {semis * 100:+.0f} cents")
    rate = int(round(SR * 2 ** (semis / 12)))
    raw = subprocess.run(["ffmpeg", "-v", "error", "-f", "f32le", "-ar", str(rate), "-ac", "2", "-i", "-", "-f", "f32le", "-ar", str(SR), "-"], input=mix.astype("<f4").tobytes(), capture_output=True, check=True).stdout
    return np.frombuffer(raw, dtype=np.float32).reshape(-1, 2).astype(np.float64)


def centre(x, key):
    """(offset in semitones, power) of the energy within 0.7 semitones of `key` in the first second."""
    a = x.mean(axis=1)[:SR]
    n = 1 << 18
    mag = np.abs(np.fft.rfft(a * np.hanning(len(a)), n)) ** 2
    hz = lambda k: 440 * 2 ** ((k - 69) / 12)
    j = np.arange(int(hz(key - 0.7) * n / SR), int(hz(key + 0.7) * n / SR) + 1)
    keys = 69 + 12 * np.log2(j * SR / n / 440)
    return float((keys * mag[j]).sum() / mag[j].sum()) - key, float(mag[j].sum())


def prepare(x, max_s, soft=False):
    env = np.abs(x).max(axis=1)
    peak = env.max()
    # start half a millisecond before the hit (a swell: before its first audible sample)
    first = int(np.argmax(env > peak * 10 ** ((-60 if soft else -40) / 20)))
    start = max(0, first - int(0.0005 * SR))
    x = x[start:]
    env = env[start:]
    # drop the tail once it stays 60 dB under the peak, and cap the length
    loud = np.nonzero(env > peak * 1e-3)[0]
    end = min(len(x), int(loud[-1]) + int(0.02 * SR), int(max_s * SR))
    x = x[:end].copy()
    fade = max(int(0.02 * SR), int(0.15 * len(x))) if end < len(env) else int(0.01 * SR)
    fade = min(fade, len(x))
    x[-fade:] *= (np.cos(np.linspace(0, np.pi, fade)) * 0.5 + 0.5)[:, None]
    lead = min(int((0.005 if soft else 0.0003) * SR), len(x))
    if start > 0:
        x[:lead] *= np.linspace(0, 1, lead)[:, None]
    x *= 10 ** (-1 / 20) / np.abs(x).max()
    mono = np.abs(x[:, 0] - x[:, 1]).max() < 1e-4
    return (x[:, :1] if mono else x), mono


def decoded(path):
    return subprocess.run(["ffmpeg", "-v", "error", "-i", path, "-f", "s16le", "-"], capture_output=True, check=True).stdout


def cut_loop(x, bpm, bars):
    """The first `bars` bars of 4/4 at `bpm`, unfaded: the source starts on the downbeat (the
    decoder drops the MP3 encoder's padding), so the cut repeats in time."""
    n = int(round(bars * 4 * 60 / bpm * SR))
    if len(x) < n - int(0.01 * SR):
        sys.exit(f"loop is {len(x) / SR:.3f} s, {bars} bars at {bpm} BPM need {n / SR:.3f} s")
    x = np.concatenate([x, np.zeros((max(0, n - len(x)), 2))])[:n].copy()
    return x * (10 ** (-1 / 20) / np.abs(x).max())


def write_ogg(dst, y, mono, tag):
    """Ogg Vorbis (quality 6, about 190 kbit/s stereo) peaking at -1 dBFS once decoded: lossy
    coding moves the peak, so the gain is corrected from a decode and the file written again.
    `tag` (stored as a comment) names the source and cut, so an unchanged file is kept."""
    if os.path.isfile(dst):
        have = subprocess.run(["ffprobe", "-v", "error", "-show_entries", "stream_tags=wave_src", "-of", "csv=p=0", dst], capture_output=True, text=True).stdout.strip()
        if have == tag:
            return False
    ch = "1" if mono else "2"
    best = None
    for _ in range(5):
        pcm = np.clip(y, -1, 1).astype("<f4").tobytes()
        subprocess.run(["ffmpeg", "-v", "error", "-y", "-f", "f32le", "-ar", str(SR), "-ac", ch, "-i", "-", "-c:a", "libvorbis", "-q:a", "6",
                        "-map_metadata", "-1", "-metadata:s:a", f"WAVE_SRC={tag}", "-bitexact", dst], input=pcm, check=True)
        dec = subprocess.run(["ffmpeg", "-v", "error", "-i", dst, "-f", "f32le", "-"], capture_output=True, check=True).stdout
        peak = float(np.abs(np.frombuffer(dec, dtype="<f4")).max())
        err = 20 * np.log10(peak) + 1
        with open(dst, "rb") as f:
            data = f.read()
        if best is None or abs(err) < best[0]:
            best = (abs(err), data)
        if abs(err) < 0.03:
            break
        y = y * 10 ** (-0.5 * err / 20)
    with open(dst, "wb") as f:
        f.write(best[1])
    return True


def credits(picks):
    """Markdown list of the files by other authors than the notices' text names."""
    lines = []
    for out, name, group, src, *_ in picks:
        if isinstance(src, list) or ":" not in src:
            continue
        key, rel = src.split(":", 1)
        if key == "fs":
            url, who, title = FREESOUND[rel]
            page = f"https://freesound.org/s/{rel}/"
            lines.append(f"* `{out}`: \"{title}\" by {who} on [Freesound]({page}), "
                         f"[CC0-1.0]({LICENSE_URL['CC0-1.0']}) (from its high-quality preview)")
        elif SOURCES[key][3]:
            repo, commit, lic, who = SOURCES[key]
            lines.append(f"* `{out}`: `{rel.rsplit('/', 1)[-1]}` by {who} "
                         f"([source](https://github.com/{repo}/blob/{commit}/{urllib.parse.quote(rel)})), [{lic}]({LICENSE_URL[lic]})")
    return "\n".join(lines)


def write_credits(picks):
    path = os.path.join(ROOT, "THIRD_PARTY_NOTICES.md")
    with open(path, encoding="utf-8") as f:
        text = f.read()
    a, b = "<!-- factory-credits:begin -->", "<!-- factory-credits:end -->"
    head, rest = text.split(a, 1)
    _, tail = rest.split(b, 1)
    with open(path, "w", encoding="utf-8", newline="\n") as f:
        f.write(f"{head}{a}\n{credits(picks)}\n{b}{tail}")


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--cache", default=os.path.join(ROOT, "target", "factory-sources"), help="where downloaded source files are kept")
    ap.add_argument("--pin", action="store_true", help="record the hashes of the source files in assets-manifest.json")
    a = ap.parse_args()
    fetch = Fetch(a.cache, a.pin)
    os.makedirs(OUT, exist_ok=True)
    assert len({p[0] for p in PICKS}) == len(PICKS), "two picks write the same file"
    rows, total, kept = [], 0, 0
    for out, name, group, src, max_s, root, *rest in PICKS:
        opt = rest[0] if rest else {}
        pitched = group == "Tonal" or isinstance(src, list) or bool(opt.get("tune") or opt.get("pitched"))
        if isinstance(src, list):
            x = layers(fetch, src)
        else:
            path = fetch.path(src)
            x = read(path)
            if opt.get("tune"):
                x = retuned(path, x, root - key_near(x, root))
            elif root is None:
                root = int(round(pitch(x)))
        if "drive" in opt:
            x = np.tanh(x / np.abs(x).max() * 10 ** (opt["drive"] / 20))
        dst = os.path.join(OUT, out)
        if "loop" in opt:
            assert out.endswith(".ogg"), out
            y = cut_loop(x, *opt["loop"])
            mono = np.abs(y[:, 0] - y[:, 1]).max() < 1e-4
            y = y[:, :1] if mono else y
            if not write_ogg(dst, y, mono, f"{src} {opt['loop'][0]} {opt['loop'][1]}"):
                kept += 1
            total += os.path.getsize(dst)
            rows.append((out, name, group, root, str(pitched).lower(), len(y) / SR, 1 if mono else 2))
            print(f"{out:28} {len(y) / SR:5.2f} s  {'mono' if mono else 'stereo':6} loop       {os.path.getsize(dst) // 1024:4} K")
            continue
        y, mono = prepare(x, max_s, opt.get("soft", False))
        pcm = (np.clip(y, -1, 1) * 32767).round().astype("<i2").tobytes()
        if os.path.isfile(dst) and decoded(dst) == pcm:
            kept += 1
        else:
            subprocess.run(["ffmpeg", "-v", "error", "-y", "-f", "s16le", "-ar", str(SR), "-ac", "1" if mono else "2", "-i", "-", "-compression_level", "8", "-bitexact", "-map_metadata", "-1", dst], input=pcm, check=True)
        total += os.path.getsize(dst)
        rows.append((out, name, group, root, str(pitched).lower(), len(y) / SR, 1 if mono else 2))
        print(f"{out:28} {len(y) / SR:5.2f} s  {'mono' if mono else 'stereo':6} root {root:3} {os.path.getsize(dst) // 1024:4} K")
    for f in set(os.listdir(OUT)) - {r[0] for r in rows}:
        os.remove(os.path.join(OUT, f))
    if a.pin:
        fetch.write_pins()
    write_credits(PICKS)
    with open(os.path.join(ROOT, "crates", "engine", "src", "factory_files.rs"), "w", newline="\n") as f:
        f.write("// Generated by scripts/make-factory-library.py; do not edit by hand.\n\n")
        f.write("use super::FactoryFile;\n\n")
        f.write("pub(super) const FILES: &[FactoryFile] = &[\n")
        for out, name, group, root, pitched, dur, ch in rows:
            f.write(f'    FactoryFile {{ file: "{out}", name: "{name}", group: "{group}", root: {root}, pitched: {pitched}, dur: {dur:.4f}, ch: {ch}, bytes: include_bytes!("../library/{out}") }},\n')
        f.write("];\n")
    print(f"{len(rows)} files ({kept} unchanged), {total / 1e6:.2f} MB")


if __name__ == "__main__":
    sys.exit(main())
