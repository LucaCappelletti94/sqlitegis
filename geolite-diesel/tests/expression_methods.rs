#![cfg(feature = "sqlite")]

//! Verify that every `GeometryExpressionMethods` method produces identical SQL
//! to the corresponding free function in `geolite_diesel::functions`.

use diesel::dsl::select;
use diesel::sql_types::{Integer, Nullable};
use geolite_core::function_catalog::SQLITE_DETERMINISTIC_FUNCTIONS;
use geolite_diesel::prelude::*;
use std::collections::BTreeSet;

const DIESEL_FUNCTIONS_SRC: &str = include_str!("../src/generated/functions.rs");

/// Geometry literal helper (not Clone, so create fresh each time via macro).
macro_rules! g {
    () => {
        diesel::dsl::sql::<Nullable<Geometry>>("x")
    };
}

macro_rules! d {
    () => {
        diesel::dsl::sql::<diesel::sql_types::Double>("1.0")
    };
}

macro_rules! i {
    () => {
        diesel::dsl::sql::<Integer>("1")
    };
}

macro_rules! t {
    () => {
        diesel::dsl::sql::<diesel::sql_types::Text>("'T*****FF*'")
    };
}

/// Assert method-style and function-style produce identical SQL.
macro_rules! assert_method_eq_func {
    ($method_expr:expr, $func_expr:expr) => {{
        let method_sql =
            diesel::debug_query::<diesel::sqlite::Sqlite, _>(&select($method_expr)).to_string();
        let func_sql =
            diesel::debug_query::<diesel::sqlite::Sqlite, _>(&select($func_expr)).to_string();
        assert_eq!(method_sql, func_sql);
    }};
}

fn parse_name_and_args_after_fn(src: &str, fn_start: usize) -> Option<(String, String)> {
    let rest = &src[fn_start..];
    let open_paren = rest.find('(')?;
    let name = rest[..open_paren].trim().to_string();

    let mut depth = 1usize;
    let mut idx = open_paren + 1;
    let bytes = rest.as_bytes();
    while idx < rest.len() {
        match bytes[idx] {
            b'(' => depth += 1,
            b')' => {
                depth -= 1;
                if depth == 0 {
                    let args = rest[open_paren + 1..idx].trim().to_string();
                    return Some((name, args));
                }
            }
            _ => {}
        }
        idx += 1;
    }
    None
}

fn normalize_fn_name(name: &str) -> String {
    name.split('<').next().unwrap_or(name).trim().to_string()
}

fn geometry_first_sql_functions(src: &str) -> BTreeSet<String> {
    src.split("diesel::define_sql_function! {")
        .skip(1)
        .filter_map(|block| {
            let fn_idx = block.find("fn st_")?;
            let fn_start = fn_idx + "fn ".len();
            let (name, args) = parse_name_and_args_after_fn(block, fn_start)?;
            let first_arg = args.split(',').next()?.trim();
            if first_arg.contains("Nullable<Geometry>") {
                Some(normalize_fn_name(&name))
            } else {
                None
            }
        })
        .collect()
}

fn geometry_expression_methods(src: &str) -> BTreeSet<String> {
    let trait_start = src
        .find("pub trait GeometryExpressionMethods")
        .expect("GeometryExpressionMethods trait must exist");
    let impl_start = src
        .find("impl<E> GeometryExpressionMethods for E")
        .unwrap_or(src.len());
    let trait_body = &src[trait_start..impl_start];

    trait_body
        .match_indices("fn st_")
        .filter_map(|(idx, _)| {
            let fn_start = idx + "fn ".len();
            parse_name_and_args_after_fn(trait_body, fn_start)
                .map(|(name, _)| normalize_fn_name(&name))
        })
        .collect()
}

fn sql_name_override(block: &str) -> Option<String> {
    for line in block.lines() {
        let trimmed = line.trim();
        if !trimmed.starts_with("#[sql_name") {
            continue;
        }
        let first_quote = trimmed.find('"')?;
        let rest = &trimmed[first_quote + 1..];
        let second_quote = rest.find('"')?;
        return Some(rest[..second_quote].to_string());
    }
    None
}

