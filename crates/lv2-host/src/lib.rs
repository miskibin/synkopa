//! `wave-lv2` -- LV2 plugins as stereo insert effects for the WAVE engine (Linux).
//!
//! A hosted LV2 plugin is split like a hosted VST3 one:
//!
//! - an [`Lv2Effect`] (returned as `Box<dyn wave_engine::Processor>`) that the audio
//!   thread runs -- no locks, no allocation, no system calls;
//! - an [`Lv2Handle`] (`Send + Sync`, cheap clone) the app keeps per effect for the
//!   parameters, the state and the reported latency.
//!
//! ```ignore
//! pub fn status() -> Result<(), String>;                       // why LV2 is unavailable, if it is
//! pub fn scan(rescan: bool) -> Result<Vec<PluginInfo>, String>; // effects, with reasons for unusable ones
//! pub fn instantiate(uri: &str, state: Option<&[u8]>, sample_rate: f32, max_block: usize)
//!     -> Result<(Box<dyn wave_engine::Processor>, Lv2Handle), String>;
//! impl Lv2Handle { info, params, set_param, get_state, take_state_changed, latency,
//!                   has_editor, editor_unavailable, is_editor_open, open_editor, close_editor }
//! ```
//!
//! # The plugin's own interface
//!
//! An X11 UI (`ui:X11UI`) opens in a WAVE plugin window, under WAVE's header (`ui`):
//! control ports both ways, `ui:resize`, `ui:idleInterface`. GTK/Qt UIs are not embedded.
//!
//! # What the host provides
//!
//! `urid:map`/`unmap`, `options:options` (sample rate, min/max/nominal block length),
//! `buf-size:boundedBlockLength`, `worker:schedule` (a worker thread per instance, fed
//! through lock-free rings), `state:mapPath`/`makePath`/`freePath`, `log:log`. Inputs
//! and outputs always have separate buffers, so `lv2:inPlaceBroken` plugins run.
//! A plugin requiring anything else (`instance-access`, `fixedBlockLength`, ...) is
//! listed with the reason and not instantiated.
//!
//! # Wiring to a stereo insert
//!
//! Stereo in/out directly; 1 in / 1 out as two instances with linked parameters
//! (dual mono); 1 in / 2 out fed the mono sum; 2 in / 1 out to both sides. Side-chain
//! and CV inputs read silence, atom inputs an empty sequence; extra outputs are
//! discarded. Parameters are the visible control inputs, by port index; `lv2:enabled`,
//! `lv2:freeWheeling` and `time:beatsPerMinute` ports are driven by the host.
//!
//! # State
//!
//! [`Lv2Handle::get_state`] is lilv's state as Turtle text: the plugin's own state
//! (state extension) and every control port value, so a plugin without the state
//! extension still comes back with its settings.
//!
//! # Platforms and liblilv
//!
//! Linux only. liblilv is opened at run time (`liblilv-0.so.0`), never linked, so WAVE
//! builds and starts without it and [`status`] says what to install. Elsewhere every
//! entry point reports that LV2 is hosted on Linux.

#![cfg_attr(not(target_os = "linux"), allow(dead_code, unused_imports))]

pub mod layout;
pub mod params;
mod sys;
mod urid;
mod worker;

#[cfg(target_os = "linux")]
mod effect;
#[cfg(target_os = "linux")]
mod instance;
#[cfg(target_os = "linux")]
mod lilv;
#[cfg(target_os = "linux")]
mod ui;
#[cfg(target_os = "linux")]
mod world;

pub use params::{ParamInfo, ParamOption, ParamSpec};

/// `work()` calls the LV2 worker threads have made in this process (diagnostics, tests).
#[doc(hidden)]
pub fn worker_jobs_run() -> usize {
    worker::jobs_run()
}

#[cfg(target_os = "linux")]
pub use effect::{instantiate, instantiate_effect, scan, Lv2Effect, Lv2Handle, PluginInfo};

/// `Ok` when LV2 plugins can be hosted here; otherwise the sentence to show (liblilv
/// missing, or not Linux).
#[cfg(target_os = "linux")]
pub fn status() -> Result<(), String> {
    lilv::lilv().map(|_| ())
}

#[cfg(not(target_os = "linux"))]
mod unavailable {
    use serde::Serialize;

    const WHY: &str = "LV2 plugins are hosted on Linux only";

    /// One installed LV2 effect (never produced on this platform).
    #[derive(Debug, Clone, PartialEq, Serialize)]
    #[serde(rename_all = "camelCase")]
    pub struct PluginInfo {
        pub path: String,
        pub cid: String,
        pub name: String,
        pub vendor: String,
        pub category: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub unsupported: Option<String>,
        pub realtime: bool,
        #[serde(skip)]
        pub worker: bool,
    }

    /// Never constructed on this platform.
    #[derive(Clone, Debug)]
    pub struct Lv2Handle {
        never: std::convert::Infallible,
    }

    impl Lv2Handle {
        pub fn info(&self) -> &PluginInfo {
            match self.never {}
        }
        pub fn params(&self) -> Vec<crate::ParamInfo> {
            match self.never {}
        }
        pub fn set_param(&self, _id: u32, _value: f64) -> Result<(), String> {
            match self.never {}
        }
        pub fn get_state(&self) -> Result<Vec<u8>, String> {
            match self.never {}
        }
        pub fn take_state_changed(&self) -> bool {
            match self.never {}
        }
        pub fn latency(&self) -> usize {
            match self.never {}
        }
        pub fn has_editor(&self) -> bool {
            match self.never {}
        }
        pub fn editor_unavailable(&self) -> Option<String> {
            match self.never {}
        }
        pub fn is_editor_open(&self) -> bool {
            match self.never {}
        }
        pub fn open_editor(&self, _windows: &wave_plugin_windows::Service, _label: &wave_plugin_windows::Label) -> Result<(), String> {
            match self.never {}
        }
        pub fn close_editor(&self) -> Result<(), String> {
            match self.never {}
        }
    }

    pub fn status() -> Result<(), String> {
        Err(WHY.into())
    }

    pub fn scan(_rescan: bool) -> Result<Vec<PluginInfo>, String> {
        Err(WHY.into())
    }

    pub fn instantiate(
        _uri: &str,
        _state: Option<&[u8]>,
        _sample_rate: f32,
        _max_block: usize,
    ) -> Result<(Box<dyn wave_engine::Processor>, Lv2Handle), String> {
        Err(WHY.into())
    }
}

#[cfg(not(target_os = "linux"))]
pub use unavailable::{instantiate, scan, status, Lv2Handle, PluginInfo};
