//! Message types exchanged between the UI bundle and the dedicated SQLite
//! worker bundle of the SQLiteGIS web demo.
//!
//! Both sides serialise these via `serde_wasm_bindgen` so they survive
//! `postMessage` between the main thread and the worker. The variants are
//! intentionally small (no Diesel / sqlite-wasm-rs types leak across the
//! boundary): rows arrive as `Vec<Vec<String>>` already rendered for the
//! results table.

use serde::{Deserialize, Serialize};

/// A pair of `(lon, lat)` in WGS 84 degrees.
pub type LonLat = (f64, f64);

/// Geometry + scalar row payload returned by a `SELECT` query.
///
/// `columns` is the projected column list in declaration order. `rows` holds
/// every cell already rendered to its display string (NULL becomes the
/// literal `"NULL"`, BLOBs become `"<N bytes>"`, etc.). Mirrors what the old
/// `runner::QueryRows` exposed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueryRows {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<String>>,
}

/// Result of running one SQL script against the worker connection.
///
/// Mirrors the old `runner::QueryOutcome`. The UI projects this directly
/// onto the results panel without any further worker interaction.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum QueryOutcome {
    Rows { result: QueryRows, elapsed_ms: f64 },
    Affected { rows: i64, elapsed_ms: f64 },
    Error(String),
}

/// Summary of a completed dataset load.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct LoadReport {
    pub rows_inserted: usize,
    pub elapsed_ms: f64,
}

/// Messages posted from the UI thread to the worker.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum WorkerRequest {
    /// Reopen the in-memory connection and apply a schema script.
    ApplySchema { token: u64, sql: String },
    /// Stream the cities5000 TSV from `tsv_url` into the `places` table.
    LoadDataset { token: u64, tsv_url: String },
    /// Run one SQL script with `:lon` / `:lat` substituted from the probe.
    RunQuery {
        token: u64,
        sql: String,
        lon: f64,
        lat: f64,
    },
}

impl WorkerRequest {
    /// Token carried by every variant. Used by the UI to match responses to
    /// the awaiting caller.
    #[must_use]
    pub fn token(&self) -> u64 {
        match *self {
            Self::ApplySchema { token, .. }
            | Self::LoadDataset { token, .. }
            | Self::RunQuery { token, .. } => token,
        }
    }
}

/// Messages posted from the worker back to the UI thread.
///
/// `Ready` is announced once the worker installs its message loop. Every
/// other variant carries the originating `token` so the UI can route the
/// response to the right awaiting caller.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum WorkerResponse {
    Ready,
    SchemaApplied {
        token: u64,
    },
    LoadProgress {
        token: u64,
        inserted: usize,
        total: usize,
        batch: Vec<LonLat>,
    },
    LoadComplete {
        token: u64,
        report: LoadReport,
    },
    QueryRows {
        token: u64,
        result: QueryRows,
        elapsed_ms: f64,
    },
    QueryAffected {
        token: u64,
        rows: i64,
        elapsed_ms: f64,
    },
    Error {
        token: u64,
        message: String,
    },
}

impl WorkerResponse {
    /// Token carried by the response. `Ready` uses sentinel `0`.
    #[must_use]
    pub const fn token(&self) -> u64 {
        match *self {
            Self::Ready => 0,
            Self::SchemaApplied { token }
            | Self::LoadProgress { token, .. }
            | Self::LoadComplete { token, .. }
            | Self::QueryRows { token, .. }
            | Self::QueryAffected { token, .. }
            | Self::Error { token, .. } => token,
        }
    }

    /// True when this is a terminal response (the awaiting caller should
    /// drop their token slot). `LoadProgress` is non-terminal; everything
    /// else is.
    #[must_use]
    pub const fn is_terminal(&self) -> bool {
        !matches!(*self, Self::LoadProgress { .. } | Self::Ready)
    }
}
