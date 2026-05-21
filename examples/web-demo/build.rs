//! Generates the dedicated wasm worker assets used by the Dioxus web demo.
//!
//! Spawns `cargo build --package sqlitegis-web-demo-worker --target
//! wasm32-unknown-unknown`, runs `wasm-bindgen` programmatically on the
//! resulting `.wasm`, and deposits the JS shim + the bindgen output into
//! `public/generated/` where the UI bundle can load them via
//!
//!     new Worker("/generated/sqlitegis-worker.js", { type: "module" })

use std::{
    env, fs,
    path::{Path, PathBuf},
    process::Command,
};

const WORKER_PACKAGE: &str = "sqlitegis-web-demo-worker";
const WORKER_STEM: &str = "sqlitegis_web_demo_worker";
const WORKER_LOADER_NAME: &str = "sqlitegis-worker.js";

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=Cargo.lock");
    println!("cargo:rerun-if-changed=worker/Cargo.toml");
    println!("cargo:rerun-if-changed=worker/src");
    println!("cargo:rerun-if-changed=protocol/Cargo.toml");
    println!("cargo:rerun-if-changed=protocol/src");
    // The worker pulls sqlitegis from ../../sqlitegis via path, so re-run
    // whenever the parent library source changes too.
    println!("cargo:rerun-if-changed=../../sqlitegis/src");

    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    if should_skip_worker_build() {
        println!("cargo:warning=skipping worker asset build");
        return Ok(());
    }

    let manifest_dir = PathBuf::from(
        env::var_os("CARGO_MANIFEST_DIR")
            .ok_or_else(|| "cargo did not provide CARGO_MANIFEST_DIR".to_owned())?,
    );
    let generated_dir = manifest_dir.join("public/generated");
    build_worker_assets(&manifest_dir, &generated_dir)
}

fn should_skip_worker_build() -> bool {
    env::var_os("CARGO_CFG_COVERAGE").is_some()
        || env::var_os("SQLITEGIS_SKIP_WEB_WORKER_BUILD").is_some()
}

fn build_worker_assets(workspace_root: &Path, generated_dir: &Path) -> Result<(), String> {
    fs::create_dir_all(generated_dir)
        .map_err(|error| format!("failed to create generated worker directory: {error}"))?;

    let out_dir = PathBuf::from(
        env::var_os("OUT_DIR").ok_or_else(|| "cargo did not provide OUT_DIR".to_owned())?,
    );
    let bindgen_dir = out_dir.join("sqlitegis-worker-bindgen");
    let target_dir = out_dir.join("sqlitegis-worker-target");
    let _ignored = fs::remove_dir_all(&bindgen_dir);
    fs::create_dir_all(&bindgen_dir)
        .map_err(|error| format!("failed to create worker bindgen directory: {error}"))?;

    let cargo = env::var("CARGO").unwrap_or_else(|_| String::from("cargo"));
    let profile = env::var("PROFILE").unwrap_or_else(|_| String::from("debug"));

    let mut build = Command::new(cargo);
    build
        .current_dir(workspace_root)
        .env_remove("RUSTFLAGS")
        .env_remove("CARGO_ENCODED_RUSTFLAGS")
        .args([
            "build",
            "--package",
            WORKER_PACKAGE,
            "--lib",
            "--target",
            "wasm32-unknown-unknown",
            "--target-dir",
        ])
        .arg(&target_dir);

    match profile.as_str() {
        "debug" => {}
        "release" => {
            build.arg("--release");
        }
        other => {
            build.args(["--profile", other]);
        }
    }

    let status = build
        .status()
        .map_err(|error| format!("failed to launch worker cargo build: {error}"))?;
    if !status.success() {
        return Err(format!("worker cargo build failed with status {status}"));
    }

    let worker_wasm = target_dir
        .join("wasm32-unknown-unknown")
        .join(&profile)
        .join(format!("{WORKER_STEM}.wasm"));
    if !worker_wasm.exists() {
        return Err(format!("expected worker wasm at {}", worker_wasm.display()));
    }

    let mut bindgen = wasm_bindgen_cli_support::Bindgen::new();
    bindgen
        .input_path(&worker_wasm)
        .out_name(WORKER_STEM)
        .typescript(false)
        .web(true)
        .map_err(|error| format!("failed to configure worker bindgen for web output: {error}"))?
        .generate(&bindgen_dir)
        .map_err(|error| format!("worker bindgen generation failed: {error}"))?;

    copy_file(
        &bindgen_dir.join(format!("{WORKER_STEM}.js")),
        &generated_dir.join(format!("{WORKER_STEM}.js")),
    )?;
    copy_file(
        &bindgen_dir.join(format!("{WORKER_STEM}_bg.wasm")),
        &generated_dir.join(format!("{WORKER_STEM}_bg.wasm")),
    )?;
    fs::write(
        generated_dir.join(WORKER_LOADER_NAME),
        worker_loader_script(),
    )
    .map_err(|error| format!("failed to write worker bootstrap script: {error}"))?;
    Ok(())
}

fn copy_file(source: &Path, destination: &Path) -> Result<(), String> {
    fs::copy(source, destination).map_err(|error| {
        format!(
            "failed to copy generated worker asset from {} to {}: {error}",
            source.display(),
            destination.display()
        )
    })?;
    Ok(())
}

fn worker_loader_script() -> &'static str {
    r#"import init from "./sqlitegis_web_demo_worker.js";

await init({ module_or_path: new URL("./sqlitegis_web_demo_worker_bg.wasm", import.meta.url) });
"#
}
