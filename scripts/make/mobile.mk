.PHONY: build-mobile

build-mobile: ## Assemble the pure-JVM contract libraries (mobile/protocol and mobile/session-core jars)
	$(GRADLE) -p mobile :protocol:assemble :session-core:assemble
