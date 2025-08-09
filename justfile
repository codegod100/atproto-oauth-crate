# Justfile for developer tasks

# Generate code from lexicons using esquema-cli
codegen:
	esquema-cli generate local -l ./examples/lexicons/ -o ./examples --module codegen

# Watch and rerun the basic_usage example on file changes (requires cargo-watch: `cargo install cargo-watch`)
watch-example:
	which cargo-watch >/dev/null || (echo "cargo-watch not installed. Install with: cargo install cargo-watch" && exit 1)
	cargo watch -x 'run --example basic_usage'
