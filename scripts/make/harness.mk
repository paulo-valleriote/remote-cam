.PHONY: harness

harness: ## Run the headless product harness end to end
	cd harness && timeout --signal=TERM --kill-after=5s 120s $(CARGO) run --quiet --locked --package remotecam-harness
