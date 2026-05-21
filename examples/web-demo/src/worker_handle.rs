//! Singleton handle to the dedicated SQLite worker.
//!
//! On first use we spawn the worker from `/generated/sqlitegis-worker.js`
//! (written by `build.rs`), install one `onmessage` listener that routes
//! responses by token, and expose three async entry points that mirror the
//! old synchronous `db.rs` / `loader.rs` / `runner.rs` surface:
//!
//! - [`apply_schema`] -- re-open the in-memory connection and apply a
//!   schema script.
//! - [`load_dataset`] -- stream cities5000 into the `places` table. The
//!   `on_progress` callback is invoked after every worker-side batch.
//! - [`run_query`] -- run one SQL script with `:lon` / `:lat` substituted.
//!
//! Each call generates a fresh monotonic `token` and parks a sender in a
//! slab keyed by token. The global onmessage callback looks up the slab
//! entry, posts the response, and removes the entry on terminal responses.
//! `LoadDataset` uses an `mpsc::UnboundedSender` so progress updates
//! stream through; everything else uses a `futures::channel::oneshot`.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use futures::channel::{mpsc, oneshot};
use futures::StreamExt;
use sqlitegis_web_demo_protocol::{
    LoadReport, LonLat, QueryOutcome, WorkerRequest, WorkerResponse,
};
use wasm_bindgen::closure::Closure;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{ErrorEvent, MessageEvent, Worker, WorkerOptions, WorkerType};

const WORKER_SCRIPT: &str = "/generated/sqlitegis-worker.js";

thread_local! {
    static CLIENT: RefCell<Option<Rc<WorkerClient>>> = const { RefCell::new(None) };
}

struct WorkerClient {
    worker: Worker,
    next_token: Cell<u64>,
    /// One-response slots (ApplySchema, RunQuery). The handler removes the
    /// entry as soon as it forwards the response.
    pending: RefCell<HashMap<u64, oneshot::Sender<WorkerResponse>>>,
    /// Streaming slots (LoadDataset). LoadProgress messages forward through
    /// the sender; LoadComplete / Error remove the entry and close it.
    streams: RefCell<HashMap<u64, mpsc::UnboundedSender<WorkerResponse>>>,
    /// Set to true after the worker posts `Ready`. Requests issued before
    /// then are queued via the `pre_ready` buffer.
    ready: Cell<bool>,
    pre_ready_queue: RefCell<Vec<JsValue>>,
    ready_waiters: RefCell<Vec<oneshot::Sender<()>>>,
    // Closures owned by the client so they outlive Rust scope; dropping the
    // client tears the worker down.
    _onmessage: Closure<dyn FnMut(MessageEvent)>,
    _onerror: Closure<dyn FnMut(ErrorEvent)>,
}

fn ensure_client() -> Result<Rc<WorkerClient>, String> {
    let existing = CLIENT.with(|cell| cell.borrow().clone());
    if let Some(client) = existing {
        return Ok(client);
    }

    let options = WorkerOptions::new();
    options.set_type(WorkerType::Module);
    let worker = Worker::new_with_options(WORKER_SCRIPT, &options)
        .map_err(|error| format!("failed to spawn worker: {}", js_error_text(&error)))?;

    // `Rc<RefCell<Option<Rc<WorkerClient>>>>` would be cleaner, but we
    // construct the client and immediately publish it to the thread-local
    // so the onmessage closure can borrow back through CLIENT.
    let onmessage = Closure::wrap(Box::new(move |event: MessageEvent| {
        on_message(event);
    }) as Box<dyn FnMut(MessageEvent)>);
    let onerror = Closure::wrap(Box::new(move |event: ErrorEvent| {
        log::error!("worker error: {}", event.message());
    }) as Box<dyn FnMut(ErrorEvent)>);

    worker.set_onmessage(Some(onmessage.as_ref().unchecked_ref()));
    worker.set_onerror(Some(onerror.as_ref().unchecked_ref()));

    let client = Rc::new(WorkerClient {
        worker,
        next_token: Cell::new(1),
        pending: RefCell::new(HashMap::new()),
        streams: RefCell::new(HashMap::new()),
        ready: Cell::new(false),
        pre_ready_queue: RefCell::new(Vec::new()),
        ready_waiters: RefCell::new(Vec::new()),
        _onmessage: onmessage,
        _onerror: onerror,
    });
    CLIENT.with(|cell| *cell.borrow_mut() = Some(client.clone()));
    Ok(client)
}

fn on_message(event: MessageEvent) {
    let response: WorkerResponse = match serde_wasm_bindgen::from_value(event.data()) {
        Ok(response) => response,
        Err(error) => {
            log::error!("failed to decode worker response: {error}");
            return;
        }
    };

    let client = match CLIENT.with(|cell| cell.borrow().clone()) {
        Some(client) => client,
        None => return,
    };

    if matches!(response, WorkerResponse::Ready) {
        client.ready.set(true);
        // Flush any messages buffered before Ready landed.
        let pending = client.pre_ready_queue.replace(Vec::new());
        for payload in pending {
            let _ = client.worker.post_message(&payload);
        }
        // Wake every awaiter parked on `await_ready`.
        let waiters = client.ready_waiters.replace(Vec::new());
        for waiter in waiters {
            let _ = waiter.send(());
        }
        return;
    }

    let token = response.token();

    // Streaming path: try the mpsc slab first.
    let stream_sender = client.streams.borrow().get(&token).cloned();
    if let Some(sender) = stream_sender {
        let terminal = response.is_terminal();
        let _ = sender.unbounded_send(response);
        if terminal {
            client.streams.borrow_mut().remove(&token);
        }
        return;
    }

    // One-shot path.
    let sender = client.pending.borrow_mut().remove(&token);
    if let Some(sender) = sender {
        let _ = sender.send(response);
    }
}

