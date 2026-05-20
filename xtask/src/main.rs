use geolite::core::function_catalog::{
    SqliteFunctionSpec, SQLITE_DETERMINISTIC_FUNCTIONS, SQLITE_DIRECT_ONLY_FUNCTIONS,
};
use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

type Signature = (String, usize);

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let mut args = env::args().skip(1);
    let Some(cmd) = args.next() else {
        print_usage();
        return Err("missing command".to_string());
    };

    match cmd.as_str() {
        "precommit" => {
            let mut full = false;
            for arg in args {
                match arg.as_str() {
                    "--full" => full = true,
                    _ => return Err(format!("unknown precommit flag: {arg}")),
                }
            }
            precommit(full)
        }
        "install-hooks" => install_hooks(),
        "gen-function-surfaces" => {
            let mut check = false;
            for arg in args {
                match arg.as_str() {
                    "--check" => check = true,
                    _ => return Err(format!("unknown gen-function-surfaces flag: {arg}")),
                }
            }
            gen_function_surfaces(check)
        }
        "help" | "--help" | "-h" => {
            print_usage();
            Ok(())
        }
        _ => {
            print_usage();
            Err(format!("unknown command: {cmd}"))
        }
    }
}

fn print_usage() {
    println!("xtask commands:");
    println!("  precommit [--full]");
    println!("  install-hooks");
    println!("  gen-function-surfaces [--check]");
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask must live inside workspace")
        .to_path_buf()
}

fn precommit(full: bool) -> Result<(), String> {
    eprintln!("+ xtask gen-function-surfaces --check");
    gen_function_surfaces(true)?;

    let root = repo_root();
    let mut steps: Vec<Vec<&str>> = vec![
        vec!["cargo", "fmt", "--all", "--", "--check"],
        vec![
            "cargo",
            "clippy",
            "--workspace",
            "--all-targets",
            "--",
            "-D",
            "warnings",
        ],
        vec![
            "cargo",
            "clippy",
            "--workspace",
            "--all-features",
            "--all-targets",
            "--",
            "-D",
            "warnings",
        ],
        vec!["cargo", "test", "--workspace"],
        // The single sqlite-extension-gated test (the load_extension symbol
        // check) is not exercised by --workspace because the default feature
        // set is diesel-sqlite, not sqlite-extension. Run it explicitly here.
        vec![
            "cargo",
            "test",
            "-p",
            "geolite",
            "--features",
            "sqlite-extension",
            "--test",
            "sqlite_integration",
        ],
        vec!["cargo", "test", "--doc", "--workspace"],
    ];

    if full {
        steps.extend([
            vec![
                "cargo",
                "test",
                "-p",
                "geolite",
                "--features",
                "diesel-postgres",
                "--test",
                "diesel_postgres_integration",
            ],
            vec![
                "cargo",
                "test",
                "-p",
                "geolite",
                "--features",
                "sqlite",
                "--target",
                "wasm32-unknown-unknown",
                "--test",
                "sqlite_wasm",
            ],
            vec![
                "cargo",
                "test",
                "-p",
                "geolite",
                "--features",
                "diesel-sqlite",
                "--target",
                "wasm32-unknown-unknown",
                "--test",
                "diesel_wasm_integration",
            ],
            vec![
                "cargo",
                "clippy",
                "-p",
                "geolite",
                "--features",
                "sqlite",
                "--target",
                "wasm32-unknown-unknown",
                "--",
                "-D",
                "warnings",
            ],
        ]);
    }

    for step in steps {
        run_step(&root, &step)?;
    }
    Ok(())
}

fn run_step(cwd: &Path, args: &[&str]) -> Result<(), String> {
    let (bin, rest) = args
        .split_first()
        .ok_or_else(|| "empty command step".to_string())?;
    eprintln!("+ {}", args.join(" "));

    let status = Command::new(bin)
        .args(rest)
        .current_dir(cwd)
        .status()
        .map_err(io_err)?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("command failed: {}", args.join(" ")))
    }
}

