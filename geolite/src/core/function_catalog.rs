//! Canonical SQLite function catalog shared across adapters.

/// Coarse SQLite output class used by registration smoke tests.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SqliteReturnClass {
    Numeric,
    Text,
    Blob,
    Bool,
}

/// Expected semantic outcome for a SQL case.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SemanticExpectation {
    Null,
    NumericFinite,
    TextNonEmpty,
    BlobNonEmpty,
    Bool01,
    ErrorContains(&'static str),
}

/// Canonical semantic SQL case.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SemanticCase {
    pub id: &'static str,
    pub sql: &'static str,
    pub expected: SemanticExpectation,
}

/// Canonical SQLite function declaration metadata.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SqliteFunctionSpec {
    pub name: &'static str,
    pub n_arg: i32,
    pub return_class: SqliteReturnClass,
    /// Semantic smoke SQL used to validate that callback wiring matches
    /// signature + return class (not just NULL short-circuit behavior).
    pub smoke_sql: &'static str,
    /// Semantic-golden SQL cases used by cross-surface parity tests.
    pub semantic_cases: &'static [SemanticCase],
    /// Explicit Rust `*_xfunc` symbol to bind for this `(name, n_arg)`. `None`
    /// means the SQLite callback generator derives the symbol from `name`
    /// (lowercased, with an arity suffix when the same SQL name is overloaded).
    /// Used for aliases like `ST_MakePoint -> st_point_2_xfunc`.
    pub xfunc_override: Option<&'static str>,
}

macro_rules! case {
    ($id:literal, $sql:expr, $expected:expr) => {
        SemanticCase {
            id: $id,
            sql: $sql,
            expected: $expected,
        }
    };
}

macro_rules! null_sql {
    ($name:literal, 1) => {
        concat!("SELECT ", $name, "(NULL)")
    };
    ($name:literal, 2) => {
        concat!("SELECT ", $name, "(NULL, NULL)")
    };
    ($name:literal, 3) => {
        concat!("SELECT ", $name, "(NULL, NULL, NULL)")
    };
    ($name:literal, 4) => {
        concat!("SELECT ", $name, "(NULL, NULL, NULL, NULL)")
    };
    ($name:literal, 5) => {
        concat!("SELECT ", $name, "(NULL, NULL, NULL, NULL, NULL)")
    };
}

macro_rules! expected_for_return_class {
    (Numeric) => {
        SemanticExpectation::NumericFinite
    };
    (Text) => {
        SemanticExpectation::TextNonEmpty
    };
    (Blob) => {
        SemanticExpectation::BlobNonEmpty
    };
    (Bool) => {
        SemanticExpectation::Bool01
    };
}

macro_rules! spec {
    ($name:literal, $n_arg:tt, $return_class:ident, $smoke_sql:literal) => {
        SqliteFunctionSpec {
            name: $name,
            n_arg: $n_arg,
            return_class: SqliteReturnClass::$return_class,
            smoke_sql: $smoke_sql,
            semantic_cases: &[
                case!(
                    "smoke",
                    $smoke_sql,
                    expected_for_return_class!($return_class)
                ),
                case!(
                    "null_input",
                    null_sql!($name, $n_arg),
                    SemanticExpectation::Null
                ),
            ],
            xfunc_override: None,
        }
    };
}

macro_rules! spec_override {
    (
        $name:literal,
        $n_arg:tt,
        $return_class:ident,
        $smoke_sql:literal,
        $xfunc:literal
    ) => {
        SqliteFunctionSpec {
            name: $name,
            n_arg: $n_arg,
            return_class: SqliteReturnClass::$return_class,
            smoke_sql: $smoke_sql,
            semantic_cases: &[
                case!(
                    "smoke",
                    $smoke_sql,
                    expected_for_return_class!($return_class)
                ),
                case!(
                    "null_input",
                    null_sql!($name, $n_arg),
                    SemanticExpectation::Null
                ),
            ],
            xfunc_override: Some($xfunc),
        }
    };
}

