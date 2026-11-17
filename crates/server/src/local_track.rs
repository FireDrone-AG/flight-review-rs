//! FireDrone: a track for flights without GPS.
//!
//! Browse draws each flight's track from its GPS fixes. Drones that fly on
//! visual odometry have no fix, but the estimator's local position
//! (`vehicle_local_position` x and y: metres north and east of where the
//! estimator started) has the same shape. It's read from the log's converted
//! Parquet file, so every stored log has one without being converted again.

use arrow::array::{ArrayRef, AsArray, RecordBatch};
use arrow::compute::cast;
use arrow::datatypes::{DataType, Float64Type, UInt64Type};
use bytes::Bytes;
use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
use parquet::arrow::ProjectionMask;
use serde::Serialize;
use uuid::Uuid;

use crate::storage::FileStorage;

/// At most this many points per track: plenty for a thumbnail.
const MAX_POINTS: usize = 150;

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct LocalPoint {
    /// Metres north of the estimator's origin.
    pub x: f32,
    /// Metres east.
    pub y: f32,
    /// Flight mode (nav_state), as in the GPS track.
    pub m: u8,
    /// Starts a new line: the estimator reset its position, or it wasn't
    /// valid for a while. Joining across would draw a jump that never happened.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub b: bool,
}

/// The local track of a log, from its `vehicle_local_position.parquet` and
/// the flight modes in its metadata.json. Empty if the log has neither GPS
/// nor a valid local position.
pub async fn load(storage: &FileStorage, id: Uuid, meta: &serde_json::Value) -> Vec<LocalPoint> {
    let data = match storage.get_file(id, "vehicle_local_position.parquet").await {
        Ok(data) => data,
        // Not logged, or a v1 log not converted yet
        Err(_) => return Vec::new(),
    };
    let modes = mode_changes(meta);
    match tokio::task::spawn_blocking(move || read(data, &modes)).await {
        Ok(Ok(points)) => downsample(points, MAX_POINTS),
        Ok(Err(e)) => {
            tracing::warn!("local track of {id}: {e}");
            Vec::new()
        }
        Err(e) => {
            tracing::warn!("local track of {id}: {e}");
            Vec::new()
        }
    }
}

/// (start µs, mode) of each flight mode segment, in time order.
fn mode_changes(meta: &serde_json::Value) -> Vec<(u64, u8)> {
    meta.pointer("/analysis/flight_modes")
        .and_then(|v| v.as_array())
        .map(|segments| {
            segments
                .iter()
                .filter_map(|s| {
                    Some((
                        s.get("start_us")?.as_u64()?,
                        s.get("mode_id")?.as_u64().unwrap_or(0) as u8,
                    ))
                })
                .collect()
        })
        .unwrap_or_default()
}

fn mode_at(modes: &[(u64, u8)], ts: u64) -> u8 {
    modes
        .iter()
        .rev()
        .find(|(start, _)| *start <= ts)
        .or(modes.first())
        .map(|(_, m)| *m)
        .unwrap_or(0)
}

fn read(data: Bytes, modes: &[(u64, u8)]) -> Result<Vec<LocalPoint>, String> {
    const COLUMNS: [&str; 5] = ["timestamp", "x", "y", "xy_valid", "xy_reset_counter"];
    let builder = ParquetRecordBatchReaderBuilder::try_new(data).map_err(|e| e.to_string())?;
    let schema = builder.parquet_schema();
    let leaves = (0..schema.num_columns())
        .filter(|&i| COLUMNS.contains(&schema.column(i).path().string().as_str()));
    let mask = ProjectionMask::leaves(schema, leaves);
    let reader = builder
        .with_projection(mask)
        .build()
        .map_err(|e| e.to_string())?;

    let mut points = Vec::new();
    let mut new_line = false;
    let mut last_reset: Option<u64> = None;
    for batch in reader {
        let batch = batch.map_err(|e| e.to_string())?;
        let (Some(ts), Some(x), Some(y)) = (
            column(&batch, "timestamp", &DataType::UInt64)?,
            column(&batch, "x", &DataType::Float64)?,
            column(&batch, "y", &DataType::Float64)?,
        ) else {
            return Ok(Vec::new());
        };
        // column() cast them, so these can't fail
        let (ts, x, y) = (
            ts.as_primitive::<UInt64Type>(),
            x.as_primitive::<Float64Type>(),
            y.as_primitive::<Float64Type>(),
        );
        // Both are in every PX4 version we know; without them, every
        // position counts as valid and there are no resets.
        let valid = column(&batch, "xy_valid", &DataType::Boolean)?;
        let valid = valid.as_ref().map(|a| a.as_boolean());
        let reset = column(&batch, "xy_reset_counter", &DataType::UInt64)?;
        let reset = reset.as_ref().map(|a| a.as_primitive::<UInt64Type>());

        for i in 0..batch.num_rows() {
            let (px, py) = (x.value(i), y.value(i));
            if valid.is_some_and(|v| !v.value(i)) || !px.is_finite() || !py.is_finite() {
                new_line = true;
                continue;
            }
            if let Some(reset) = reset {
                let r = reset.value(i);
                if last_reset.is_some_and(|last| last != r) {
                    new_line = true;
                }
                last_reset = Some(r);
            }
            let t = ts.value(i);
            points.push(LocalPoint {
                x: px as f32,
                y: py as f32,
                m: mode_at(modes, t),
                b: new_line && !points.is_empty(),
            });
            new_line = false;
        }
    }
    Ok(points)
}

