//! How a plugin's audio ports meet a stereo insert slot.

/// The wiring of a Synkopa stereo insert to a plugin's main audio ports. Side-chain inputs
/// and CV inputs are fed silence; extra outputs are written to a scratch sink.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layout {
    /// Two or more main inputs and outputs: left and right go to the first two of each.
    Stereo,
    /// One input, one output: two instances, one per side, with linked parameters.
    DualMono,
    /// One input, two or more outputs (a mono reverb or widener): fed the mono sum.
    MonoToStereo,
    /// Two or more inputs, one output (a summing effect): the output goes to both sides.
    StereoToMono,
}

impl Layout {
    /// The wiring for `inputs` main audio inputs and `outputs` audio outputs, or why the
    /// plugin is not an insert effect.
    pub fn plan(inputs: usize, outputs: usize) -> Result<Self, String> {
        match (inputs, outputs) {
            (0, 0) => Err("it has no audio ports".into()),
            (0, _) => Err("it has no audio input (an instrument or a generator)".into()),
            (_, 0) => Err("it has no audio output (an analyser)".into()),
            (1, 1) => Ok(Self::DualMono),
            (1, _) => Ok(Self::MonoToStereo),
            (_, 1) => Ok(Self::StereoToMono),
            _ => Ok(Self::Stereo),
        }
    }

    /// Plugin instances this wiring runs.
    pub fn instances(self) -> usize {
        if self == Self::DualMono {
            2
        } else {
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_port_count_has_a_wiring_or_a_reason() {
        assert_eq!(Layout::plan(2, 2), Ok(Layout::Stereo));
        assert_eq!(Layout::plan(4, 2), Ok(Layout::Stereo), "extra inputs are side chains, fed silence");
        assert_eq!(Layout::plan(1, 1), Ok(Layout::DualMono));
        assert_eq!(Layout::DualMono.instances(), 2);
        assert_eq!(Layout::Stereo.instances(), 1);
        assert_eq!(Layout::plan(1, 2), Ok(Layout::MonoToStereo));
        assert_eq!(Layout::plan(2, 1), Ok(Layout::StereoToMono));
        assert!(Layout::plan(0, 2).unwrap_err().contains("instrument"));
        assert!(Layout::plan(2, 0).unwrap_err().contains("analyser"));
        assert!(Layout::plan(0, 0).is_err());
    }
}
