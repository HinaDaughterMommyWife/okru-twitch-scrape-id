.PHONY: install build build-release build-oracle-arm run tui dev-backend dev-tui test clean up down logs tunnel tunnel-down

BACKEND_DIR := backend
DIST_DIR := dist
ARM_DIR := arm
TARGET_DIR := target
BINS := okru-backend okru-tui
# Every local process shares this DB (pm2 via ecosystem.config.cjs, tui.sh, dev builds).
# Only the server (arm/) keeps okru.db next to the binaries.
LOCAL_DB := $(CURDIR)/data/okru.db
# …and this config.toml (ipcPort, worker, twitch). credentials.json lives next to it.
LOCAL_CONFIG := $(CURDIR)/$(DIST_DIR)/config.toml
CARGO_BINS := -p okru-backend -p okru-tui --bin okru-backend --bin okru-tui

# Local prod-like build: release binaries → ./dist (config.toml next to them; DB = $(LOCAL_DB))
install: build-release
	mkdir -p $(DIST_DIR)
	@# install replaces the file (new inode): works while pm2 is running the old binary
	$(foreach bin,$(BINS),install -m 755 $(TARGET_DIR)/release/$(bin) $(DIST_DIR)/$(bin) &&) true
	@if [ ! -f $(DIST_DIR)/config.toml ]; then \
		cp $(BACKEND_DIR)/config.example.toml $(DIST_DIR)/config.toml; \
		echo "Wrote $(DIST_DIR)/config.toml — edit before running."; \
	fi
	@echo "Binaries: $(foreach bin,$(BINS),$(DIST_DIR)/$(bin))"

build:
	cargo build $(CARGO_BINS)

build-release:
	cargo build --release $(CARGO_BINS)

# Dev (debug builds): backend + TUI share the workspace DB at ./data/okru.db
dev-backend:
	OKRU_CONFIG=$(LOCAL_CONFIG) cargo run -p okru-backend --bin okru-backend

dev-tui:
	./tui.sh

test:
	cargo test --workspace

# Cross-build for Oracle Linux 8 ARM (aarch64, glibc 2.28) via cargo-zigbuild.
# Builds on host arch — no ARM emulation required. Output: ./arm/okru-backend + ./arm/okru-tui
build-oracle-arm:
	mkdir -p $(ARM_DIR)
	docker buildx build \
		-f $(BACKEND_DIR)/Dockerfile.oraclelinux-arm \
		--target export \
		--output type=local,dest=$(ARM_DIR) \
		.
	@if [ ! -f $(ARM_DIR)/config.toml ]; then \
		cp $(BACKEND_DIR)/config.example.toml $(ARM_DIR)/config.toml; \
		echo "Wrote $(ARM_DIR)/config.toml — edit before running."; \
	fi
	@file $(foreach bin,$(BINS),$(ARM_DIR)/$(bin))
	@echo "ARM binaries: $(foreach bin,$(BINS),$(ARM_DIR)/$(bin))"

run:
	cd $(DIST_DIR) && OKRU_DB=$(LOCAL_DB) ./okru-backend

# Release TUI on the shared local DB (same one the pm2 backend uses)
tui:
	cd $(DIST_DIR) && OKRU_DB=$(LOCAL_DB) ./okru-tui

clean:
	cargo clean
	rm -rf $(DIST_DIR) $(ARM_DIR)

# Local dev stack via pm2 (backend + worker + web)
up: install
	mkdir -p logs
	pm2 startOrRestart ecosystem.config.cjs --update-env

down:
	pm2 delete ecosystem.config.cjs || true

logs:
	pm2 logs

# Optional: anonymous cloudflared tunnel → localhost:9622 (OAuth setup from outside)
tunnel:
	docker compose up -d tunnel

tunnel-down:
	docker compose down
