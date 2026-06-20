//! Packed little-endian **columnar codec** for bulk numeric observation data (#6).
//!
//! The observation/analysis plane carries large numeric arrays — spike trains
//! (`senders`), `V_m`/`g_ex`/`w` traces (`values`), and their `times`. Encoding
//! those as protobuf `repeated double` or JSON is parse+serialize work that scales
//! with the event count (~11 ms for 50k spikes, measured). This module carries
//! them instead as a **self-describing little-endian column block** inside a thin
//! `bytes` envelope: fixed-width, parse-free (bulk `copy_from_slice`, no
//! tokenizer), and random-access via a column directory of byte offsets — the
//! property Arrow IPC / Cap'n Proto provide, without the dependency.
//!
//! ## Boundary
//!
//! This codec is for the **observation/analysis data plane only** — the bulk
//! channel. The small, latency-critical control-loop frames
//! ([`SensorFrame`](crate::SensorFrame) / [`CommandFrame`](crate::CommandFrame) /
//! [`StimulusFrame`](crate::StimulusFrame)) stay JSON/protobuf and **never** ride
//! this codec; it must never be on the hot action loop. It is an additive wire
//! option (v0.2): peers negotiate it; the JSON `ObservationFrame` remains the
//! canonical, always-available representation.
//!
//! ## Wire layout (all integers little-endian)
//!
//! ```text
//! offset  size  field
//! 0       4     magic   = b"NCPB"
//! 4       1     version = 1
//! 5       1     flags   = 0          (bit0: 0 = little-endian; reserved otherwise)
//! 6       2     n_cols  : u16
//! 8       4     total_len : u32      (== bytes.len(); guards truncation/over-read)
//! 12      16*n  column directory (one 16-byte entry per column):
//!                 0  4  name_off : u32   (offset from block start to the name bytes)
//!                 4  2  name_len : u16
//!                 6  1  dtype    : u8    (1=f32, 2=f64, 3=i32, 4=i64)
//!                 7  1  _pad     : u8 = 0
//!                 8  4  n_rows   : u32    (element count of this column)
//!                 12 4  data_off : u32   (offset from block start to column data)
//! ...           name pool (concatenated utf-8 names), then column data blocks
//! ```
//!
//! Decoding is **fully bounds-checked** against untrusted bytes: a bad magic,
//! unsupported version/flags/dtype, an out-of-range offset/length, an
//! allocation-bomb `n_rows`, or a `total_len` that disagrees with the buffer all
//! fail closed with [`BulkError`] rather than panic or over-read.

use crate::messages::{Observable, Observation};

/// Magic prefix identifying an NCP bulk column block.
pub const BULK_MAGIC: [u8; 4] = *b"NCPB";
/// On-wire format version for [`BulkBlock`].
pub const BULK_VERSION: u8 = 1;

const HEADER_LEN: usize = 12;
const DIR_ENTRY_LEN: usize = 16;

const DTYPE_F32: u8 = 1;
const DTYPE_F64: u8 = 2;
const DTYPE_I32: u8 = 3;
const DTYPE_I64: u8 = 4;

/// A single typed numeric column. `f32`/`i32` are the compact widths the issue
/// calls for (halving trace/sender bytes); `f64`/`i64` are lossless and match the
/// [`Observation`] field types exactly.
#[derive(Clone, PartialEq, Debug)]
pub enum Column {
    F32(Vec<f32>),
    F64(Vec<f64>),
    I32(Vec<i32>),
    I64(Vec<i64>),
}

