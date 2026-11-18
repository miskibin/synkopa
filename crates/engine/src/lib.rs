//! `wave-engine` — everything Synkopa does with sound, independent of any window or device.

pub mod autovocal;
pub mod catalog;
mod clipmix;
pub mod comp;
pub mod content;
pub mod demo;
pub mod factory;
pub mod melody;
pub mod midi;
pub mod model;
pub mod music;
pub mod note_track;
pub mod ops;
pub mod pitch;
pub mod plug;
pub mod poly;
pub mod pool;
pub mod processor;
pub mod reference;
pub mod rider;
pub mod routing;
pub mod singer;
pub mod transform;

pub use model::*;
pub use ops::{Dirty, Op, OpError};
pub use processor::{BandParams, FxData, InstEvent, InstMsg, Instrument, ParamBlock, ProcessCtx, Processor, MAX_INST_EVENTS, MAX_PARAMS};
pub mod analysis;
pub mod denoise;
pub mod dsp;
pub mod engine;
pub mod errors;
pub mod keys;
pub mod midi_file;
pub mod pitch_api;
mod project_zip;
pub mod relink;
pub mod rt;
pub mod sources;
pub mod synth;
pub mod telemetry;
pub mod tune_edit;
pub mod tune_render;
pub mod vocal;

pub use engine::{
    pattern_project, Engine, EngineError, InPlace, MadeUnique, NoPlugins, PluginFactory, Relink, RelinkSearch, Relinked, RenderProgress,
    RenderTail, StateDto, TransportDto,
};
pub use rt::Rt;