fn install_hooks() -> Result<(), String> {
    let root = repo_root();
    let hook_path = root.join(".git/hooks/pre-commit");
    let script = format!(
        "#!/usr/bin/env sh\nset -eu\ncd \"{}\"\ncargo run --quiet -p xtask -- precommit\n",
        root.display()
    );

    if let Some(parent) = hook_path.parent() {
        fs::create_dir_all(parent).map_err(io_err)?;
    }
    fs::write(&hook_path, script).map_err(io_err)?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&hook_path).map_err(io_err)?.permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&hook_path, perms).map_err(io_err)?;
    }

    println!("installed pre-commit hook at {}", hook_path.display());
    Ok(())
}

fn gen_function_surfaces(check: bool) -> Result<(), String> {
    let root = repo_root();

    let deterministic_callbacks = render_sqlite_callbacks(
        "SQLITE_DETERMINISTIC_CALLBACKS",
        SQLITE_DETERMINISTIC_FUNCTIONS,
    );
    let direct_only_callbacks =
        render_sqlite_callbacks("SQLITE_DIRECT_ONLY_CALLBACKS", SQLITE_DIRECT_ONLY_FUNCTIONS);
    let diesel_functions = render_diesel_functions(&root)?;

    let sqlite_generated_dir = root.join("geolite/src/sqlite/generated");
    let diesel_generated_dir = root.join("geolite/src/diesel/generated");

    write_or_check(
        &sqlite_generated_dir.join("deterministic_callbacks.rs"),
        &deterministic_callbacks,
        check,
    )?;
    write_or_check(
        &sqlite_generated_dir.join("direct_only_callbacks.rs"),
        &direct_only_callbacks,
        check,
    )?;
    write_or_check(
        &diesel_generated_dir.join("functions.rs"),
        &diesel_functions,
        check,
    )?;

    if !check {
        println!("generated function surfaces from canonical catalog");
    }

    Ok(())
}

fn write_or_check(path: &Path, content: &str, check: bool) -> Result<(), String> {
    if check {
        let current = fs::read_to_string(path).map_err(|e| format!("{}: {}", path.display(), e))?;
        if current != content {
            return Err(format!(
                "generated file is stale: {} (run `cargo run -p xtask -- gen-function-surfaces`)",
                path.display()
            ));
        }
        return Ok(());
    }

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(io_err)?;
    }
    let needs_write = match fs::read_to_string(path) {
        Ok(current) => current != content,
        Err(_) => true,
    };
    if needs_write {
        fs::write(path, content).map_err(io_err)?;
    }
    Ok(())
}

fn render_sqlite_callbacks(const_name: &str, specs: &[SqliteFunctionSpec]) -> String {
    let mut name_counts: BTreeMap<&str, usize> = BTreeMap::new();
    for spec in specs {
        *name_counts.entry(spec.name).or_insert(0) += 1;
    }

    let mut out = String::new();
    out.push_str("// @generated by `cargo run -p xtask -- gen-function-surfaces`\n");
    out.push_str("// DO NOT EDIT BY HAND.\n\n");
    out.push_str(&format!("const {const_name}: &[SqliteCallbackSpec] = &[\n"));

    for spec in specs {
        let overloaded = name_counts.get(spec.name).copied().unwrap_or(0) > 1;
        let xfunc = callback_symbol(spec, overloaded);
        out.push_str(&format!(
            "    callback_spec!(\"{}\", {}, {}),\n",
            spec.name, spec.n_arg, xfunc
        ));
    }

    out.push_str("];\n");
    out
}

fn callback_symbol(spec: &SqliteFunctionSpec, overloaded: bool) -> String {
    if let Some(override_name) = spec.xfunc_override {
        return override_name.to_string();
    }

    let mut base = String::new();
    for ch in spec.name.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' {
            base.push(ch.to_ascii_lowercase());
        }
    }

    if overloaded {
        format!("{base}_{}_xfunc", spec.n_arg)
    } else {
        format!("{base}_xfunc")
    }
}