impl Column {
    fn dtype(&self) -> u8 {
        match self {
            Column::F32(_) => DTYPE_F32,
            Column::F64(_) => DTYPE_F64,
            Column::I32(_) => DTYPE_I32,
            Column::I64(_) => DTYPE_I64,
        }
    }
    fn width(dtype: u8) -> usize {
        match dtype {
            DTYPE_F32 | DTYPE_I32 => 4,
            DTYPE_F64 | DTYPE_I64 => 8,
            _ => 0,
        }
    }
    /// Element count.
    pub fn len(&self) -> usize {
        match self {
            Column::F32(v) => v.len(),
            Column::F64(v) => v.len(),
            Column::I32(v) => v.len(),
            Column::I64(v) => v.len(),
        }
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
    fn encode_data(&self, out: &mut Vec<u8>) {
        match self {
            Column::F32(v) => v
                .iter()
                .for_each(|x| out.extend_from_slice(&x.to_le_bytes())),
            Column::F64(v) => v
                .iter()
                .for_each(|x| out.extend_from_slice(&x.to_le_bytes())),
            Column::I32(v) => v
                .iter()
                .for_each(|x| out.extend_from_slice(&x.to_le_bytes())),
            Column::I64(v) => v
                .iter()
                .for_each(|x| out.extend_from_slice(&x.to_le_bytes())),
        }
    }
    /// View as `f64` (for analog columns, regardless of stored width). Exact for
    /// the f32/f64/i32 arms; the i64 arm rounds magnitudes above 2^53 (not hit by
    /// the codec round-trip, which only feeds analog data through f32/f64).
    pub fn as_f64(&self) -> Vec<f64> {
        match self {
            Column::F32(v) => v.iter().map(|&x| x as f64).collect(),
            Column::F64(v) => v.clone(),
            Column::I32(v) => v.iter().map(|&x| x as f64).collect(),
            Column::I64(v) => v.iter().map(|&x| x as f64).collect(),
        }
    }
    /// View as `i64` (for integer columns like spike senders). Exact for the
    /// i32/i64 arms; the f32/f64 arms truncate toward zero (not hit by the codec
    /// round-trip, which only feeds integer data through i32/i64).
    pub fn as_i64(&self) -> Vec<i64> {
        match self {
            Column::I32(v) => v.iter().map(|&x| x as i64).collect(),
            Column::I64(v) => v.clone(),
            Column::F32(v) => v.iter().map(|&x| x as i64).collect(),
            Column::F64(v) => v.iter().map(|&x| x as i64).collect(),
        }
    }
}

/// Why a [`BulkBlock::decode`] of untrusted bytes was rejected.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum BulkError {
    TooShort,
    BadMagic,
    UnsupportedVersion(u8),
    UnsupportedFlags(u8),
    LengthMismatch {
        declared: usize,
        actual: usize,
    },
    BadDtype(u8),
    /// An offset/length in the directory points outside the block.
    OutOfBounds,
    /// `n_rows * width` would overflow `usize` (allocation bomb).
    Overflow,
    /// A column name was not valid utf-8.
    BadName,
    /// The parallel numeric columns (`times`/`values`/`senders`) disagree in
    /// length — they index the same events/samples, so a mismatch is corrupt.
    ColumnLengthMismatch {
        a: &'static str,
        a_len: usize,
        b: &'static str,
        b_len: usize,
    },
}

impl std::fmt::Display for BulkError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BulkError::TooShort => write!(f, "bulk block shorter than header"),
            BulkError::BadMagic => write!(f, "bulk block has wrong magic (expected NCPB)"),
            BulkError::UnsupportedVersion(v) => write!(f, "unsupported bulk version {v}"),
            BulkError::UnsupportedFlags(x) => write!(f, "unsupported bulk flags {x:#x}"),
            BulkError::LengthMismatch { declared, actual } => {
                write!(f, "bulk total_len {declared} != buffer {actual}")
            }
            BulkError::BadDtype(d) => write!(f, "unknown bulk dtype {d}"),
            BulkError::OutOfBounds => write!(f, "bulk directory offset out of bounds"),
            BulkError::Overflow => write!(f, "bulk column size overflow"),
            BulkError::BadName => write!(f, "bulk column name not valid utf-8"),
            BulkError::ColumnLengthMismatch { a, a_len, b, b_len } => write!(
                f,
                "bulk parallel columns disagree: {a} has {a_len}, {b} has {b_len}"
            ),
        }
    }
}