/// A column cast to `to`, or `None` if the file doesn't have it.
fn column(batch: &RecordBatch, name: &str, to: &DataType) -> Result<Option<ArrayRef>, String> {
    match batch.column_by_name(name) {
        Some(col) => cast(col, to).map(Some).map_err(|e| format!("{name}: {e}")),
        None => Ok(None),
    }
}

/// Evenly spaced points, first and last kept. A kept point starts a new line
/// if any point dropped before it did.
fn downsample(points: Vec<LocalPoint>, max: usize) -> Vec<LocalPoint> {
    if points.len() <= max || max < 2 {
        return points;
    }
    let step = (points.len() - 1) as f64 / (max - 1) as f64;
    let mut out = Vec::with_capacity(max);
    out.push(points[0].clone());
    let mut prev = 0;
    for i in 1..max {
        let idx = if i == max - 1 {
            points.len() - 1
        } else {
            (i as f64 * step).round() as usize
        };
        let mut p = points[idx].clone();
        p.b = points[prev + 1..=idx].iter().any(|q| q.b);
        out.push(p);
        prev = idx;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use arrow::array::{BooleanArray, Float32Array, UInt64Array, UInt8Array};
    use arrow::datatypes::{Field, Schema};
    use parquet::arrow::ArrowWriter;
    use std::sync::Arc;

    fn parquet(rows: &[(u64, f32, f32, bool, u8)]) -> Bytes {
        let schema = Arc::new(Schema::new(vec![
            Field::new("timestamp", DataType::UInt64, false),
            Field::new("x", DataType::Float32, false),
            Field::new("xy_global", DataType::Boolean, false),
            Field::new("xy_reset_counter", DataType::UInt8, false),
            Field::new("xy_valid", DataType::Boolean, false),
            Field::new("y", DataType::Float32, false),
        ]));
        let batch = RecordBatch::try_new(
            schema.clone(),
            vec![
                Arc::new(UInt64Array::from_iter_values(rows.iter().map(|r| r.0))),
                Arc::new(Float32Array::from_iter_values(rows.iter().map(|r| r.1))),
                Arc::new(BooleanArray::from(vec![false; rows.len()])),
                Arc::new(UInt8Array::from_iter_values(rows.iter().map(|r| r.4))),
                Arc::new(BooleanArray::from(
                    rows.iter().map(|r| r.3).collect::<Vec<_>>(),
                )),
                Arc::new(Float32Array::from_iter_values(rows.iter().map(|r| r.2))),
            ],
        )
        .unwrap();
        let mut buf = Vec::new();
        let mut writer = ArrowWriter::try_new(&mut buf, schema, None).unwrap();
        writer.write(&batch).unwrap();
        writer.close().unwrap();
        Bytes::from(buf)
    }

    fn pt(x: f32, y: f32, m: u8, b: bool) -> LocalPoint {
        LocalPoint { x, y, m, b }
    }

    #[test]
    fn reads_valid_positions_with_modes() {
        let data = parquet(&[
            (1_000, 0.0, 0.0, true, 3),
            (2_000, 1.0, 0.5, true, 3),
            (3_000, 2.0, 1.0, true, 3),
        ]);
        // Manual from the start, Position from 2 ms
        let points = read(data, &[(500, 0), (2_000, 2)]).unwrap();
        assert_eq!(
            points,
            vec![
                pt(0.0, 0.0, 0, false),
                pt(1.0, 0.5, 2, false),
                pt(2.0, 1.0, 2, false)
            ]
        );
    }

    #[test]
    fn breaks_the_line_at_resets_and_invalid_positions() {
        let data = parquet(&[
            (1, 0.0, 0.0, false, 3), // not valid yet: skipped, no break before the first point
            (2, 1.0, 0.0, true, 3),
            (3, 2.0, 0.0, true, 3),
            (4, 9.0, 9.0, false, 3), // invalid: skipped
            (5, 3.0, 0.0, true, 3),  // new line after it
            (6, 4.0, 0.0, true, 3),
            (7, -5.0, 2.0, true, 4), // reset: new line
            (8, -4.0, 2.0, true, 4),
        ]);
        let b: Vec<bool> = read(data, &[]).unwrap().iter().map(|p| p.b).collect();
        assert_eq!(b, vec![false, false, true, false, true, false]);
    }

    #[test]
    fn downsampling_keeps_ends_and_breaks() {
        let mut points: Vec<LocalPoint> = (0..1000).map(|i| pt(i as f32, 0.0, 0, false)).collect();
        points[501].b = true; // likely dropped; the next kept point must start a line
        let out = downsample(points, 100);
        assert_eq!(out.len(), 100);
        assert_eq!(out[0].x, 0.0);
        assert_eq!(out[99].x, 999.0);
        assert_eq!(out.iter().filter(|p| p.b).count(), 1);
        let first_after = out.iter().position(|p| p.x >= 501.0).unwrap();
        assert!(out[first_after].b);
    }

    #[test]
    fn json_leaves_out_b_when_false() {
        let json =
            serde_json::to_string(&[pt(1.5, -2.0, 2, false), pt(0.0, 0.0, 2, true)]).unwrap();
        assert_eq!(
            json,
            r#"[{"x":1.5,"y":-2.0,"m":2},{"x":0.0,"y":0.0,"m":2,"b":true}]"#
        );
    }
}
