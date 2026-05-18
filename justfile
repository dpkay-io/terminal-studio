# Terminal Studio — development commands

# Run in debug mode
dev:
    cargo run

# Run all tests
test:
    cargo test

# Run clippy lints (deny warnings)
lint:
    cargo clippy --all-targets -- -D warnings

# Check formatting
fmt:
    cargo fmt --check

# Auto-fix formatting
fmt-fix:
    cargo fmt

# Optimized release build
build-release:
    cargo build --release

# Run all checks (format, lint, test)
check-all: fmt lint test

# Audit dependencies for security advisories and license issues
audit:
    cargo deny check
