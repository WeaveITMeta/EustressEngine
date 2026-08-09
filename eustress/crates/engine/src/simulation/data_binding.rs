//! Data → Simulation binding: a Dataset column drives a live sim parameter.
//!
//! This is the input half of the Data Platform's fusion with the simulation.
//! The read half already exists (`WatchPointRegistry` records sim values into
//! series; `BreakPointRegistry` pauses on a condition). What was missing is the
//! other direction: real measured numbers *driving* the simulation.
//!
//! A [`DataBinding`] materializes one numeric column of a Dataset's CSV and
//! writes it into the sim's parameter map each frame, either row-by-row
//! (`ByRow`) or interpolated against simulation time (`ByTime`).
//!
//! ## Why it writes to BOTH sim-value maps
//!
//! `SimValuesResource` is the published mirror the engine republishes out of the
//! ECS every frame; `ScriptSimWrites` is the "written explicitly this frame"
//! set that `apply_sim_values_to_ecs` consults so a mode default (e.g.
//! `battery.mode` re-applying "discharge at 0.5C") does not silently clobber an
//! intentional write on the very next frame. A binding is an intentional write,
//! so it must appear in both — writing only the published mirror would make the
//! bound value flicker back to the default and look like the binding "didn't
//! take".

use bevy::prelude::*;
use eustress_common::simulation::SimulationClock;

use super::plugin::{ScriptSimWrites, SimValuesResource};

/// How a binding picks the value for the current frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BindMode {
    /// Advance one row per frame (a playback / replay drive).
    ByRow,
    /// Interpolate the column against `simulation_time_s` using an index
    /// column of times (a time-accurate drive).
    ByTime,
}

impl BindMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            BindMode::ByRow => "by_row",
            BindMode::ByTime => "by_time",
        }
    }
    pub fn parse(s: &str) -> Self {
        match s {
            "by_time" | "time" => BindMode::ByTime,
            _ => BindMode::ByRow,
        }
    }
}

/// One Dataset column bound to a named simulation parameter.
#[derive(Debug, Clone)]
pub struct DataBinding {
    /// Dataset instance name (identity for listing / unbinding).
    pub dataset: String,
    /// Column name inside the dataset's frame.
    pub column: String,
    /// Sim value key this column drives, e.g. `battery.current`.
    pub target: String,
    pub mode: BindMode,
    /// Materialized column — read once at bind time, not per frame.
    pub values: Vec<f64>,
    /// Time column for `ByTime` (same length as `values`).
    pub times: Option<Vec<f64>>,
    /// Row cursor for `ByRow`.
    pub cursor: usize,
    /// Most recent value written (surfaced by `data.bindings`).
    pub last_value: Option<f64>,
    pub enabled: bool,
    /// Restart at row 0 when the column runs out (`ByRow` only).
    pub looping: bool,
}

impl DataBinding {
    /// Value for the current frame, advancing the cursor for `ByRow`.
    fn sample(&mut self, sim_time_s: f64) -> Option<f64> {
        if self.values.is_empty() {
            return None;
        }
        match self.mode {
            BindMode::ByRow => {
                let idx = self.cursor.min(self.values.len() - 1);
                let v = self.values[idx];
                if self.cursor + 1 < self.values.len() {
                    self.cursor += 1;
                } else if self.looping {
                    self.cursor = 0;
                }
                Some(v)
            }
            BindMode::ByTime => {
                let times = self.times.as_ref()?;
                if times.len() != self.values.len() || times.is_empty() {
                    return None;
                }
                // Clamp outside the recorded span rather than extrapolating —
                // a driven parameter should never invent data past its record.
                if sim_time_s <= times[0] {
                    return Some(self.values[0]);
                }
                if sim_time_s >= times[times.len() - 1] {
                    return Some(self.values[self.values.len() - 1]);
                }
                // Linear interpolation between the bracketing samples.
                let mut hi = 0usize;
                while hi < times.len() && times[hi] < sim_time_s {
                    hi += 1;
                }
                let lo = hi.saturating_sub(1);
                let (t0, t1) = (times[lo], times[hi]);
                let span = t1 - t0;
                if span.abs() < f64::EPSILON {
                    return Some(self.values[lo]);
                }
                let f = (sim_time_s - t0) / span;
                Some(self.values[lo] + (self.values[hi] - self.values[lo]) * f)
            }
        }
    }
}

