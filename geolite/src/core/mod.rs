//! Pure-Rust geometry primitives, EWKB I/O, and the canonical function
//! catalog used by the SQLite and Diesel layers to generate their surfaces.
//! No SQLite, Diesel, or wasm dependency at this level.

pub mod error;
pub mod ewkb;
pub mod function_catalog;
pub mod functions;
