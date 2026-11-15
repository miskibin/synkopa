//! An LV2 control port as a parameter: its range, how its real value maps to the 0..1
//! that automation, the generic editor and `set_plugin_param` speak, and how it reads.
//!
//! Pure functions of the port's metadata, so they are tested on every platform.

use serde::Serialize;

/// What the plugin's Turtle says about one control input port (ranges already scaled by
/// the sample rate for `lv2:sampleRate` ports).
#[derive(Debug, Clone, PartialEq)]
pub struct ParamSpec {
    pub min: f32,
    pub max: f32,
    pub def: f32,
    /// `lv2:integer`: whole numbers only.
    pub integer: bool,
    /// `lv2:toggled`: off (≤ 0) or on (> 0).
    pub toggled: bool,
    /// `lv2:enumeration`: only the scale points are valid values.
    pub enumeration: bool,
    /// `pprops:logarithmic` (only honoured for a strictly positive range).
    pub log: bool,
    /// `lv2:scalePoint`s: (value, label), sorted by value.
    pub points: Vec<(f32, String)>,
    /// Unit symbol (`dB`, `Hz`, `ms`, `%`) or empty.
    pub unit: String,
}

impl ParamSpec {
    /// A spec from raw Turtle values: missing (NaN) or inverted bounds become a sane range,
    /// the default is clamped into it, scale points are sorted.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        min: f32,
        max: f32,
        def: f32,
        integer: bool,
        toggled: bool,
        enumeration: bool,
        log: bool,
        mut points: Vec<(f32, String)>,
        unit: String,
    ) -> Self {
        points.retain(|(v, _)| v.is_finite());
        points.sort_by(|a, b| a.0.total_cmp(&b.0));
        let (mut lo, mut hi) = (min, max);
        if !lo.is_finite() {
            lo = points.first().map_or(0.0, |p| p.0);
        }
        if !hi.is_finite() {
            hi = points.last().map_or(if toggled { 1.0 } else { lo.max(0.0) + 1.0 }, |p| p.0);
        }
        if hi < lo {
            std::mem::swap(&mut lo, &mut hi);
        }
        let def = if def.is_finite() { def.clamp(lo, hi) } else { lo };
        Self { min: lo, max: hi, def, integer, toggled, enumeration: enumeration && !points.is_empty(), log, points, unit }
    }

    fn uses_log(&self) -> bool {
        self.log && self.min > 0.0 && self.max > self.min && !self.toggled && !self.enumeration
    }

    /// Snap a real value to what the port accepts (range, whole numbers, on/off, a scale point).
    pub fn snap(&self, v: f32) -> f32 {
        if !v.is_finite() {
            return self.def;
        }
        if self.toggled {
            return if v > 0.5 * (self.min + self.max) { self.max } else { self.min };
        }
        if self.enumeration {
            return self.points.iter().map(|p| p.0).min_by(|a, b| (a - v).abs().total_cmp(&(b - v).abs())).unwrap_or(self.def);
        }
        let v = v.clamp(self.min, self.max);
        if self.integer {
            v.round().clamp(self.min.ceil(), self.max.floor().max(self.min.ceil()))
        } else {
            v
        }
    }

    /// Real value for a normalized 0..1 (out-of-range and NaN are clamped / the default).
    /// Allocation-free: called on the audio thread for automation.
    pub fn from_normalized(&self, n: f64) -> f32 {
        if !n.is_finite() {
            return self.def;
        }
        let n = n.clamp(0.0, 1.0) as f32;
        if self.enumeration {
            // the scale points are spread evenly over 0..1, in value order
            let count = self.points.len();
            let i = ((n * count as f32) as usize).min(count - 1);
            return self.points[i].0;
        }
        let v = if self.uses_log() { self.min * (self.max / self.min).powf(n) } else { self.min + n * (self.max - self.min) };
        self.snap(v)
    }

    /// Normalized 0..1 of a real value.
    pub fn to_normalized(&self, v: f32) -> f64 {
        let v = self.snap(v);
        if self.enumeration {
            let count = self.points.len();
            let i = self.points.iter().position(|p| p.0 == v).unwrap_or(0);
            return (i as f64 + 0.5) / count as f64;
        }
        if self.max <= self.min {
            return 0.0;
        }
        let n = if self.uses_log() { (v / self.min).ln() / (self.max / self.min).ln() } else { (v - self.min) / (self.max - self.min) };
        f64::from(n.clamp(0.0, 1.0))
    }

    /// Steps a control has when it is not continuous (toggle 2, enumeration its points, an
    /// integer its whole numbers up to 128); `None` for a continuous one.
    pub fn steps(&self) -> Option<u32> {
        if self.toggled {
            Some(2)
        } else if self.enumeration {
            Some(self.points.len() as u32)
        } else if self.integer {
            let n = (self.max.floor() - self.min.ceil()) as i64 + 1;
            (1..=128).contains(&n).then_some(n as u32)
        } else {
            None
        }
    }

    /// The value as a person reads it: a scale point's label, On/Off, or the number with its unit.
    pub fn display(&self, v: f32) -> String {
        if self.toggled {
            return if v > 0.5 * (self.min + self.max) { "On".into() } else { "Off".into() };
        }
        if let Some((_, label)) = self.points.iter().find(|p| (p.0 - v).abs() <= f32::EPSILON * p.0.abs().max(1.0)) {
            return label.clone();
        }
        let num = if self.integer {
            format!("{v:.0}")
        } else {
            let a = v.abs();
            if a >= 100.0 || a == 0.0 {
                format!("{v:.0}")
            } else if a >= 10.0 {
                format!("{v:.1}")
            } else if a >= 1.0 {
                format!("{v:.2}")
            } else {
                format!("{v:.3}")
            }
        };
        if self.unit.is_empty() {
            num
        } else if self.unit == "%" {
            format!("{num}%")
        } else {
            format!("{num} {}", self.unit)
        }
    }
}