macro_rules! direct_spec {
    (
        $name:literal,
        $n_arg:tt,
        $return_class:ident,
        $smoke_sql:literal,
        $null_sql:literal,
        $null_error_contains:literal,
        $xfunc:literal
    ) => {
        SqliteFunctionSpec {
            name: $name,
            n_arg: $n_arg,
            return_class: SqliteReturnClass::$return_class,
            smoke_sql: $smoke_sql,
            semantic_cases: &[
                case!(
                    "smoke",
                    $smoke_sql,
                    expected_for_return_class!($return_class)
                ),
                case!(
                    "null_input_error",
                    $null_sql,
                    SemanticExpectation::ErrorContains($null_error_contains)
                ),
            ],
            xfunc_override: Some($xfunc),
        }
    };
}

pub const SQLITE_DETERMINISTIC_FUNCTIONS: &[SqliteFunctionSpec] = &[
    // I/O
    spec!(
        "ST_GeomFromText",
        1,
        Blob,
        "SELECT ST_GeomFromText('POINT(1 2)')"
    ),
    spec!(
        "ST_GeomFromText",
        2,
        Blob,
        "SELECT ST_GeomFromText('POINT(1 2)', 4326)"
    ),
    spec!(
        "ST_GeomFromWKB",
        1,
        Blob,
        "SELECT ST_GeomFromWKB(ST_AsBinary(ST_Point(1, 2)))"
    ),
    spec!(
        "ST_GeomFromWKB",
        2,
        Blob,
        "SELECT ST_GeomFromWKB(ST_AsBinary(ST_Point(1, 2)), 4326)"
    ),
    spec!(
        "ST_GeomFromEWKB",
        1,
        Blob,
        "SELECT ST_GeomFromEWKB(ST_AsEWKB(ST_Point(1, 2, 4326)))"
    ),
    // PostGIS parity: GeoJSON parser is one-argument only.
    // Use ST_SetSRID(ST_GeomFromGeoJSON(...), srid) to override 4326.
    spec!(
        "ST_GeomFromGeoJSON",
        1,
        Blob,
        "SELECT ST_GeomFromGeoJSON('{\"type\":\"Point\",\"coordinates\":[1,2]}')"
    ),
    spec!("ST_AsText", 1, Text, "SELECT ST_AsText(ST_Point(1, 2))"),
    spec!(
        "ST_AsEWKT",
        1,
        Text,
        "SELECT ST_AsEWKT(ST_Point(1, 2, 4326))"
    ),
    spec!(
        "ST_AsBinary",
        1,
        Blob,
        "SELECT ST_AsBinary(ST_Point(1, 2))"
    ),
    spec!(
        "ST_AsEWKB",
        1,
        Blob,
        "SELECT ST_AsEWKB(ST_Point(1, 2, 4326))"
    ),
    spec!(
        "ST_AsGeoJSON",
        1,
        Text,
        "SELECT ST_AsGeoJSON(ST_Point(1, 2))"
    ),
    // Constructors
    spec!("ST_Point", 2, Blob, "SELECT ST_Point(1, 2)"),
    spec!("ST_Point", 3, Blob, "SELECT ST_Point(1, 2, 4326)"),
    spec_override!(
        "ST_MakePoint",
        2,
        Blob,
        "SELECT ST_MakePoint(1, 2)",
        "st_point_2_xfunc"
    ),
    spec!(
        "ST_MakeLine",
        2,
        Blob,
        "SELECT ST_MakeLine(ST_Point(0, 0), ST_Point(1, 1))"
    ),
    spec!(
        "ST_MakePolygon",
        1,
        Blob,
        "SELECT ST_MakePolygon(ST_GeomFromText('LINESTRING(0 0,1 0,1 1,0 1,0 0)'))"
    ),
    spec!(
        "ST_MakeEnvelope",
        4,
        Blob,
        "SELECT ST_MakeEnvelope(0, 0, 1, 1)"
    ),
    spec!(
        "ST_MakeEnvelope",
        5,
        Blob,
        "SELECT ST_MakeEnvelope(0, 0, 1, 1, 4326)"
    ),
    spec!(
        "ST_Collect",
        2,
        Blob,
        "SELECT ST_Collect(ST_Point(0, 0), ST_Point(1, 1))"
    ),
    spec!(
        "ST_TileEnvelope",
        3,
        Blob,
        "SELECT ST_TileEnvelope(1, 0, 0)"
    ),
    // Accessors
    spec!("ST_SRID", 1, Numeric, "SELECT ST_SRID(ST_Point(1, 2, 4326))"),
    spec!(
        "ST_SetSRID",
        2,
        Blob,
        "SELECT ST_SetSRID(ST_Point(1, 2), 3857)"
    ),
    spec!(
        "ST_GeometryType",
        1,
        Text,
        "SELECT ST_GeometryType(ST_Point(1, 2))"
    ),
    spec_override!(
        "GeometryType",
        1,
        Text,
        "SELECT GeometryType(ST_Point(1, 2))",
        "st_geometrytype_xfunc"
    ),
    spec!("ST_NDims", 1, Numeric, "SELECT ST_NDims(ST_Point(1, 2))"),
    spec!("ST_CoordDim", 1, Numeric, "SELECT ST_CoordDim(ST_Point(1, 2))"),
    spec!("ST_Zmflag", 1, Numeric, "SELECT ST_Zmflag(ST_Point(1, 2))"),
    spec!("ST_IsEmpty", 1, Bool, "SELECT ST_IsEmpty(ST_Point(1, 2))"),
    spec!("ST_MemSize", 1, Numeric, "SELECT ST_MemSize(ST_Point(1, 2))"),
    spec!("ST_X", 1, Numeric, "SELECT ST_X(ST_Point(1, 2))"),
    spec!("ST_Y", 1, Numeric, "SELECT ST_Y(ST_Point(1, 2))"),
    spec!(
        "ST_Z",
        1,
        Numeric,
        "SELECT ST_Z(X'0101000080000000000000F03F00000000000000400000000000000840')"
    ),
    spec!(
        "ST_NumPoints",
        1,
        Numeric,
        "SELECT ST_NumPoints(ST_GeomFromText('LINESTRING(0 0,1 1)'))"
    ),
    spec!(
        "ST_NPoints",
        1,
        Numeric,
        "SELECT ST_NPoints(ST_GeomFromText('LINESTRING(0 0,1 1,2 2)'))"
    ),
    spec!(
        "ST_NumGeometries",
        1,
        Numeric,
        "SELECT ST_NumGeometries(ST_GeomFromText('MULTIPOINT((0 0),(1 1))'))"
    ),
    spec!(
        "ST_NumInteriorRings",
        1,
        Numeric,
        "SELECT ST_NumInteriorRings(ST_GeomFromText('POLYGON((0 0,3 0,3 3,0 3,0 0),(1 1,2 1,2 2,1 2,1 1))'))"
    ),
    spec_override!(
        "ST_NumInteriorRing",
        1,
        Numeric,
        "SELECT ST_NumInteriorRing(ST_GeomFromText('POLYGON((0 0,3 0,3 3,0 3,0 0),(1 1,2 1,2 2,1 2,1 1))'))",
        "st_numinteriorrings_xfunc"
    ),
    spec!(
        "ST_NumRings",
        1,
        Numeric,
        "SELECT ST_NumRings(ST_GeomFromText('POLYGON((0 0,3 0,3 3,0 3,0 0),(1 1,2 1,2 2,1 2,1 1))'))"
    ),
    spec!(
        "ST_PointN",
        2,
        Blob,
        "SELECT ST_PointN(ST_GeomFromText('LINESTRING(0 0,1 1,2 2)'), 2)"
    ),
    spec!(
        "ST_StartPoint",
        1,
        Blob,
        "SELECT ST_StartPoint(ST_GeomFromText('LINESTRING(0 0,1 1,2 2)'))"
    ),
    spec!(
        "ST_EndPoint",
        1,
        Blob,
        "SELECT ST_EndPoint(ST_GeomFromText('LINESTRING(0 0,1 1,2 2)'))"
    ),
    spec!(
        "ST_ExteriorRing",
        1,
        Blob,
        "SELECT ST_ExteriorRing(ST_GeomFromText('POLYGON((0 0,2 0,2 2,0 2,0 0))'))"
    ),
    spec!(
        "ST_InteriorRingN",
        2,
        Blob,
        "SELECT ST_InteriorRingN(ST_GeomFromText('POLYGON((0 0,3 0,3 3,0 3,0 0),(1 1,2 1,2 2,1 2,1 1))'), 1)"
    ),
    spec!(
        "ST_GeometryN",
        2,
        Blob,
        "SELECT ST_GeometryN(ST_GeomFromText('GEOMETRYCOLLECTION(POINT(0 0),LINESTRING(0 0,1 1))'), 2)"
    ),
    spec!(
        "ST_Dimension",
        1,
        Numeric,
        "SELECT ST_Dimension(ST_GeomFromText('LINESTRING(0 0,1 1)'))"
    ),
    spec!(
        "ST_Envelope",
        1,
        Blob,
        "SELECT ST_Envelope(ST_GeomFromText('LINESTRING(0 0,1 1)'))"
    ),
    spec!(
        "ST_IsValid",
        1,
        Bool,
        "SELECT ST_IsValid(ST_GeomFromText('POLYGON((0 0,1 0,1 1,0 1,0 0))'))"
    ),
    spec!(
        "ST_IsValidReason",
        1,
        Text,
        "SELECT ST_IsValidReason(ST_GeomFromText('POLYGON((0 0,1 0,1 1,0 1,0 0))'))"
    ),
    // Measurement
    spec!(
        "ST_Area",
        1,
        Numeric,
        "SELECT ST_Area(ST_GeomFromText('POLYGON((0 0,1 0,1 1,0 1,0 0))'))"
    ),
    spec!(
        "ST_Length",
        1,
        Numeric,
        "SELECT ST_Length(ST_GeomFromText('LINESTRING(0 0,1 1)'))"
    ),
    spec_override!(
        "ST_Length2D",
        1,
        Numeric,
        "SELECT ST_Length2D(ST_GeomFromText('LINESTRING(0 0,1 1)'))",
        "st_length_xfunc"
    ),
    spec!(
        "ST_Perimeter",
        1,
        Numeric,
        "SELECT ST_Perimeter(ST_GeomFromText('POLYGON((0 0,1 0,1 1,0 1,0 0))'))"
    ),
    spec_override!(
        "ST_Perimeter2D",
        1,
        Numeric,
        "SELECT ST_Perimeter2D(ST_GeomFromText('POLYGON((0 0,1 0,1 1,0 1,0 0))'))",
        "st_perimeter_xfunc"
    ),
    spec!(
        "ST_Distance",
        2,
        Numeric,
        "SELECT ST_Distance(ST_Point(0, 0), ST_Point(3, 4))"
    ),
    spec!(
        "ST_Centroid",
        1,
        Blob,
        "SELECT ST_Centroid(ST_GeomFromText('POLYGON((0 0,2 0,2 2,0 2,0 0))'))"
    ),
    spec!(
        "ST_PointOnSurface",
        1,
        Blob,
        "SELECT ST_PointOnSurface(ST_GeomFromText('POLYGON((0 0,2 0,2 2,0 2,0 0))'))"
    ),
    spec!(
        "ST_HausdorffDistance",
        2,
        Numeric,
        "SELECT ST_HausdorffDistance(ST_GeomFromText('LINESTRING(0 0,1 0)'), ST_GeomFromText('LINESTRING(0 1,1 1)'))"
    ),
    spec!(
        "ST_XMin",
        1,
        Numeric,
        "SELECT ST_XMin(ST_GeomFromText('POLYGON((0 0,2 0,2 2,0 2,0 0))'))"
    ),
    spec!(
        "ST_XMax",
        1,
        Numeric,
        "SELECT ST_XMax(ST_GeomFromText('POLYGON((0 0,2 0,2 2,0 2,0 0))'))"
    ),
    spec!(
        "ST_YMin",
        1,
        Numeric,
        "SELECT ST_YMin(ST_GeomFromText('POLYGON((0 0,2 0,2 2,0 2,0 0))'))"
    ),
    spec!(
        "ST_YMax",
        1,
        Numeric,
        "SELECT ST_YMax(ST_GeomFromText('POLYGON((0 0,2 0,2 2,0 2,0 0))'))"
    ),
    spec!(
        "ST_DistanceSphere",
        2,
        Numeric,
        "SELECT ST_DistanceSphere(ST_Point(0, 0, 4326), ST_Point(0, 1, 4326))"
    ),
    spec!(
        "ST_DistanceSpheroid",
        2,
        Numeric,
        "SELECT ST_DistanceSpheroid(ST_Point(0, 0, 4326), ST_Point(0, 1, 4326))"
    ),
    spec!(
        "ST_LengthSphere",
        1,
        Numeric,
        "SELECT ST_LengthSphere(ST_GeomFromText('LINESTRING(0 0,0 1)', 4326))"
    ),
    spec!(
        "ST_Azimuth",
        2,
        Numeric,
        "SELECT ST_Azimuth(ST_Point(0, 0, 4326), ST_Point(1, 1, 4326))"
    ),
    spec!(
        "ST_Project",
        3,
        Blob,
        "SELECT ST_Project(ST_Point(0, 0, 4326), 1000.0, 0.5)"
    ),
    spec!(
        "ST_ClosestPoint",
        2,
        Blob,
        "SELECT ST_ClosestPoint(ST_GeomFromText('LINESTRING(0 0,2 0)'), ST_Point(1, 1))"
    ),
    // Operations
    spec!(
        "ST_Union",
        2,
        Blob,
        "SELECT ST_Union(ST_GeomFromText('POLYGON((0 0,2 0,2 2,0 2,0 0))'), ST_GeomFromText('POLYGON((1 1,3 1,3 3,1 3,1 1))'))"
    ),
    spec!(
        "ST_Intersection",
        2,
        Blob,
        "SELECT ST_Intersection(ST_GeomFromText('POLYGON((0 0,2 0,2 2,0 2,0 0))'), ST_GeomFromText('POLYGON((1 1,3 1,3 3,1 3,1 1))'))"
    ),
    spec!(
        "ST_Difference",
        2,
        Blob,
        "SELECT ST_Difference(ST_GeomFromText('POLYGON((0 0,3 0,3 3,0 3,0 0))'), ST_GeomFromText('POLYGON((1 1,2 1,2 2,1 2,1 1))'))"
    ),
    spec!(
        "ST_SymDifference",
        2,
        Blob,
        "SELECT ST_SymDifference(ST_GeomFromText('POLYGON((0 0,2 0,2 2,0 2,0 0))'), ST_GeomFromText('POLYGON((1 1,3 1,3 3,1 3,1 1))'))"
    ),
    spec!(
        "ST_Buffer",
        2,
        Blob,
        "SELECT ST_Buffer(ST_Point(0, 0), 1.0)"
    ),
    // Predicates
    spec!(
        "ST_Intersects",
        2,
        Bool,
        "SELECT ST_Intersects(ST_GeomFromText('LINESTRING(0 0,2 0)'), ST_GeomFromText('LINESTRING(1 -1,1 1)'))"
    ),
    spec!(
        "ST_Contains",
        2,
        Bool,
        "SELECT ST_Contains(ST_GeomFromText('POLYGON((0 0,2 0,2 2,0 2,0 0))'), ST_Point(1, 1))"
    ),
    spec!(
        "ST_Within",
        2,
        Bool,
        "SELECT ST_Within(ST_Point(1, 1), ST_GeomFromText('POLYGON((0 0,2 0,2 2,0 2,0 0))'))"
    ),
    spec!(
        "ST_Disjoint",
        2,
        Bool,
        "SELECT ST_Disjoint(ST_Point(0, 0), ST_Point(5, 5))"
    ),
    spec!(
        "ST_DWithin",
        3,
        Bool,
        "SELECT ST_DWithin(ST_Point(0, 0), ST_Point(3, 4), 5.0)"
    ),
    spec!(
        "ST_DWithinSphere",
        3,
        Bool,
        "SELECT ST_DWithinSphere(ST_Point(0, 0, 4326), ST_Point(0, 1, 4326), 200000.0)"
    ),
    spec!(
        "ST_DWithinSpheroid",
        3,
        Bool,
        "SELECT ST_DWithinSpheroid(ST_Point(0, 0, 4326), ST_Point(0, 1, 4326), 200000.0)"
    ),
    spec!(
        "ST_Covers",
        2,
        Bool,
        "SELECT ST_Covers(ST_GeomFromText('POLYGON((0 0,2 0,2 2,0 2,0 0))'), ST_Point(1, 1))"
    ),
    spec!(
        "ST_CoveredBy",
        2,
        Bool,
        "SELECT ST_CoveredBy(ST_Point(1, 1), ST_GeomFromText('POLYGON((0 0,2 0,2 2,0 2,0 0))'))"
    ),
    spec!(
        "ST_Equals",
        2,
        Bool,
        "SELECT ST_Equals(ST_Point(1, 1), ST_Point(1, 1))"
    ),
    spec!(
        "ST_Touches",
        2,
        Bool,
        "SELECT ST_Touches(ST_GeomFromText('POLYGON((0 0,2 0,2 2,0 2,0 0))'), ST_Point(0, 1))"
    ),
    spec!(
        "ST_Crosses",
        2,
        Bool,
        "SELECT ST_Crosses(ST_GeomFromText('LINESTRING(0 0,2 2)'), ST_GeomFromText('LINESTRING(0 2,2 0)'))"
    ),
    spec!(
        "ST_Overlaps",
        2,
        Bool,
        "SELECT ST_Overlaps(ST_GeomFromText('LINESTRING(0 0,2 0)'), ST_GeomFromText('LINESTRING(1 0,3 0)'))"
    ),
    spec!(
        "ST_Relate",
        2,
        Text,
        "SELECT ST_Relate(ST_Point(0, 0), ST_Point(0, 0))"
    ),
    spec!(
        "ST_Relate",
        3,
        Bool,
        "SELECT ST_Relate(ST_Point(0, 0), ST_Point(0, 0), '0FFFFFFF2')"
    ),
    spec!(
        "ST_RelateMatch",
        2,
        Bool,
        "SELECT ST_RelateMatch('0FFFFFFF2', '0FFFFFFF2')"
    ),
];

pub const SQLITE_DIRECT_ONLY_FUNCTIONS: &[SqliteFunctionSpec] = &[
    direct_spec!(
        "CreateSpatialIndex",
        2,
        Numeric,
        "SELECT CreateSpatialIndex('_rt', 'geom')",
        "SELECT CreateSpatialIndex(NULL, 'geom')",
        "table name must not be NULL",
        "create_spatial_index_xfunc"
    ),
    direct_spec!(
        "DropSpatialIndex",
        2,
        Numeric,
        "SELECT DropSpatialIndex('_rt', 'geom')",
        "SELECT DropSpatialIndex(NULL, 'geom')",
        "table name must not be NULL",
        "drop_spatial_index_xfunc"
    ),
];