impl std::error::Error for BulkError {}

/// An ordered set of named typed columns — the parse-free representation of a
/// bulk numeric payload.
#[derive(Clone, PartialEq, Debug, Default)]
pub struct BulkBlock {
    pub columns: Vec<(String, Column)>,
}

impl BulkBlock {
    pub fn new() -> Self {
        BulkBlock::default()
    }

    /// Append a named column (builder style).
    pub fn with(mut self, name: impl Into<String>, col: Column) -> Self {
        self.columns.push((name.into(), col));
        self
    }

    /// Look up a column by name.
    pub fn get(&self, name: &str) -> Option<&Column> {
        self.columns.iter().find(|(n, _)| n == name).map(|(_, c)| c)
    }

    /// The parallel numeric columns (`times`/`values`/`senders`) index the same
    /// events/samples, so every PRESENT, non-empty one MUST agree in length. A
    /// mismatch is a corrupt/hostile block — fail closed rather than silently
    /// pairing arrays of different lengths.
    pub fn check_parallel(&self) -> Result<(), BulkError> {
        let mut expected: Option<(&'static str, usize)> = None;
        for name in ["times", "values", "senders"] {
            let n = match self.get(name) {
                Some(c) => c.len(),
                None => continue,
            };
            if n == 0 {
                continue;
            }
            match expected {
                None => expected = Some((name, n)),
                Some((a, a_len)) if a_len != n => {
                    return Err(BulkError::ColumnLengthMismatch {
                        a,
                        a_len,
                        b: name,
                        b_len: n,
                    });
                }
                _ => {}
            }
        }
        Ok(())
    }

    /// Serialize to the packed little-endian block.
    ///
    /// Limits (far above the observation-plane envelope): at most 65535 columns,
    /// and each column's element count, the per-column byte offsets, and the total
    /// block length must each fit in a `u32`. Inputs beyond these would wrap the
    /// header casts; the observation bulk channel is orders of magnitude smaller
    /// (a 50k-spike block is ~1.6 MB), so this is a documented bound, not a guard.
    pub fn encode(&self) -> Vec<u8> {
        let n_cols = self.columns.len();
        // Names laid out after the directory; column data after the name pool.
        let dir_len = n_cols * DIR_ENTRY_LEN;
        let name_pool_start = HEADER_LEN + dir_len;

        // Pre-compute the name pool and per-column data offsets.
        let mut name_pool = Vec::new();
        let mut name_spans = Vec::with_capacity(n_cols); // (off, len)
        for (name, _) in &self.columns {
            let off = name_pool_start + name_pool.len();
            let bytes = name.as_bytes();
            name_pool.extend_from_slice(bytes);
            name_spans.push((off, bytes.len()));
        }

        let data_start = name_pool_start + name_pool.len();
        let mut data = Vec::new();
        let mut data_offs = Vec::with_capacity(n_cols);
        for (_, col) in &self.columns {
            data_offs.push(data_start + data.len());
            col.encode_data(&mut data);
        }

        let total_len = data_start + data.len();
        let mut out = Vec::with_capacity(total_len);
        out.extend_from_slice(&BULK_MAGIC);
        out.push(BULK_VERSION);
        out.push(0); // flags: little-endian
        out.extend_from_slice(&(n_cols as u16).to_le_bytes());
        out.extend_from_slice(&(total_len as u32).to_le_bytes());

        for (i, (_, col)) in self.columns.iter().enumerate() {
            let (name_off, name_len) = name_spans[i];
            out.extend_from_slice(&(name_off as u32).to_le_bytes());
            out.extend_from_slice(&(name_len as u16).to_le_bytes());
            out.push(col.dtype());
            out.push(0); // pad
            out.extend_from_slice(&(col.len() as u32).to_le_bytes());
            out.extend_from_slice(&(data_offs[i] as u32).to_le_bytes());
        }
        out.extend_from_slice(&name_pool);
        out.extend_from_slice(&data);
        debug_assert_eq!(out.len(), total_len);
        out
    }

    /// Parse a packed block. Fully bounds-checked against untrusted input.
    pub fn decode(bytes: &[u8]) -> Result<BulkBlock, BulkError> {
        if bytes.len() < HEADER_LEN {
            return Err(BulkError::TooShort);
        }
        if bytes[0..4] != BULK_MAGIC {
            return Err(BulkError::BadMagic);
        }
        if bytes[4] != BULK_VERSION {
            return Err(BulkError::UnsupportedVersion(bytes[4]));
        }
        if bytes[5] != 0 {
            return Err(BulkError::UnsupportedFlags(bytes[5]));
        }
        let n_cols = u16::from_le_bytes([bytes[6], bytes[7]]) as usize;
        let total_len = u32::from_le_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]) as usize;
        if total_len != bytes.len() {
            return Err(BulkError::LengthMismatch {
                declared: total_len,
                actual: bytes.len(),
            });
        }
        // The directory itself must fit.
        let dir_end = HEADER_LEN
            .checked_add(
                n_cols
                    .checked_mul(DIR_ENTRY_LEN)
                    .ok_or(BulkError::Overflow)?,
            )
            .ok_or(BulkError::Overflow)?;
        if dir_end > bytes.len() {
            return Err(BulkError::OutOfBounds);
        }