fn diesel_sql_signatures(src: &str) -> BTreeSet<(String, usize)> {
    src.split("diesel::define_sql_function! {")
        .skip(1)
        .filter_map(|block| {
            let fn_idx = block.find("fn ")?;
            let fn_start = fn_idx + "fn ".len();
            let (fn_name, args) = parse_name_and_args_after_fn(block, fn_start)?;
            let sql_name = sql_name_override(block).unwrap_or(fn_name);
            let arg_count = if args.trim().is_empty() {
                0
            } else {
                args.split(',').filter(|arg| !arg.trim().is_empty()).count()
            };
            Some((sql_name.to_ascii_uppercase(), arg_count))
        })
        .collect()
}

#[test]
fn diesel_functions_and_methods_surface_parity() {
    let sql_surface = geometry_first_sql_functions(DIESEL_FUNCTIONS_SRC);
    let method_surface = geometry_expression_methods(include_str!("../src/expression_methods.rs"));

    let missing_methods: Vec<_> = sql_surface.difference(&method_surface).cloned().collect();
    let extra_methods: Vec<_> = method_surface.difference(&sql_surface).cloned().collect();

    assert!(
        missing_methods.is_empty(),
        "missing method wrappers for SQL functions: {missing_methods:?}"
    );
    assert!(
        extra_methods.is_empty(),
        "method wrappers without matching SQL function declarations: {extra_methods:?}"
    );
}

#[test]
fn diesel_sql_functions_are_backed_by_sqlite_catalog() {
    let diesel_signatures = diesel_sql_signatures(DIESEL_FUNCTIONS_SRC);
    let catalog_signatures: BTreeSet<(String, usize)> = SQLITE_DETERMINISTIC_FUNCTIONS
        .iter()
        .map(|spec| (spec.name.to_ascii_uppercase(), spec.n_arg as usize))
        .collect();

    let missing_catalog_entries: Vec<_> = diesel_signatures
        .difference(&catalog_signatures)
        .cloned()
        .collect();

    assert!(
        missing_catalog_entries.is_empty(),
        "diesel SQL functions missing from canonical SQLite catalog: {missing_catalog_entries:?}"
    );
}

#[test]
fn catalog_functions_are_covered_by_diesel_declarations() {
    let catalog_signatures: BTreeSet<(String, usize)> = SQLITE_DETERMINISTIC_FUNCTIONS
        .iter()
        .map(|spec| (spec.name.to_ascii_uppercase(), spec.n_arg as usize))
        .collect();

    let diesel_signatures = diesel_sql_signatures(DIESEL_FUNCTIONS_SRC);

    let missing_diesel: Vec<_> = catalog_signatures
        .difference(&diesel_signatures)
        .cloned()
        .collect();

    assert!(
        missing_diesel.is_empty(),
        "catalog functions not covered by Diesel declarations: {missing_diesel:?}"
    );
}

macro_rules! assert_unary_cases {
    ($( $test_name:ident => $method:ident ),+ $(,)?) => {
        $(
            #[test]
            fn $test_name() {
                assert_method_eq_func!(g!().$method(), $method(g!()));
            }
        )+
    };
}

macro_rules! assert_geom_geom_cases {
    ($( $test_name:ident => $method:ident ),+ $(,)?) => {
        $(
            #[test]
            fn $test_name() {
                assert_method_eq_func!(g!().$method(g!()), $method(g!(), g!()));
            }
        )+
    };
}

macro_rules! assert_geom_int_cases {
    ($( $test_name:ident => $method:ident ),+ $(,)?) => {
        $(
            #[test]
            fn $test_name() {
                assert_method_eq_func!(g!().$method(i!()), $method(g!(), i!()));
            }
        )+
    };
}

macro_rules! assert_geom_double_cases {
    ($( $test_name:ident => $method:ident ),+ $(,)?) => {
        $(
            #[test]
            fn $test_name() {
                assert_method_eq_func!(g!().$method(d!()), $method(g!(), d!()));
            }
        )+
    };
}

macro_rules! assert_geom_geom_double_cases {
    ($( $test_name:ident => $method:ident ),+ $(,)?) => {
        $(
            #[test]
            fn $test_name() {
                assert_method_eq_func!(g!().$method(g!(), d!()), $method(g!(), g!(), d!()));
            }
        )+
    };
}

macro_rules! assert_geom_geom_text_cases {
    ($( $test_name:ident => $method:ident ),+ $(,)?) => {
        $(
            #[test]
            fn $test_name() {
                assert_method_eq_func!(g!().$method(g!(), t!()), $method(g!(), g!(), t!()));
            }
        )+
    };
}

