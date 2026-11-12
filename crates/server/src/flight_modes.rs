//! FireDrone: time per flight mode, for the stats page.
//!
//! The converter's analysis splits each log into flight mode segments
//! (`vehicle_status.nav_state`, whole log). They live in each log's
//! metadata.json; the `log_flight_modes` table keeps their total per mode
//! so the stats can sum them over many logs. Uploads fill it; logs from
//! before it existed are filled from their metadata.json at startup
//! ([`backfill`]).

use axum::{
    extract::{Query, State},
    Json,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use uuid::Uuid;

use crate::api::ApiError;
use crate::db::{FlightModeStat, LogStore, StatsParams};
use crate::storage::FileStorage;

/// Seconds per flight mode, in order of first appearance. The converter calls
/// modes it doesn't know "Unknown"; those keep their number ("Mode 7").
pub fn time_per_mode<'a>(
    segments: impl IntoIterator<Item = (&'a str, u8, f64)>,
) -> Vec<(String, f64)> {
    let mut totals: Vec<(String, f64)> = Vec::new();
    for (name, id, duration_s) in segments {
        if !duration_s.is_finite() || duration_s <= 0.0 {
            continue;
        }
        let name = if name == "Unknown" {
            format!("Mode {id}")
        } else {
            name.to_string()
        };
        match totals.iter_mut().find(|(n, _)| *n == name) {
            Some((_, total)) => *total += duration_s,
            None => totals.push((name, duration_s)),
        }
    }
    totals
}

/// Same as [`time_per_mode`], from the converter's segments.
pub fn from_analysis(
    segments: &[flight_review::analysis::FlightModeSegment],
) -> Vec<(String, f64)> {
    time_per_mode(
        segments
            .iter()
            .map(|s| (s.mode.as_str(), s.mode_id, s.duration_s)),
    )
}

/// Fill `log_flight_modes` for logs that have no rows yet, from their
/// metadata.json. Runs in the background at startup; logs without flight
/// modes (or without metadata) are looked at again at the next start.
pub async fn backfill(db: Arc<dyn LogStore>, storage: Arc<FileStorage>) {
    let ids = match db.logs_without_flight_modes().await {
        Ok(ids) => ids,
        Err(e) => {
            tracing::warn!("flight modes: can't list logs to fill in: {e}");
            return;
        }
    };
    if ids.is_empty() {
        return;
    }
    let (mut filled, mut without) = (0, 0);
    for id in ids {
        match from_metadata(&storage, id).await {
            Ok(modes) if !modes.is_empty() => match db.insert_flight_modes(id, &modes).await {
                Ok(()) => filled += 1,
                Err(e) => tracing::warn!(log_id = %id, "flight modes: {e}"),
            },
            Ok(_) => without += 1,
            Err(e) => {
                tracing::debug!(log_id = %id, "flight modes: {e}");
                without += 1;
            }
        }
    }
    tracing::info!(
        "flight modes: filled in {filled} logs, {without} without flight modes or metadata"
    );
}

async fn from_metadata(storage: &FileStorage, id: Uuid) -> Result<Vec<(String, f64)>, String> {
    // Only the part needed, so metadata from older converter versions still reads.
    #[derive(Deserialize)]
    struct Metadata {
        analysis: Option<Analysis>,
    }
    #[derive(Deserialize)]
    struct Analysis {
        #[serde(default)]
        flight_modes: Vec<Segment>,
    }
    #[derive(Deserialize)]
    struct Segment {
        mode: String,
        #[serde(default)]
        mode_id: u8,
        duration_s: f64,
    }

    let bytes = storage
        .get_file(id, "metadata.json")
        .await
        .map_err(|e| format!("metadata.json: {e}"))?;
    let meta: Metadata =
        serde_json::from_slice(&bytes).map_err(|e| format!("metadata.json: {e}"))?;
    let segments = meta.analysis.map(|a| a.flight_modes).unwrap_or_default();
    Ok(time_per_mode(
        segments
            .iter()
            .map(|s| (s.mode.as_str(), s.mode_id, s.duration_s)),
    ))
}

/// Filters of `GET /api/stats/flight-modes`, as for `/api/stats`.
#[derive(Debug, Deserialize)]
pub struct FlightModeQuery {
    pub period: Option<String>,
    pub vehicle_type: Option<String>,
    pub ver_hw: Option<String>,
    pub ver_sw_release_str: Option<String>,
    pub source: Option<String>,
    pub vibration_status: Option<String>,
    pub vehicle_name: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct FlightModeResponse {
    pub period: String,
    pub data: Vec<FlightModeStat>,
}

/// `GET /api/stats/flight-modes`: time per flight mode over the public logs
/// that match the filters, most time first.
pub async fn get_stats(
    State(state): State<Arc<crate::AppState>>,
    Query(q): Query<FlightModeQuery>,
) -> Result<Json<FlightModeResponse>, ApiError> {
    let period = q.period.clone().unwrap_or_else(|| "30d".to_string());
    let params = StatsParams {
        group_by: String::new(),
        period: q.period,
        limit: None,
        vehicle_type: q.vehicle_type,
        ver_hw: q.ver_hw,
        ver_sw_release_str: q.ver_sw_release_str,
        source: q.source,
        vibration_status: q.vibration_status,
        vehicle_name: q.vehicle_name,
    };
    let data = state.db.flight_mode_stats(&params).await?;
    Ok(Json(FlightModeResponse { period, data }))
}

#[cfg(test)]
mod tests {
    use super::time_per_mode;

    #[test]
    fn sums_time_per_mode() {
        let segments = [
            ("Position", 2, 10.0),
            ("Offboard", 14, 5.5),
            ("Position", 2, 2.5),
            ("Unknown", 7, 1.0),
            ("Land", 18, 0.0),       // empty segments don't count
            ("Manual", 0, f64::NAN), // nor broken ones
        ];
        assert_eq!(
            time_per_mode(segments),
            vec![
                ("Position".to_string(), 12.5),
                ("Offboard".to_string(), 5.5),
                ("Mode 7".to_string(), 1.0),
            ]
        );
    }
}