fn render_diesel_functions(root: &Path) -> Result<String, String> {
    let template_path = root.join("geolite/src/diesel/functions_template.rs");
    let template = fs::read_to_string(&template_path).map_err(io_err)?;
    let blocks = extract_diesel_define_blocks(&template)?;

    let catalog_signatures = catalog_signatures(SQLITE_DETERMINISTIC_FUNCTIONS);
    let catalog_set: BTreeSet<Signature> = catalog_signatures.iter().cloned().collect();

    let mut blocks_by_signature: BTreeMap<Signature, Vec<String>> = BTreeMap::new();
    for block in blocks {
        let sig = diesel_signature(&block)?;
        if !catalog_set.contains(&sig) {
            return Err(format!(
                "diesel template declaration is not in canonical catalog: {}({})",
                sig.0, sig.1
            ));
        }
        blocks_by_signature.entry(sig).or_default().push(block);
    }

    let mut out = String::new();
    out.push_str("// @generated by `cargo run -p xtask -- gen-function-surfaces`\n");
    out.push_str("// DO NOT EDIT BY HAND.\n\n");

    for signature in &catalog_signatures {
        let Some(sig_blocks) = blocks_by_signature.remove(signature) else {
            return Err(format!(
                "diesel declaration missing for catalog function: {}({})",
                signature.0, signature.1
            ));
        };
        for block in sig_blocks {
            out.push_str(block.trim_end());
            out.push_str("\n\n");
        }
    }

    if !blocks_by_signature.is_empty() {
        let extras: Vec<String> = blocks_by_signature
            .keys()
            .map(|(name, argc)| format!("{}({})", name, argc))
            .collect();
        return Err(format!(
            "diesel template has declarations not consumed by catalog ordering: {}",
            extras.join(", ")
        ));
    }

    Ok(out)
}

fn catalog_signatures(specs: &[SqliteFunctionSpec]) -> Vec<Signature> {
    let mut out = Vec::new();
    let mut seen = BTreeSet::new();
    for spec in specs {
        let sig = (spec.name.to_ascii_uppercase(), spec.n_arg as usize);
        if seen.insert(sig.clone()) {
            out.push(sig);
        }
    }
    out
}

fn extract_diesel_define_blocks(src: &str) -> Result<Vec<String>, String> {
    let needle = "diesel::define_sql_function! {";
    let bytes = src.as_bytes();
    let mut blocks = Vec::new();
    let mut pos = 0;

    while let Some(rel_start) = src[pos..].find(needle) {
        let start = pos + rel_start;
        let mut i = start + needle.len();
        let mut depth = 1usize;

        while i < src.len() {
            match bytes[i] {
                b'{' => depth += 1,
                b'}' => {
                    depth -= 1;
                    if depth == 0 {
                        i += 1;
                        break;
                    }
                }
                _ => {}
            }
            i += 1;
        }

        if depth != 0 {
            return Err("unterminated diesel::define_sql_function! block in template".to_string());
        }

        blocks.push(src[start..i].to_string());
        pos = i;
    }

    Ok(blocks)
}

fn diesel_signature(block: &str) -> Result<Signature, String> {
    let fn_idx = block
        .find("fn ")
        .ok_or_else(|| "missing `fn` in diesel declaration block".to_string())?;
    let fn_start = fn_idx + "fn ".len();
    let (fn_name, args) = parse_name_and_args_after_fn(block, fn_start)
        .ok_or_else(|| "failed to parse diesel declaration function signature".to_string())?;

    let sql_name = sql_name_override(block).unwrap_or(fn_name);
    let arg_count = if args.trim().is_empty() {
        0
    } else {
        args.split(',').filter(|arg| !arg.trim().is_empty()).count()
    };

    Ok((sql_name.to_ascii_uppercase(), arg_count))
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

fn io_err(e: io::Error) -> String {
    e.to_string()
}
