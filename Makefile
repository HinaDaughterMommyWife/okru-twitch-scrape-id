.PHONY: up down logs install

install:
	cd backend && uv venv --python python3 && uv pip install -e .
	cd web && pnpm install

up:
	pm2 start ecosystem.config.cjs

down:
	pm2 delete ecosystem.config.cjs

logs:
	pm2 logs
