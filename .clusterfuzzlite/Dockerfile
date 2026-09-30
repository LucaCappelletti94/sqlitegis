FROM gcr.io/oss-fuzz-base/base-builder-rust
# fuzz_geos_differential links the system GEOS as its oracle
RUN apt-get update && apt-get install -y --no-install-recommends libgeos-dev pkg-config patchelf && rm -rf /var/lib/apt/lists/*
COPY . $SRC/sqlitegis
WORKDIR $SRC/sqlitegis
COPY ./.clusterfuzzlite/build.sh $SRC/
