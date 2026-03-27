CLI_VENV := $(abspath ../reeln-cli/.venv)

.PHONY: build test check install clean

build:
	cargo build --release

test:
	cargo test --workspace

check:
	cargo clippy --workspace -- -D warnings
	cargo fmt --check
	$(MAKE) test

install:
	cd crates/reeln-python && VIRTUAL_ENV=$(CLI_VENV) maturin develop --release

clean:
	cargo clean
