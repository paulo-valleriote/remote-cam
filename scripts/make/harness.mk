.PHONY: harness residue

HARNESS_ARGS ?=
RESIDUE_EVIDENCE ?= ../docs/evidence/f1/rec-15-zero-persistence-linux.json

harness: ## Run the headless product harness end to end
	cd harness && timeout --signal=TERM --kill-after=5s 120s $(CARGO) run --quiet --locked --package remotecam-harness -- $(HARNESS_ARGS)

residue: ## Prove a finished session and a restart leave no residue (Linux); writes the versioned evidence file
	mkdir -p docs/evidence/f1
	cd harness && timeout --signal=TERM --kill-after=5s 120s $(CARGO) run --quiet --locked --package remotecam-residue -- --out $(RESIDUE_EVIDENCE)
