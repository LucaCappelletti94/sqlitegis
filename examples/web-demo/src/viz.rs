//! Equirectangular world map on a `<canvas>`, with mouse-drag pan, mouse-wheel
//! zoom, and wraparound on both axes. Cities are 1-px dots in muted gray, rows
//! returned by the last query are overlaid in orange, and the user marker is a
//! small green disc.

use dioxus::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{CanvasRenderingContext2d, HtmlCanvasElement};

const CANVAS_ID: &str = "geolite-map";
const CANVAS_W: u32 = 1080;
const CANVAS_H: u32 = 540;
const MIN_ZOOM: f64 = 0.25;
const MAX_ZOOM: f64 = 64.0;
const ZOOM_STEP: f64 = 1.15;
const CLICK_PX_THRESHOLD: f64 = 5.0;

#[component]
pub fn WorldMap(
    coords: ReadSignal<Vec<(f64, f64)>>,
    highlighted: ReadSignal<Vec<(f64, f64)>>,
    user_lon: Signal<f64>,
    user_lat: Signal<f64>,
) -> Element {
    let mut user_lon = user_lon;
    let mut user_lat = user_lat;

    let mut zoom = use_signal(|| 1.0_f64);
    let mut center_lon = use_signal(|| 0.0_f64);
    let mut center_lat = use_signal(|| 0.0_f64);

    // Drag state: Some((mouse_x_at_down, mouse_y_at_down,
    // center_lon_at_down, center_lat_at_down, accumulated_pixel_distance)).
    let mut drag = use_signal::<Option<(f64, f64, f64, f64, f64)>>(|| None);

    use_effect(move || {
        let all = coords.read();
        let hits = highlighted.read();
        let ux = *user_lon.read();
        let uy = *user_lat.read();
        let z = *zoom.read();
        let clon = *center_lon.read();
        let clat = *center_lat.read();
        draw(&all, &hits, (ux, uy), z, clon, clat);
    });

    rsx! {
        section {
            h2 { "World map" }
            canvas {
                id: CANVAS_ID,
                width: "{CANVAS_W}",
                height: "{CANVAS_H}",
                style: "cursor: grab; touch-action: none;",
                onwheel: move |e| {
                    e.prevent_default();
                    let dy = e.data().delta().strip_units().y;
                    if dy == 0.0 { return; }
                    let factor = if dy < 0.0 { ZOOM_STEP } else { 1.0 / ZOOM_STEP };
                    let pt = e.element_coordinates();
                    let cx: f64 = pt.x.into();
                    let cy: f64 = pt.y.into();
                    let Some((w, h)) = display_size() else { return };
                    let old_z = *zoom.peek();
                    let new_z = (old_z * factor).clamp(MIN_ZOOM, MAX_ZOOM);
                    if new_z == old_z { return; }
                    // Keep the world point under the cursor fixed across the zoom.
                    let old_clon = *center_lon.peek();
                    let old_clat = *center_lat.peek();
                    let world_lon = old_clon + (cx / w - 0.5) * (360.0 / old_z);
                    let world_lat = old_clat - (cy / h - 0.5) * (180.0 / old_z);
                    let new_clon = world_lon - (cx / w - 0.5) * (360.0 / new_z);
                    let new_clat = world_lat + (cy / h - 0.5) * (180.0 / new_z);
                    zoom.set(new_z);
                    center_lon.set(new_clon);
                    center_lat.set(new_clat);
                },
                onmousedown: move |e| {
                    let pt = e.element_coordinates();
                    let x: f64 = pt.x.into();
                    let y: f64 = pt.y.into();
                    drag.set(Some((x, y, *center_lon.peek(), *center_lat.peek(), 0.0)));
                },
                onmousemove: move |e| {
                    let Some((sx, sy, scl, scla, total)) = *drag.peek() else { return };
                    let Some((w, h)) = display_size() else { return };
                    let pt = e.element_coordinates();
                    let mx: f64 = pt.x.into();
                    let my: f64 = pt.y.into();
                    let z = *zoom.peek();
                    let lon_per_px = (360.0 / z) / w;
                    let lat_per_px = (180.0 / z) / h;
                    center_lon.set(scl - (mx - sx) * lon_per_px);
                    center_lat.set(scla + (my - sy) * lat_per_px);
                    let new_total = total + (mx - sx).abs() + (my - sy).abs();
                    drag.set(Some((sx, sy, scl, scla, new_total)));
                },
                onmouseup: move |e| {
                    let prev = *drag.peek();
                    drag.set(None);
                    let Some((_, _, _, _, total)) = prev else { return };
                    if total >= CLICK_PX_THRESHOLD { return; }
                    // Tap: place the user pin. Reject clicks that fall outside
                    // the world rectangle (panned into empty space).
                    let pt = e.element_coordinates();
                    let Some((w, h)) = display_size() else { return };
                    let z = *zoom.peek();
                    let clon = *center_lon.peek();
                    let clat = *center_lat.peek();
                    let x: f64 = pt.x.into();
                    let y: f64 = pt.y.into();
                    let lon = clon + (x / w - 0.5) * (360.0 / z);
                    let lat = clat - (y / h - 0.5) * (180.0 / z);
                    if !(-180.0..=180.0).contains(&lon) || !(-90.0..=90.0).contains(&lat) {
                        return;
                    }
                    user_lon.set(lon);
                    user_lat.set(lat);
                },
                onmouseleave: move |_| drag.set(None),
            }
            p { class: "meta",
                "zoom={zoom:.2}x | center=({center_lon:.1}, {center_lat:.1}) | "
                "scroll to zoom, drag to pan, click to drop the position pin"
            }
            p { class: "meta",
                "Equirectangular projection. Gray dots: every city. "
                "Orange: rows from the last query that exposed `lon` and `lat`. "
                "Green: your position."
            }
        }
    }
}