/// The symbol for an LV2 `units:` unit URI (the standard ones), else empty.
pub fn unit_symbol(uri: &str) -> &'static str {
    let Some(name) = uri.strip_prefix("http://lv2plug.in/ns/extensions/units#") else {
        return "";
    };
    match name {
        "db" => "dB",
        "hz" => "Hz",
        "khz" => "kHz",
        "mhz" => "MHz",
        "ms" => "ms",
        "s" => "s",
        "min" => "min",
        "pc" => "%",
        "bpm" => "BPM",
        "cent" => "ct",
        "semitone12TET" => "st",
        "oct" => "oct",
        "degree" => "°",
        "midiNote" => "note",
        "frame" => "frames",
        "beat" => "beats",
        "bar" => "bars",
        "m" => "m",
        "cm" => "cm",
        "mm" => "mm",
        "km" => "km",
        "inch" => "in",
        "mile" => "mi",
        _ => "",
    }
}

/// A scale point of an enumeration parameter, for a picker.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ParamOption {
    /// Normalized 0..1 value that selects it.
    pub value: f64,
    pub label: String,
}

/// One parameter of a hosted LV2 plugin, as `plugin_params` reports it. `value` and
/// `def` are normalized 0..1 (what automation and `set_param` take); `real`, `min` and
/// `max` are the port's own values, `display` the value with its unit.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ParamInfo {
    /// The control port's index.
    pub id: u32,
    /// The port symbol (stable across plugin versions, unlike the index).
    pub symbol: String,
    pub name: String,
    pub value: f64,
    pub display: String,
    pub def: f64,
    pub real: f64,
    pub min: f64,
    pub max: f64,
    pub unit: String,
    /// Discrete positions (toggle, enumeration, integer); absent for a continuous control.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub steps: Option<u32>,
    pub toggle: bool,
    pub integer: bool,
    pub log: bool,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub options: Vec<ParamOption>,
}

