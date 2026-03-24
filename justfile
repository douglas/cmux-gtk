# cmux-gtk task runner

# Build debug binary
build:
    cargo build --features cmux/link-ghostty

# Build release binary
release:
    cargo build --release --features cmux/link-ghostty

# Run the application (debug)
run:
    cargo run --features cmux/link-ghostty

# Run all tests
test:
    cargo test --workspace

# Run clippy lints
lint:
    cargo clippy --workspace -- -D warnings

# Check formatting
fmt-check:
    cargo fmt --all --check

# Format code
fmt:
    cargo fmt --all

# Full CI check (test + lint + fmt)
ci: test lint fmt-check

# Install release binary to ~/.cargo/bin
install:
    cargo install --path cmux --features link-ghostty
    cargo install --path cmux-cli

# Cross-compile cmuxd-remote for all supported platforms
build-daemon VERSION=(env_var_or_default("CARGO_PKG_VERSION", "dev")):
    #!/usr/bin/env bash
    set -euo pipefail
    LDFLAGS="-X main.version={{VERSION}}"
    mkdir -p artifacts
    for TARGET in linux-amd64 linux-arm64 darwin-amd64 darwin-arm64; do
        GOOS="${TARGET%-*}" GOARCH="${TARGET#*-}" CGO_ENABLED=0 \
            go build -ldflags "${LDFLAGS}" \
            -o "artifacts/cmuxd-remote-${TARGET}" \
            ./daemon/remote/cmd/cmuxd-remote
        echo "Built artifacts/cmuxd-remote-${TARGET}"
    done
    cd artifacts && sha256sum cmuxd-remote-* > checksums-sha256.txt
    echo "Checksums written to artifacts/checksums-sha256.txt"

# Run Go tests for the remote daemon
test-daemon:
    cd daemon/remote && go test ./...
