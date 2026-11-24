# Build a Rust service whose types are generated from JSON schemas.
SCHEMAS := $(wildcard schema/*.json)
GEN     := src/gen.rs
CARGO   ?= cargo

.PHONY: all build release test lint clean

all: build

# Regenerate only when a schema or the generator changed.
$(GEN): $(SCHEMAS) tools/codegen.py
	python3 tools/codegen.py schema/ > $(GEN)

build: $(GEN)
	$(CARGO) build

release: $(GEN)
	$(CARGO) build --release

test: $(GEN)
	$(CARGO) test

lint: $(GEN)
	$(CARGO) fmt --check
	$(CARGO) clippy -- -D warnings

clean:
	rm -rf target $(GEN)
