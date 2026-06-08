test:
	cargo test --all-features

bench:
	cargo bench --all-features

test-minimal:
	cargo test --lib

test-production:
	cargo test --features production

clippy:
	cargo clippy --all-targets --all-features -- -D warnings

docs:
	cargo doc --no-deps --open

# Generate C4 architecture diagrams from workspace.dsl into docs/diagrams.
# Requires Docker (structurizr/structurizr + plantuml/plantuml images).
diagrams:
	@docker info >/dev/null 2>&1 || { echo "Docker is not running or not installed."; exit 1; }
	mkdir -p docs/diagrams
	docker run --rm -v "$(PWD)":/usr/local/structurizr structurizr/structurizr \
		export -workspace /usr/local/structurizr/docs/diagrams/workspace.dsl \
		-format plantuml/c4plantuml -output /usr/local/structurizr/docs/diagrams
	docker run --rm -v "$(PWD)/docs/diagrams":/data plantuml/plantuml -tpng /data/*.puml
	docker run --rm -v "$(PWD)/docs/diagrams":/data plantuml/plantuml -tsvg /data/*.puml
	@echo "Diagrams written to docs/diagrams/"

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
