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
