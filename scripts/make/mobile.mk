.PHONY: build-mobile

build-mobile: ## Build the Android debug application
	$(GRADLE) -p mobile :app:assembleDebug
