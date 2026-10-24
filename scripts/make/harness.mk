.PHONY: harness

harness: ## Run the headless product harness end to end
	cd harness && $(CARGO) run --quiet --locked --package remotecam-harness
