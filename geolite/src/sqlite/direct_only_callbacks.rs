// Hand-maintained. Each row must correspond 1:1 to an entry in
// crate::core::function_catalog::SQLITE_DIRECT_ONLY_FUNCTIONS. The
// assert_catalog_callback_parity const-assertion in ffi.rs verifies this at
// compile time.

const SQLITE_DIRECT_ONLY_CALLBACKS: &[SqliteCallbackSpec] = &[
    callback_spec!("CreateSpatialIndex", 2, create_spatial_index_xfunc),
    callback_spec!("DropSpatialIndex", 2, drop_spatial_index_xfunc),
];
