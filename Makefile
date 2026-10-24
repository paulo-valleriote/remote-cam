.DEFAULT_GOAL := help

SHELL := /bin/bash

include scripts/make/common.mk
include scripts/make/desktop.mk
include scripts/make/harness.mk
include scripts/make/mobile.mk
include scripts/make/quality.mk
include scripts/make/spikes.mk
