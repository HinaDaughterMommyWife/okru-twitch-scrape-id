.PHONY: install build build-release build-oracle-arm run clean up down logs tunnel tunnel-down

BACKEND_DIR := backend
DIST_DIR := dist
ARM_DIR := arm

# Local dev: native release binary → ./dist
install: build-release
	mkdir -p $(DIST_DIR)
	cp $(BACKEND_DIR)/target/release/okru-backend $(DIST_DIR)/okru-backend
	@if [ ! -f $(DIST_DIR)/config.toml ]; then \
		cp $(BACKEND_DIR)/config.example.toml $(DIST_DIR)/config.toml; \
		echo "Wrote $(DIST_DIR)/config.toml — edit before running."; \
	fi
	@echo "Binary: $(DIST_DIR)/okru-backend"

build:
	cd $(BACKEND_DIR) && cargo build

build-release:
	cd $(BACKEND_DIR) && cargo build --release

# Cross-build for Oracle Linux 8 ARM (aarch64, glibc 2.28) via cargo-zigbuild.
# Builds on host arch — no ARM emulation required. Output: ./arm/okru-backend
build-oracle-arm:
	mkdir -p $(ARM_DIR)
	docker buildx build \
		-f $(BACKEND_DIR)/Dockerfile.oraclelinux-arm \
		--target export \
		--output type=local,dest=$(ARM_DIR) \
		$(BACKEND_DIR)
	@if [ ! -f $(ARM_DIR)/config.toml ]; then \
		cp $(BACKEND_DIR)/config.example.toml $(ARM_DIR)/config.toml; \
		echo "Wrote $(ARM_DIR)/config.toml — edit before running."; \
	fi
	@file $(ARM_DIR)/okru-backend
	@echo "ARM binary: $(ARM_DIR)/okru-backend"

run:
	cd $(DIST_DIR) && ./okru-backend

clean:
	cd $(BACKEND_DIR) && cargo clean
	rm -rf $(DIST_DIR) $(ARM_DIR)

# Local dev stack via pm2 (backend + worker + web)
up: install
	mkdir -p logs
	pm2 start ecosystem.config.cjs

down:
	pm2 delete ecosystem.config.cjs || true

logs:
	pm2 logs

# Optional: anonymous cloudflared tunnel → localhost:9622 (OAuth setup from outside)
tunnel:
	docker compose up -d tunnel

tunnel-down:
	docker compose down