impl ParamInfo {
    pub fn new(id: u32, symbol: &str, name: String, spec: &ParamSpec, real: f32) -> Self {
        let options = if spec.enumeration {
            spec.points.iter().map(|(v, label)| ParamOption { value: spec.to_normalized(*v), label: label.clone() }).collect()
        } else {
            vec![]
        };
        Self {
            id,
            symbol: symbol.to_owned(),
            name,
            value: spec.to_normalized(real),
            display: spec.display(real),
            def: spec.to_normalized(spec.def),
            real: f64::from(real),
            min: f64::from(spec.min),
            max: f64::from(spec.max),
            unit: spec.unit.clone(),
            steps: spec.steps(),
            toggle: spec.toggled,
            integer: spec.integer,
            log: spec.uses_log(),
            options,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lin(min: f32, max: f32, def: f32) -> ParamSpec {
        ParamSpec::new(min, max, def, false, false, false, false, vec![], String::new())
    }

    #[test]
    fn linear_ranges_round_trip_through_normalized() {
        let s = lin(-24.0, 12.0, 0.0);
        assert_eq!(s.from_normalized(0.0), -24.0);
        assert_eq!(s.from_normalized(1.0), 12.0);
        assert!((s.to_normalized(0.0) - 24.0 / 36.0).abs() < 1e-6);
        for v in [-24.0, -3.5, 0.0, 11.0] {
            assert!((s.from_normalized(s.to_normalized(v)) - v).abs() < 1e-4, "{v}");
        }
        assert_eq!(s.from_normalized(f64::NAN), 0.0, "NaN is the default");
        assert_eq!(s.from_normalized(7.0), 12.0, "clamped");
    }

    #[test]
    fn logarithmic_frequency_is_geometric_and_falls_back_to_linear_through_zero() {
        let s = ParamSpec::new(20.0, 20000.0, 1000.0, false, false, false, true, vec![], "Hz".into());
        assert!((s.from_normalized(0.5) - 632.455_5).abs() < 0.01, "{}", s.from_normalized(0.5));
        assert!((s.to_normalized(632.455_5) - 0.5).abs() < 1e-5);
        let through_zero = ParamSpec::new(0.0, 100.0, 0.0, false, false, false, true, vec![], String::new());
        assert_eq!(through_zero.from_normalized(0.5), 50.0, "a log range through 0 is linear");
    }

    #[test]
    fn integers_toggles_and_enumerations_snap() {
        let int = ParamSpec::new(1.0, 8.0, 2.0, true, false, false, false, vec![], String::new());
        assert_eq!(int.from_normalized(0.5), 5.0, "1 + 3.5 rounds to a whole number");
        assert_eq!(int.steps(), Some(8));
        let toggle = ParamSpec::new(0.0, 1.0, 0.0, false, true, false, false, vec![], String::new());
        assert_eq!(toggle.from_normalized(0.4), 0.0);
        assert_eq!(toggle.from_normalized(0.6), 1.0);
        assert_eq!(toggle.display(1.0), "On");
        assert_eq!(toggle.steps(), Some(2));
        let modes = vec![(2.0, "Peak".to_owned()), (0.0, "Low shelf".to_owned()), (1.0, "High shelf".to_owned())];
        let e = ParamSpec::new(0.0, 2.0, 0.0, true, false, true, false, modes, String::new());
        assert_eq!(e.points[0].1, "Low shelf", "sorted by value");
        assert_eq!(e.from_normalized(0.0), 0.0);
        assert_eq!(e.from_normalized(0.5), 1.0);
        assert_eq!(e.from_normalized(1.0), 2.0);
        for v in [0.0, 1.0, 2.0] {
            assert_eq!(e.from_normalized(e.to_normalized(v)), v);
        }
        assert_eq!(e.display(2.0), "Peak");
        let info = ParamInfo::new(4, "mode", "Mode".into(), &e, 1.0);
        assert_eq!(info.options.len(), 3);
        assert_eq!(info.steps, Some(3));
        assert_eq!(e.from_normalized(info.options[2].value), 2.0);
    }

    #[test]
    fn missing_or_inverted_metadata_gives_a_usable_range() {
        let s = ParamSpec::new(f32::NAN, f32::NAN, f32::NAN, false, false, false, false, vec![], String::new());
        assert_eq!((s.min, s.max, s.def), (0.0, 1.0, 0.0));
        let s = ParamSpec::new(10.0, -10.0, 50.0, false, false, false, false, vec![], String::new());
        assert_eq!((s.min, s.max, s.def), (-10.0, 10.0, 10.0));
        let flat = lin(3.0, 3.0, 3.0);
        assert_eq!(flat.to_normalized(3.0), 0.0);
        assert_eq!(flat.from_normalized(0.7), 3.0);
    }

    #[test]
    fn values_read_with_their_units() {
        let db = ParamSpec::new(
            -60.0,
            6.0,
            0.0,
            false,
            false,
            false,
            false,
            vec![],
            unit_symbol("http://lv2plug.in/ns/extensions/units#db").into(),
        );
        assert_eq!(db.display(-6.0), "-6.00 dB");
        let pc = ParamSpec::new(
            0.0,
            100.0,
            0.0,
            false,
            false,
            false,
            false,
            vec![],
            unit_symbol("http://lv2plug.in/ns/extensions/units#pc").into(),
        );
        assert_eq!(pc.display(50.0), "50.0%");
        assert_eq!(unit_symbol("http://example.com/units#furlong"), "");
        let info = ParamInfo::new(3, "gain", "Gain".into(), &db, -6.0);
        let json = serde_json::to_value(&info).unwrap();
        assert_eq!(json["display"], "-6.00 dB");
        assert_eq!(json["unit"], "dB");
        assert!(json.get("steps").is_none() && json.get("options").is_none(), "{json}");
    }
}