        let mut columns = Vec::with_capacity(n_cols);
        for i in 0..n_cols {
            let base = HEADER_LEN + i * DIR_ENTRY_LEN;
            let e = &bytes[base..base + DIR_ENTRY_LEN];
            let name_off = u32::from_le_bytes([e[0], e[1], e[2], e[3]]) as usize;
            let name_len = u16::from_le_bytes([e[4], e[5]]) as usize;
            let dtype = e[6];
            let n_rows = u32::from_le_bytes([e[8], e[9], e[10], e[11]]) as usize;
            let data_off = u32::from_le_bytes([e[12], e[13], e[14], e[15]]) as usize;

            // Name slice in bounds.
            let name_end = name_off.checked_add(name_len).ok_or(BulkError::Overflow)?;
            let name_bytes = bytes
                .get(name_off..name_end)
                .ok_or(BulkError::OutOfBounds)?;
            let name = std::str::from_utf8(name_bytes)
                .map_err(|_| BulkError::BadName)?
                .to_string();

            let width = Column::width(dtype);
            if width == 0 {
                return Err(BulkError::BadDtype(dtype));
            }
            let data_len = n_rows.checked_mul(width).ok_or(BulkError::Overflow)?;
            let data_end = data_off.checked_add(data_len).ok_or(BulkError::Overflow)?;
            let data = bytes
                .get(data_off..data_end)
                .ok_or(BulkError::OutOfBounds)?;

            let col = decode_column(dtype, data, n_rows);
            columns.push((name, col));
        }
        Ok(BulkBlock { columns })
    }
}

fn decode_column(dtype: u8, data: &[u8], n_rows: usize) -> Column {
    macro_rules! read {
        ($ty:ty, $variant:ident, $w:expr) => {{
            let mut v = Vec::with_capacity(n_rows);
            for chunk in data.chunks_exact($w) {
                let mut buf = [0u8; $w];
                buf.copy_from_slice(chunk);
                v.push(<$ty>::from_le_bytes(buf));
            }
            Column::$variant(v)
        }};
    }
    match dtype {
        DTYPE_F32 => read!(f32, F32, 4),
        DTYPE_F64 => read!(f64, F64, 8),
        DTYPE_I32 => read!(i32, I32, 4),
        DTYPE_I64 => read!(i64, I64, 8),
        _ => unreachable!("dtype validated by caller"),
    }
}

