.PHONY: build-desktop

build-desktop: ## Build the Rust desktop workspace
	$(CARGO) build --manifest-path desktop/Cargo.toml --workspace
