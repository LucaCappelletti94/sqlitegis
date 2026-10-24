//! Pure-Rust geometry primitives, EWKB I/O, and the canonical function
//! catalog used by the SQLite and Diesel layers to generate their surfaces.
//! No SQLite, Diesel, or wasm dependency at this level.

/// Crate-wide error and result types returned by every fallible function.
pub mod error;
/// EWKB (Extended Well-Known Binary) wire format encoder and decoder, used
/// as the on-disk and over-the-wire representation for geometry BLOBs.
pub mod ewkb;
/// Authoritative catalog of every SQL function the crate exposes, used by
/// the SQLite and Diesel layers to keep their surfaces in sync.
pub mod function_catalog;
/// Pure-Rust implementations of the spatial functions in the catalog,
/// operating on EWKB BLOBs and primitive scalars.
pub mod functions;
/// The `geo` types geometry values hold, any geometry or one of its
/// variants.
pub mod geometry_kind;
