#!/bin/bash
set -eu

cd "$SRC/sqlitegis"
# the base image exports its own nightly as RUSTUP_TOOLCHAIN, so no toolchain is named here
cargo fuzz build -O --debug-assertions --fuzz-dir fuzz

targets=$(cargo fuzz list --fuzz-dir fuzz)
if [[ -z "$targets" ]]; then
    echo "cargo fuzz list named no target" >&2
    exit 1
fi

target_dir=fuzz/target/x86_64-unknown-linux-gnu/release
for name in $targets; do
    cp "$target_dir/$name" "$OUT/"
    # the runner unpacks <target>_seed_corpus.zip as the starting corpus
    seeds="fuzz/seeds/$name"
    if [[ ! -d "$seeds" ]] || [[ -z "$(ls -A "$seeds")" ]]; then
        echo "fuzz target $name has no seeds in $seeds" >&2
        exit 1
    fi
    zip -qj "$OUT/${name}_seed_corpus.zip" "$seeds"/*
done

# the runner image has no GEOS, so the differential target ships its own copy;
# DT_RPATH (unlike RUNPATH) also resolves libgeos_c's own dependency on libgeos
mkdir -p "$OUT/lib"
cp /usr/lib/x86_64-linux-gnu/libgeos_c.so.1 /usr/lib/x86_64-linux-gnu/libgeos-*.so "$OUT/lib/"
patchelf --force-rpath --set-rpath '$ORIGIN/lib' "$OUT/fuzz_geos_differential"
