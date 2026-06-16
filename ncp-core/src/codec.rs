//! Reference sensor→rate encoder and rate→command decoder.
//!
//! The codec is a *declarative* contract (`CodecSpec`) so a trained SNN policy
//! trains against a frozen interface. The reference implementation is linear
//! **rate coding** (deterministic, dependency-light): the encoder maps a
//! sensor-channel component onto a population firing rate; the decoder maps a
//! population's readout rate back onto a command-channel component. Mirrors
//! `backend/neurocontrol/codec.py`.

use crate::messages::{ChannelValue, CommandFrame, Map, Mode, SensorFrame};
use serde::{Deserialize, Serialize};

fn clamp(x: f64, lo: f64, hi: f64) -> f64 {
    if x < lo {
        lo
    } else if x > hi {
        hi
    } else {
        x
    }
}

fn lerp(x: f64, in_lo: f64, in_hi: f64, out_lo: f64, out_hi: f64) -> f64 {
    if (in_hi - in_lo).abs() < f64::EPSILON {
        return out_lo;
    }
    let frac = clamp((x - in_lo) / (in_hi - in_lo), 0.0, 1.0);
    out_lo + frac * (out_hi - out_lo)
}

fn default_value_range() -> (f64, f64) {
    (-1.0, 1.0)
}
fn default_rate_range() -> (f64, f64) {
    (0.0, 200.0)
}
fn default_codec_id() -> String {
    "ncp.codec.rate.v0".to_string()
}
fn one() -> i64 {
    1
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct EncoderChannelMap {
    pub channel: String,
    #[serde(default)]
    pub component: usize,
    pub population: String,
    #[serde(default = "default_coding")]
    pub coding: String,
    #[serde(default = "default_value_range")]
    pub value_range: (f64, f64),
    #[serde(default = "default_rate_range")]
    pub rate_range_hz: (f64, f64),
    #[serde(default = "one")]
    pub n_neurons: i64,
}

fn default_coding() -> String {
    "rate".to_string()
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct DecoderChannelMap {
    pub population: String,
    #[serde(default = "default_readout")]
    pub readout: String,
    pub command_channel: String,
    #[serde(default)]
    pub component: usize,
    #[serde(default)]
    pub unit: Option<String>,
    #[serde(default = "default_rate_range")]
    pub rate_range_hz: (f64, f64),
    #[serde(default = "default_value_range")]
    pub value_range: (f64, f64),
}

fn default_readout() -> String {
    "rate".to_string()
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(default)]
pub struct CodecSpec {
    pub codec_id: String,
    pub encoder: Vec<EncoderChannelMap>,
    pub decoder: Vec<DecoderChannelMap>,
}

impl Default for CodecSpec {
    fn default() -> Self {
        Self { codec_id: default_codec_id(), encoder: Vec::new(), decoder: Vec::new() }
    }
}

impl CodecSpec {
    /// Map a `SensorFrame` to `{population: firing_rate_hz}`.
    pub fn encode(&self, sensor: Option<&SensorFrame>) -> Map<f64> {
        let mut rates: Map<f64> = Map::new();
        for m in &self.encoder {
            let cv = sensor.and_then(|s| s.channels.get(&m.channel));
            match cv {
                Some(cv) if m.component < cv.data.len() => {
                    rates.insert(
                        m.population.clone(),
                        lerp(
                            cv.data[m.component],
                            m.value_range.0,
                            m.value_range.1,
                            m.rate_range_hz.0,
                            m.rate_range_hz.1,
                        ),
                    );
                }
                _ => {
                    rates.insert(m.population.clone(), m.rate_range_hz.0);
                }
            }
        }
        rates
    }

    /// Map `{population: readout_rate_hz}` to a `CommandFrame`.
    pub fn decode(&self, pop_rates: &Map<f64>, t: f64, seq: i64, frame_id: &str, mode: Mode) -> CommandFrame {
        let mut buffers: Map<Vec<f64>> = Map::new();
        let mut units: Map<Option<String>> = Map::new();
        for m in &self.decoder {
            // `component` is deserialized from an untrusted CodecSpec; bound the
            // per-channel buffer growth so a hostile/garbage value cannot drive an
            // unbounded Vec<f64> allocation (OOM/DoS). 4096 >> any real dimensionality.
            const MAX_COMPONENT: usize = 4096;
            if m.component >= MAX_COMPONENT {
                continue;
            }
            let rate = *pop_rates.get(&m.population).unwrap_or(&m.rate_range_hz.0);
            let value =
                lerp(rate, m.rate_range_hz.0, m.rate_range_hz.1, m.value_range.0, m.value_range.1);
            let buf = buffers.entry(m.command_channel.clone()).or_default();
            while buf.len() <= m.component {
                buf.push(0.0);
            }
            buf[m.component] = value;
            units.insert(m.command_channel.clone(), m.unit.clone());
        }
        let channels: Map<ChannelValue> = buffers
            .into_iter()
            .map(|(name, data)| {
                let unit = units.get(&name).cloned().flatten();
                (name, ChannelValue { data, unit })
            })
            .collect();
        CommandFrame { t, seq, frame_id: frame_id.to_string(), mode, channels, ..Default::default() }
    }
}

/// Illustrative 3-axis position-error → velocity-setpoint codec (untuned; it
/// documents the interface a trained SNN controller would train against).
pub fn default_uav_velocity_codec() -> CodecSpec {
    let mut enc = Vec::new();
    let mut dec = Vec::new();
    for (i, axis) in ["x", "y", "z"].iter().enumerate() {
        enc.push(EncoderChannelMap {
            channel: "pose_error".into(),
            component: i,
            population: format!("err_{axis}"),
            coding: "rate".into(),
            value_range: (-2.0, 2.0),
            rate_range_hz: (0.0, 200.0),
            n_neurons: 1,
        });
        dec.push(DecoderChannelMap {
            population: format!("vel_{axis}"),
            readout: "rate".into(),
            command_channel: "velocity_setpoint".into(),
            component: i,
            unit: Some("m/s".into()),
            rate_range_hz: (0.0, 200.0),
            value_range: (-1.5, 1.5),
        });
    }
    CodecSpec { codec_id: default_codec_id(), encoder: enc, decoder: dec }
}
