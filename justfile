set windows-shell := ["pwsh.exe", "-Nologo", "-Command"]

export RUST_LOG := "info,wgpu_core=error"
export RUST_BACKTRACE := "1"

[private]
default:
    @just --list

# Build the workspace
build:
    cargo build

# Check the workspace
check:
    cargo check --all --tests
    cargo fmt --all -- --check

# Show the workspace documentation
docs:
    cargo doc --open -p app

# Fix all automatically resolvable lints with clippy
fix:
    cargo clippy --all --tests --fix --allow-dirty --allow-staged

# Autoformat the workspace
format:
    cargo fmt --all

# Install wasm tooling
init-wasm:
  rustup target add wasm32-unknown-unknown
  cargo install --locked trunk

# Lint the workspace
lint:
    cargo clippy --all --tests -- -D warnings

# Run the desktop app
run $project="main":
    cargo run --bin {{project}}

# Build the app with wgpu + WebGL
build-webgl:
    cd main && trunk build --features webgl

# Build the app with wgpu + WebGPU
build-webgpu:
    cd main && trunk build --features webgpu

# Serve the app with wgpu + WebGL
run-webgl:
    cd main && trunk serve --features webgl --open

# Serve the app with wgpu + WebGPU
run-webgpu:
    cd main && trunk serve --features webgpu --open

# Run the test suite
test:
    cargo test --all -- --nocapture

# Check for unused dependencies with cargo-machete
udeps:
  cargo machete

# Watch for changes and rebuild the app
watch $project="main":
    cargo watch -x 'run -p {{project}}'

# Display toolchain versions
@versions:
    rustc --version
    cargo fmt -- --version
    cargo clippy -- --version