impl Observation {
    /// Pack this observation's bulk numeric arrays into a [`BulkBlock`]
    /// (lossless: `times`/`values` as f64, `senders` as i64). Only non-empty
    /// arrays become columns, so a spike port (times+senders) and an analog port
    /// (times+values) each pack just their two populated columns.
    pub fn to_bulk_block(&self) -> BulkBlock {
        let mut b = BulkBlock::new();
        if !self.times.is_empty() {
            b = b.with("times", Column::F64(self.times.clone()));
        }
        if !self.values.is_empty() {
            b = b.with("values", Column::F64(self.values.clone()));
        }
        if !self.senders.is_empty() {
            b = b.with("senders", Column::I64(self.senders.clone()));
        }
        b
    }

    /// Replace this observation's bulk arrays from a decoded [`BulkBlock`]
    /// (the inverse of [`Observation::to_bulk_block`]; tolerant of compact
    /// f32/i32 widths). Columns absent from the block are cleared.
    pub fn apply_bulk_block(&mut self, b: &BulkBlock) {
        self.times = b.get("times").map(Column::as_f64).unwrap_or_default();
        self.values = b.get("values").map(Column::as_f64).unwrap_or_default();
        self.senders = b.get("senders").map(Column::as_i64).unwrap_or_default();
    }

    /// Round-trip the bulk arrays through the packed codec, returning the encoded
    /// bytes — the observation-plane bulk envelope payload for this port.
    pub fn to_bulk_bytes(&self) -> Vec<u8> {
        self.to_bulk_block().encode()
    }
}

