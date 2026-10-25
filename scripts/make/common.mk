PYTHON ?= python3
# Resolve a relative interpreter path (for example spikes/tools/.venv/bin/python)
# before a recipe changes directory. Bare commands such as python3 pass through.
PYTHON_BIN = $(if $(wildcard $(PYTHON)),$(abspath $(PYTHON)),$(PYTHON))
CARGO ?= cargo
GRADLE ?= mobile/gradlew

define need_script
@test -f $(1) || { echo "error: $(1) is not implemented yet; see specs/001-f0-technical-spikes/tasks.md." >&2; exit 1; }
endef

.PHONY: help clean install

help: ## Show the available targets
	@awk 'BEGIN {FS = ":.*## "; print "Usage: make <target>\n"} /^[a-zA-Z0-9_-]+:.*## / {printf "  %-26s %s\n", $$1, $$2}' $(MAKEFILE_LIST)

install: ## Install the Python measurement package and its development dependencies
	$(PYTHON) -m pip install -e "spikes/tools[dev]"

clean: ## Remove generated build artefacts and never committed evidence
	$(CARGO) clean --manifest-path desktop/Cargo.toml
	$(CARGO) clean --manifest-path harness/Cargo.toml
	@if command -v $(GRADLE) >/dev/null 2>&1; then $(GRADLE) -p mobile clean; else echo "gradle not found; skipped mobile clean"; fi
	rm -rf spikes/evidence/tmp