/// All active data → sim bindings.
#[derive(Resource, Default)]
pub struct DataBindingRegistry(pub Vec<DataBinding>);

impl DataBindingRegistry {
    /// Replace any binding already driving `target` — one column owns a
    /// parameter, so re-binding is an update rather than a silent double-write.
    pub fn upsert(&mut self, binding: DataBinding) {
        if let Some(slot) = self.0.iter_mut().find(|b| b.target == binding.target) {
            *slot = binding;
        } else {
            self.0.push(binding);
        }
    }

    /// Remove the binding driving `target`. Returns whether one was removed.
    pub fn remove_target(&mut self, target: &str) -> bool {
        let before = self.0.len();
        self.0.retain(|b| b.target != target);
        self.0.len() != before
    }
}

/// Write every enabled binding's current value into the sim parameter maps.
///
/// Runs each frame while a simulation is live. See the module docs for why the
/// value lands in both `SimValuesResource` and `ScriptSimWrites`.
pub fn advance_data_bindings(
    clock: Res<SimulationClock>,
    mut registry: ResMut<DataBindingRegistry>,
    mut sim_values: ResMut<SimValuesResource>,
    mut script_writes: ResMut<ScriptSimWrites>,
) {
    if registry.0.is_empty() {
        return;
    }
    let t = clock.simulation_time_s;
    for binding in registry.0.iter_mut() {
        if !binding.enabled {
            continue;
        }
        if let Some(v) = binding.sample(t) {
            binding.last_value = Some(v);
            sim_values.0.insert(binding.target.clone(), v);
            script_writes.0.insert(binding.target.clone(), v);
        }
    }
}

/// Read a numeric column (plus an optional time column) out of the first `.csv`
/// beside a Dataset's instance folder — the same resolution the Data Grid,
/// chart, and Properties inspector use.
#[cfg(feature = "data")]
pub fn load_column(
    dir: &std::path::Path,
    column: &str,
    time_column: Option<&str>,
) -> Result<(Vec<f64>, Option<Vec<f64>>), String> {
    use eustress_data::ColumnData;

    let csv = std::fs::read_dir(dir)
        .map_err(|e| format!("cannot read dataset dir: {e}"))?
        .filter_map(|x| x.ok())
        .map(|x| x.path())
        .find(|p| {
            p.extension()
                .and_then(|x| x.to_str())
                .map(|x| x.eq_ignore_ascii_case("csv"))
                .unwrap_or(false)
        })
        .ok_or_else(|| "no .csv beside the dataset".to_string())?;

    let file = std::fs::File::open(&csv).map_err(|e| format!("cannot open csv: {e}"))?;
    let frame = eustress_data::import::frame_from_csv(file)
        .map_err(|e| format!("cannot parse csv: {e}"))?;

    let take = |name: &str| -> Option<Vec<f64>> {
        let (_, data) = frame.columns().iter().find(|(s, _)| s.name == name)?;
        let n = frame.n_rows();
        let mut out = Vec::with_capacity(n);
        for r in 0..n {
            let v = match data {
                ColumnData::F64(v) => v.get(r).and_then(|o| *o),
                ColumnData::I64(v) => v.get(r).and_then(|o| *o).map(|x| x as f64),
                _ => None,
            };
            out.push(v.unwrap_or(f64::NAN));
        }
        Some(out)
    };

    let values = take(column).ok_or_else(|| {
        let available: Vec<&str> = frame.columns().iter().map(|(s, _)| s.name.as_str()).collect();
        format!("column '{column}' not found; available: {}", available.join(", "))
    })?;
    let times = time_column.and_then(take);
    Ok((values, times))
}