macro_rules! assert_geom_double_double_cases {
    ($( $test_name:ident => $method:ident ),+ $(,)?) => {
        $(
            #[test]
            fn $test_name() {
                assert_method_eq_func!(g!().$method(d!(), d!()), $method(g!(), d!(), d!()));
            }
        )+
    };
}

// -- I/O ---------------------------------------------------------------------

assert_unary_cases!(
    method_st_astext => st_astext,
    method_st_asewkt => st_asewkt,
    method_st_asbinary => st_asbinary,
    method_st_asewkb => st_asewkb,
    method_st_asgeojson => st_asgeojson,
);

// -- Constructors / transforms -----------------------------------------------

assert_unary_cases!(method_st_makepolygon => st_makepolygon,);
assert_geom_geom_cases!(
    method_st_makeline => st_makeline,
    method_st_collect => st_collect,
);
assert_geom_double_cases!(method_st_buffer => st_buffer,);

// -- Accessors ----------------------------------------------------------------

assert_unary_cases!(
    method_st_srid => st_srid,
    method_st_geometrytype => st_geometrytype,
    method_st_x => st_x,
    method_st_y => st_y,
    method_st_z => st_z,
    method_st_isempty => st_isempty,
    method_st_ndims => st_ndims,
    method_st_coorddim => st_coorddim,
    method_st_zmflag => st_zmflag,
    method_st_memsize => st_memsize,
    method_st_isvalid => st_isvalid,
    method_st_isvalidreason => st_isvalidreason,
    method_st_numpoints => st_numpoints,
    method_st_npoints => st_npoints,
    method_st_numgeometries => st_numgeometries,
    method_st_numinteriorrings => st_numinteriorrings,
    method_st_numinteriorring => st_numinteriorring,
    method_st_numrings => st_numrings,
    method_st_dimension => st_dimension,
    method_st_envelope => st_envelope,
    method_st_startpoint => st_startpoint,
    method_st_endpoint => st_endpoint,
    method_st_exteriorring => st_exteriorring,
    method_st_xmin => st_xmin,
    method_st_xmax => st_xmax,
    method_st_ymin => st_ymin,
    method_st_ymax => st_ymax,
);
assert_geom_int_cases!(
    method_st_setsrid => st_setsrid,
    method_st_pointn => st_pointn,
    method_st_interiorringn => st_interiorringn,
    method_st_geometryn => st_geometryn,
);

// -- Measurement --------------------------------------------------------------

assert_unary_cases!(
    method_st_area => st_area,
    method_st_length => st_length,
    method_st_length2d => st_length2d,
    method_st_perimeter => st_perimeter,
    method_st_perimeter2d => st_perimeter2d,
    method_st_centroid => st_centroid,
    method_st_pointonsurface => st_pointonsurface,
);
assert_geom_geom_cases!(
    method_st_distance => st_distance,
    method_st_distancesphere => st_distancesphere,
    method_st_distancespheroid => st_distancespheroid,
    method_st_hausdorffdistance => st_hausdorffdistance,
);

// -- Operations ---------------------------------------------------------------

assert_geom_geom_cases!(
    method_st_union => st_union,
    method_st_intersection => st_intersection,
    method_st_difference => st_difference,
    method_st_symdifference => st_symdifference,
);

// -- Predicates ---------------------------------------------------------------

assert_geom_geom_cases!(
    method_st_intersects => st_intersects,
    method_st_contains => st_contains,
    method_st_within => st_within,
    method_inside_area => inside_area,
    method_st_covers => st_covers,
    method_st_coveredby => st_coveredby,
    method_st_disjoint => st_disjoint,
    method_outside_area => outside_area,
    method_st_equals => st_equals,
    method_st_relate => st_relate,
    method_st_touches => st_touches,
    method_st_crosses => st_crosses,
    method_st_overlaps => st_overlaps,
    method_st_azimuth => st_azimuth,
    method_st_closestpoint => st_closestpoint,
);
assert_geom_geom_double_cases!(
    method_st_dwithin => st_dwithin,
    method_st_dwithinsphere => st_dwithinsphere,
    method_st_dwithinspheroid => st_dwithinspheroid,
);
assert_geom_geom_text_cases!(method_st_relate_match_geoms => st_relate_match_geoms,);

// -- Geography variants -------------------------------------------------------

assert_unary_cases!(method_st_lengthsphere => st_lengthsphere,);
assert_geom_double_double_cases!(method_st_project => st_project,);
