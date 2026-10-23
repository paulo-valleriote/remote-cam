.PHONY: build-desktop build-desktop-win sync-desktop-win

build-desktop: ## Build the Rust desktop workspace
	$(CARGO) build --manifest-path desktop/Cargo.toml --workspace

# ---------------------------------------------------------------------------
# Windows-native build from WSL2 (docs/windows-build.md)
#
# The repository normally lives on the WSL ext4 filesystem and the Windows
# toolchain cannot run an incremental rustc session across the \\wsl.localhost
# UNC share (ERROR_INVALID_FUNCTION on the session lock). The `-win` targets
# mirror the working tree to the NTFS filesystem and build it with the MSVC
# toolchain on the Windows side. Requires WSL2 with Windows interop.
# ---------------------------------------------------------------------------

WIN_USER ?= $(shell cmd.exe /c echo %USERNAME% 2>/dev/null | tr -d '\r')
WIN_MIRROR ?= /mnt/c/Users/$(WIN_USER)/remotecam-win
WIN_RELEASE ?= 0
WIN_TARGET_PROFILE = $(if $(filter 1,$(WIN_RELEASE)),release,debug)

sync-desktop-win: ## Mirror the working tree to the Windows filesystem (NTFS) for native builds
	@test -d /mnt/c/Users || { echo "error: /mnt/c/Users not found; this target requires WSL2 with Windows interop." >&2; exit 1; }
	@test -d "/mnt/c/Users/$(WIN_USER)" || { echo "error: /mnt/c/Users/$(WIN_USER) not found; run with WIN_USER=<Windows username>." >&2; exit 1; }
	rm -rf "$(WIN_MIRROR)"
	mkdir -p "$(WIN_MIRROR)"
	rsync -a --delete \
		--exclude .git/ --exclude target/ --exclude build/ \
		--exclude node_modules/ --exclude .coverage/ \
		./ "$(WIN_MIRROR)/"

build-desktop-win: sync-desktop-win ## Build the Rust desktop workspace natively on Windows from WSL2
	@test -n "$(WIN_USER)" || { echo "error: could not detect the Windows username; run with WIN_USER=<name>." >&2; exit 1; }
	cmd.exe /c "cd /d C:\Users\$(WIN_USER)\remotecam-win && cargo build --manifest-path desktop\Cargo.toml --workspace $(if $(filter 1,$(WIN_RELEASE)),--release)"
	@echo "Windows build complete: $(WIN_MIRROR)/desktop/target/$(WIN_TARGET_PROFILE)/"
	@echo "The mirror $(WIN_MIRROR) is regenerated from the current working tree at every build; edit files only in this repository."