fn display_size() -> Option<(f64, f64)> {
    let canvas = web_sys::window()?
        .document()?
        .get_element_by_id(CANVAS_ID)?
        .dyn_into::<HtmlCanvasElement>()
        .ok()?;
    let rect = canvas.get_bounding_client_rect();
    let w = rect.width();
    let h = rect.height();
    if w <= 0.0 || h <= 0.0 { None } else { Some((w, h)) }
}

fn draw(
    all: &[(f64, f64)],
    highlighted: &[(f64, f64)],
    user: (f64, f64),
    zoom: f64,
    center_lon: f64,
    center_lat: f64,
) {
    let Some(canvas) = web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.get_element_by_id(CANVAS_ID))
        .and_then(|e| e.dyn_into::<HtmlCanvasElement>().ok())
    else {
        return;
    };
    let Some(ctx) = canvas
        .get_context("2d")
        .ok()
        .flatten()
        .and_then(|c| c.dyn_into::<CanvasRenderingContext2d>().ok())
    else {
        return;
    };

    let w = canvas.width() as f64;
    let h = canvas.height() as f64;
    let lon_span = 360.0 / zoom;
    let lat_span = 180.0 / zoom;
    let lon_min = center_lon - lon_span / 2.0;
    let lon_max = center_lon + lon_span / 2.0;
    let lat_min = center_lat - lat_span / 2.0;
    let lat_max = center_lat + lat_span / 2.0;

    ctx.set_fill_style_str(&String::from("#07090b"));
    ctx.fill_rect(0.0, 0.0, w, h);

    // Subtle frame at the world rectangle (-180..180, -90..90) so users see
    // where the world ends when they pan into empty space.
    let world_left = project_x(-180.0, w, zoom, center_lon);
    let world_right = project_x(180.0, w, zoom, center_lon);
    let world_top = project_y(90.0, h, zoom, center_lat);
    let world_bottom = project_y(-90.0, h, zoom, center_lat);
    ctx.set_stroke_style_str(&String::from("#2a323b"));
    ctx.set_line_width(1.0);
    ctx.stroke_rect(
        world_left,
        world_top,
        world_right - world_left,
        world_bottom - world_top,
    );

    // Grid lines at multiples of 30 degrees, only within the world rectangle.
    ctx.set_stroke_style_str(&String::from("#1a2128"));
    ctx.begin_path();
    for lat_g in (-60..=60).step_by(30) {
        let y = project_y(lat_g as f64, h, zoom, center_lat);
        if y >= world_top && y <= world_bottom {
            ctx.move_to(world_left.max(0.0), y);
            ctx.line_to(world_right.min(w), y);
        }
    }
    for lon_g in (-150..=150).step_by(30) {
        let x = project_x(lon_g as f64, w, zoom, center_lon);
        if x >= world_left && x <= world_right {
            ctx.move_to(x, world_top.max(0.0));
            ctx.line_to(x, world_bottom.min(h));
        }
    }
    ctx.stroke();

    let dot = if zoom >= 4.0 { 2.5 } else { 1.5 };

    // Cities -- one draw per city, clipped to the visible canvas.
    ctx.set_fill_style_str(&String::from("#3a4554"));
    for &(lon, lat) in all {
        if lon < lon_min - 1.0 || lon > lon_max + 1.0 {
            continue;
        }
        if lat < lat_min - 1.0 || lat > lat_max + 1.0 {
            continue;
        }
        let x = project_x(lon, w, zoom, center_lon);
        let y = project_y(lat, h, zoom, center_lat);
        ctx.fill_rect(x - dot / 2.0, y - dot / 2.0, dot, dot);
    }

    // Query hits.
    let hit = if zoom >= 4.0 { 6.0 } else { 5.0 };
    ctx.set_fill_style_str(&String::from("#ffae57"));
    for &(lon, lat) in highlighted {
        if lon < lon_min - 1.0 || lon > lon_max + 1.0 {
            continue;
        }
        if lat < lat_min - 1.0 || lat > lat_max + 1.0 {
            continue;
        }
        let x = project_x(lon, w, zoom, center_lon);
        let y = project_y(lat, h, zoom, center_lat);
        ctx.fill_rect(x - hit / 2.0, y - hit / 2.0, hit, hit);
    }

    // User marker.
    let (ulon, ulat) = user;
    if (lon_min - 1.0..=lon_max + 1.0).contains(&ulon)
        && (lat_min - 1.0..=lat_max + 1.0).contains(&ulat)
    {
        ctx.set_fill_style_str(&String::from("#4fbf6f"));
        let x = project_x(ulon, w, zoom, center_lon);
        let y = project_y(ulat, h, zoom, center_lat);
        ctx.begin_path();
        let _ = ctx.arc(x, y, 6.0, 0.0, std::f64::consts::TAU);
        ctx.fill();
    }
}

fn project_x(lon: f64, w: f64, zoom: f64, center_lon: f64) -> f64 {
    ((lon - center_lon) / (360.0 / zoom) + 0.5) * w
}

fn project_y(lat: f64, h: f64, zoom: f64, center_lat: f64) -> f64 {
    (0.5 - (lat - center_lat) / (180.0 / zoom)) * h
}
