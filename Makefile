test:
	cargo test --all-features

bench:
	cargo bench --all-features

test-minimal:
	cargo test --lib

test-production:
	cargo test --features production

docs:
	cargo doc --no-deps --open

lint:
	cargo clippy --all-targets --all-features -- -D warnings

audit:
	cargo audit

coverage:
	cargo llvm-cov --html --no-cfg-coverage --open --all-features -- tests

readme:
	cargo readme > README.md

publish:
	cargo publish
