set shell := ["bash", "-cu"]

export ORT_DYLIB_PATH := justfile_directory() + "/src-tauri/resources/libonnxruntime.so"
export MODELS_DIR := env_var_or_default("RAPIDRAW_MODELS_DIR", env_var_or_default("HOME", "") + "/.local/share/io.github.CyberTimon.RapidRAW/models")

default := "check"

# toolchain + all dev deps (bun, nightly, nextest, typos, onnx models)
@setup:
    bun install
    rustup toolchain install nightly
    cargo-nextest --version >/dev/null 2>&1 || cargo install cargo-nextest --locked
    typos --version >/dev/null 2>&1 || cargo install typos-cli --locked
    hf --version >/dev/null 2>&1 || (uv tool install --upgrade 'huggingface_hub[cli]' || true)
    mkdir -p {{ MODELS_DIR }}
    hf download CyberTimon/RapidRAW-Models --local-dir {{ MODELS_DIR }} \
        || echo "models skipped; fetch with: hf download CyberTimon/RapidRAW-Models --local-dir {{ MODELS_DIR }}"
    echo "onnx runtime downloads/verifies on the first cargo build"

# list recipes
@default:
    just --list

# typecheck the workspace
@check:
    cargo check --workspace --all-targets

# clippy (pedantic) + spelling
@lint:
    cargo clippy --workspace --all-targets
    typos --locale en-gb

# test suite
@test:
    cargo nextest run --workspace

# frontend + backend build
@build:
    bun run build
    cargo build --workspace

# lint, test, build
@verify: lint test build

# run the app in dev mode (patches wayland env when the shell has none)
@run:
    export XDG_RUNTIME_DIR="${XDG_RUNTIME_DIR:-/run/user/$(id -u)}"
    export WAYLAND_DISPLAY="${WAYLAND_DISPLAY:-$(ls "$XDG_RUNTIME_DIR" 2>/dev/null | grep -oE 'wayland-[0-9]+' | tail -1)}"
    export DISPLAY="${DISPLAY:-:0}"
    bun run start

# run the app with camera tethering
@run-tethering:
    export XDG_RUNTIME_DIR="${XDG_RUNTIME_DIR:-/run/user/$(id -u)}"
    export WAYLAND_DISPLAY="${WAYLAND_DISPLAY:-$(ls "$XDG_RUNTIME_DIR" 2>/dev/null | grep -oE 'wayland-[0-9]+' | tail -1)}"
    export DISPLAY="${DISPLAY:-:0}"
    bun run start:tethering

# cpu image/pipeline benchmarks (e.g. `just bench image_ops`)
@bench bench="image_ops":
    cargo bench -p RapidRAW --bench {{ bench }}

# onnx model benchmarks (models via `hf download CyberTimon/RapidRAW-Models`)
@bench-ai bench="ai_ops":
    cargo bench -p RapidRAW --bench {{ bench }}

# simd kernel benches (nightly)
@bench-simd:
    cargo +nightly bench -p simd_kernels

# engine bench target
@bench-engine:
    cargo bench -p engine

# rules qa gate (engine + simd must be problem-free)
@rules:
    rules -q

# format rust + check frontend types
@fmt:
    cargo fmt --all
    bunx tsc --noEmit