fn next_token(client: &WorkerClient) -> u64 {
    let token = client.next_token.get();
    client.next_token.set(token + 1);
    token
}

fn post(client: &WorkerClient, request: &WorkerRequest) -> Result<(), String> {
    let payload = serde_wasm_bindgen::to_value(request)
        .map_err(|error| format!("failed to encode worker request: {error}"))?;
    if !client.ready.get() {
        client.pre_ready_queue.borrow_mut().push(payload);
        return Ok(());
    }
    client
        .worker
        .post_message(&payload)
        .map_err(|error| format!("failed to post worker request: {}", js_error_text(&error)))
}

/// Wait until the worker posts its `Ready` greeting. Cheap once Ready has
/// landed (returns immediately).
pub async fn await_ready() -> Result<(), String> {
    let client = ensure_client()?;
    if client.ready.get() {
        return Ok(());
    }
    let (tx, rx) = oneshot::channel::<()>();
    client.ready_waiters.borrow_mut().push(tx);
    rx.await
        .map_err(|_| "worker dropped before ready".to_owned())
}

/// Re-open the in-memory SQLite connection on the worker and apply a
/// schema script (typically `state::DEFAULT_SCHEMA_SQL`).
pub async fn apply_schema(sql: &str) -> Result<(), String> {
    let client = ensure_client()?;
    let token = next_token(&client);
    let (tx, rx) = oneshot::channel();
    client.pending.borrow_mut().insert(token, tx);
    post(
        &client,
        &WorkerRequest::ApplySchema {
            token,
            sql: sql.to_owned(),
        },
    )?;
    match rx.await {
        Ok(WorkerResponse::SchemaApplied { .. }) => Ok(()),
        Ok(WorkerResponse::Error { message, .. }) => Err(message),
        Ok(other) => Err(format!("unexpected worker response: {other:?}")),
        Err(_) => Err("worker dropped while applying schema".to_owned()),
    }
}

/// Stream the cities5000 TSV into the `places` table. The callback fires
/// after every batch with `(inserted_so_far, total_lines, batch_coords)`.
pub async fn load_dataset(
    tsv_url: &str,
    mut on_progress: impl FnMut(usize, usize, &[LonLat]),
) -> Result<LoadReport, String> {
    let client = ensure_client()?;
    let token = next_token(&client);
    let (tx, mut rx) = mpsc::unbounded::<WorkerResponse>();
    client.streams.borrow_mut().insert(token, tx);
    post(
        &client,
        &WorkerRequest::LoadDataset {
            token,
            tsv_url: tsv_url.to_owned(),
        },
    )?;

    while let Some(message) = rx.next().await {
        match message {
            WorkerResponse::LoadProgress {
                inserted,
                total,
                batch,
                ..
            } => on_progress(inserted, total, &batch),
            WorkerResponse::LoadComplete { report, .. } => return Ok(report),
            WorkerResponse::Error { message, .. } => return Err(message),
            _ => continue,
        }
    }
    Err("worker dropped while loading dataset".to_owned())
}

/// Run one SQL script with `:lon` / `:lat` substituted from the probe
/// point. Returns the same `QueryOutcome` shape the old synchronous
/// runner did, just through a worker round-trip.
pub async fn run_query(sql: &str, lon: f64, lat: f64) -> QueryOutcome {
    let client = match ensure_client() {
        Ok(client) => client,
        Err(error) => return QueryOutcome::Error(error),
    };
    let token = next_token(&client);
    let (tx, rx) = oneshot::channel();
    client.pending.borrow_mut().insert(token, tx);
    if let Err(error) = post(
        &client,
        &WorkerRequest::RunQuery {
            token,
            sql: sql.to_owned(),
            lon,
            lat,
        },
    ) {
        client.pending.borrow_mut().remove(&token);
        return QueryOutcome::Error(error);
    }
    match rx.await {
        Ok(WorkerResponse::QueryRows {
            result, elapsed_ms, ..
        }) => QueryOutcome::Rows { result, elapsed_ms },
        Ok(WorkerResponse::QueryAffected {
            rows, elapsed_ms, ..
        }) => QueryOutcome::Affected { rows, elapsed_ms },
        Ok(WorkerResponse::Error { message, .. }) => QueryOutcome::Error(message),
        Ok(other) => QueryOutcome::Error(format!("unexpected worker response: {other:?}")),
        Err(_) => QueryOutcome::Error("worker dropped while running query".to_owned()),
    }
}

/// Pull `(lon, lat)` tuples out of a query result, if it exposed columns
/// named `lon` and `lat`. Used to highlight rows on the canvas. Same
/// behaviour as the old `runner::extract_lonlat`; lives here now because
/// it's a UI-side projection of the `QueryRows` shape from the protocol.
pub fn extract_lonlat(result: &sqlitegis_web_demo_protocol::QueryRows) -> Vec<LonLat> {
    let lon_idx = result
        .columns
        .iter()
        .position(|c| c.eq_ignore_ascii_case("lon"));
    let lat_idx = result
        .columns
        .iter()
        .position(|c| c.eq_ignore_ascii_case("lat"));
    let (Some(lon_idx), Some(lat_idx)) = (lon_idx, lat_idx) else {
        return Vec::new();
    };
    result
        .rows
        .iter()
        .filter_map(|row| {
            let lon = row.get(lon_idx)?.parse::<f64>().ok()?;
            let lat = row.get(lat_idx)?.parse::<f64>().ok()?;
            Some((lon, lat))
        })
        .collect()
}

fn js_error_text(value: &JsValue) -> String {
    value
        .as_string()
        .filter(|message| !message.is_empty())
        .unwrap_or_else(|| format!("{value:?}"))
}
