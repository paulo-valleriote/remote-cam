.PHONY: build-mobile

build-mobile: ## Assemble the Android contract library (mobile/protocol/build/outputs/aar)
	$(GRADLE) -p mobile :protocol:assembleDebug