/// Reconstruct an [`Observation`] from its metadata plus a packed bulk block —
/// the receive side of the bulk envelope. `observable`/`port`/etc. travel in the
/// small JSON envelope; the numeric arrays travel in `block`.
pub fn observation_from_bulk(
    port: impl Into<String>,
    target: impl Into<String>,
    observable: Observable,
    unit: Option<String>,
    recordable: Option<String>,
    block: &[u8],
) -> Result<Observation, BulkError> {
    let b = BulkBlock::decode(block)?;
    b.check_parallel()?; // cross-column length invariant: fail closed on a corrupt block
    let mut obs = Observation {
        port: port.into(),
        target: target.into(),
        observable,
        unit,
        recordable,
        ..Default::default()
    };
    obs.apply_bulk_block(&b);
    Ok(obs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_all_dtypes() {
        let b = BulkBlock::new()
            .with("a_f32", Column::F32(vec![1.5, -2.25, f32::MAX]))
            .with("b_f64", Column::F64(vec![1.0, 2.0, 3.5, -4.0]))
            .with("c_i32", Column::I32(vec![-1, 0, 7, i32::MIN]))
            .with("d_i64", Column::I64(vec![0, -9_000_000_000, i64::MAX]));
        let bytes = b.encode();
        let back = BulkBlock::decode(&bytes).unwrap();
        assert_eq!(b, back);
    }

    #[test]
    fn roundtrip_empty_block_and_empty_columns() {
        let empty = BulkBlock::new();
        assert_eq!(BulkBlock::decode(&empty.encode()).unwrap(), empty);

        let with_empty = BulkBlock::new()
            .with("times", Column::F64(vec![]))
            .with("senders", Column::I64(vec![]));
        assert_eq!(BulkBlock::decode(&with_empty.encode()).unwrap(), with_empty);
    }

    #[test]
    fn decode_rejects_unequal_parallel_columns() {
        // times has 3, senders has 2 -> corrupt parallel block -> fail closed.
        let bad = BulkBlock::new()
            .with("times", Column::F64(vec![0.0, 1.0, 2.0]))
            .with("senders", Column::I64(vec![1, 2]));
        let err =
            observation_from_bulk("spk", "pop", Observable::Spikes, None, None, &bad.encode())
                .unwrap_err();
        assert!(
            matches!(err, BulkError::ColumnLengthMismatch { .. }),
            "unequal parallel columns must fail closed, got {err:?}"
        );
        // Equal lengths still round-trip cleanly.
        let ok = BulkBlock::new()
            .with("times", Column::F64(vec![0.0, 1.0]))
            .with("senders", Column::I64(vec![1, 2]));
        assert!(
            observation_from_bulk("spk", "pop", Observable::Spikes, None, None, &ok.encode())
                .is_ok()
        );
    }

    #[test]
    fn preserves_non_finite_f64() {
        let b = BulkBlock::new().with(
            "vm",
            Column::F64(vec![f64::NAN, f64::INFINITY, f64::NEG_INFINITY, 0.0]),
        );
        let back = BulkBlock::decode(&b.encode()).unwrap();
        if let Some(Column::F64(v)) = back.get("vm") {
            assert!(v[0].is_nan());
            assert_eq!(v[1], f64::INFINITY);
            assert_eq!(v[2], f64::NEG_INFINITY);
            assert_eq!(v[3], 0.0);
        } else {
            panic!("vm column missing/wrong type");
        }
    }

    #[test]
    fn observation_spike_and_analog_roundtrip() {
        // Spike port: times + senders, no values.
        let spk = Observation {
            port: "spk".into(),
            target: "exc".into(),
            observable: Observable::Spikes,
            times: vec![1.0, 1.2, 5.7, 9.9],
            senders: vec![3, 3, 7, 12],
            ..Default::default()
        };
        let bytes = spk.to_bulk_bytes();
        let rebuilt =
            observation_from_bulk("spk", "exc", Observable::Spikes, None, None, &bytes).unwrap();
        assert_eq!(rebuilt.times, spk.times);
        assert_eq!(rebuilt.senders, spk.senders);
        assert!(rebuilt.values.is_empty());

        // Analog port: times + values, no senders.
        let vm = Observation {
            port: "vm".into(),
            target: "exc".into(),
            observable: Observable::Vm,
            times: vec![0.0, 1.0, 2.0],
            values: vec![-70.0, -69.5, -55.0],
            unit: Some("mV".into()),
            ..Default::default()
        };
        let mut back = BulkBlock::new();
        back = BulkBlock::decode(&vm.to_bulk_bytes()).unwrap_or(back);
        let mut rebuilt_vm = Observation::default();
        rebuilt_vm.apply_bulk_block(&back);
        assert_eq!(rebuilt_vm.times, vm.times);
        assert_eq!(rebuilt_vm.values, vm.values);
        assert!(rebuilt_vm.senders.is_empty());
    }

    #[test]
    fn compact_f32_widths_halve_senders_and_values() {
        // The issue's f32-values + i32-senders compaction path.
        let block = BulkBlock::new()
            .with("values", Column::F32(vec![1.0, 2.0, 3.0]))
            .with("senders", Column::I32(vec![1, 2, 3]));
        let bytes = block.encode();
        let mut obs = Observation::default();
        obs.apply_bulk_block(&BulkBlock::decode(&bytes).unwrap());
        assert_eq!(obs.values, vec![1.0, 2.0, 3.0]); // widened losslessly to f64
        assert_eq!(obs.senders, vec![1, 2, 3]); // widened to i64
    }

    #[test]
    fn rejects_bad_magic() {
        let mut bytes = BulkBlock::new().with("x", Column::F64(vec![1.0])).encode();
        bytes[0] = b'X';
        assert_eq!(BulkBlock::decode(&bytes), Err(BulkError::BadMagic));
    }

    #[test]
    fn rejects_truncation() {
        let bytes = BulkBlock::new()
            .with("x", Column::F64(vec![1.0, 2.0, 3.0]))
            .encode();
        // Drop the last 4 bytes: total_len no longer matches the buffer.
        let truncated = &bytes[..bytes.len() - 4];
        assert!(matches!(
            BulkBlock::decode(truncated),
            Err(BulkError::LengthMismatch { .. })
        ));
    }

    #[test]
    fn rejects_too_short() {
        assert_eq!(BulkBlock::decode(&[]), Err(BulkError::TooShort));
        assert_eq!(BulkBlock::decode(b"NCP"), Err(BulkError::TooShort));
    }

    #[test]
    fn rejects_allocation_bomb_nrows() {
        // Hand-craft a header claiming a single column with a huge n_rows but a
        // tiny buffer: the data-slice bounds check must reject it (no OOM).
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&BULK_MAGIC);
        bytes.push(BULK_VERSION);
        bytes.push(0);
        bytes.extend_from_slice(&1u16.to_le_bytes()); // n_cols = 1
        let total_len = (HEADER_LEN + DIR_ENTRY_LEN) as u32;
        bytes.extend_from_slice(&total_len.to_le_bytes());
        // directory entry: name_off=total_len(empty name), name_len=0, dtype=f64,
        // n_rows=u32::MAX, data_off=total_len
        bytes.extend_from_slice(&total_len.to_le_bytes()); // name_off
        bytes.extend_from_slice(&0u16.to_le_bytes()); // name_len
        bytes.push(DTYPE_F64);
        bytes.push(0);
        bytes.extend_from_slice(&u32::MAX.to_le_bytes()); // n_rows
        bytes.extend_from_slice(&total_len.to_le_bytes()); // data_off (== end)
        assert_eq!(bytes.len(), total_len as usize);
        // n_rows*8 points way past the buffer -> OutOfBounds, never an allocation.
        assert_eq!(BulkBlock::decode(&bytes), Err(BulkError::OutOfBounds));
    }

    #[test]
    fn rejects_bad_dtype() {
        let mut bytes = BulkBlock::new().with("x", Column::F64(vec![1.0])).encode();
        // dtype byte sits at directory entry offset 6.
        bytes[HEADER_LEN + 6] = 99;
        assert_eq!(BulkBlock::decode(&bytes), Err(BulkError::BadDtype(99)));
    }

    /// Cross-language byte-stability: the Rust encoder must produce the EXACT
    /// bytes of the committed conformance vector (`conformance/vectors/
    /// bulk_observation.bin`), which a Python peer also generates/decodes. This
    /// pins the on-wire layout so the f32/f64/i64 column block is interoperable,
    /// not merely self-consistent.
    #[test]
    fn matches_committed_golden_vector() {
        let golden = include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../conformance/vectors/bulk_observation.bin"
        ));
        let block = BulkBlock::new()
            .with("times", Column::F64(vec![1.5, 2.5, 9.0]))
            .with("senders", Column::I64(vec![7, 7, 9]));
        assert_eq!(
            block.encode().as_slice(),
            &golden[..],
            "Rust bulk encoding drifted from the committed golden vector"
        );
        assert_eq!(BulkBlock::decode(golden).unwrap(), block);
    }

    #[test]
    fn fifty_k_spikes_roundtrip() {
        // The motivating payload: 50k spikes (times + senders).
        let n = 50_000;
        let times: Vec<f64> = (0..n).map(|i| i as f64 * 0.1).collect();
        let senders: Vec<i64> = (0..n).map(|i| (i % 128) as i64).collect();
        let block = BulkBlock::new()
            .with("times", Column::F64(times.clone()))
            .with("senders", Column::I64(senders.clone()));
        let bytes = block.encode();
        // Header + dir + names + 50k*8 (times) + 50k*8 (senders).
        assert_eq!(bytes.len(), HEADER_LEN + 2 * DIR_ENTRY_LEN + 5 + 7 + n * 16);
        let back = BulkBlock::decode(&bytes).unwrap();
        assert_eq!(back.get("times").unwrap().as_f64(), times);
        assert_eq!(back.get("senders").unwrap().as_i64(), senders);
    }
}
