const path = require("node:path");

// One SQLite + one config.toml for every local process (pm2 backend, ./tui.sh, make tui/dev-*).
const LOCAL_DB = path.join(__dirname, "data", "okru.db");
const LOCAL_CONFIG = path.join(__dirname, "dist", "config.toml");

module.exports = {
  apps: [
    {
      name: "okru-backend",
      cwd: "./dist",
      script: "./okru-backend",
      interpreter: "none",
      watch: false,
      autorestart: true,
      env: {
        RUST_LOG: "info",
        OKRU_DB: LOCAL_DB,
        OKRU_CONFIG: LOCAL_CONFIG,
      },
      out_file: "../logs/backend.out.log",
      error_file: "../logs/backend.err.log",
      log_date_format: "YYYY-MM-DD HH:mm:ss",
    },
    {
      name: "okru-worker",
      cwd: "./worker",
      script: "../web/node_modules/.bin/wrangler",
      args: "dev --port 8787 --local",
      interpreter: "none",
      watch: false,
      autorestart: true,
      out_file: "../logs/worker.out.log",
      error_file: "../logs/worker.err.log",
      log_date_format: "YYYY-MM-DD HH:mm:ss",
    },
    {
      name: "okru-web",
      cwd: "./web",
      script: "node_modules/.bin/astro",
      args: "dev",
      interpreter: "none",
      watch: false,
      autorestart: true,
      out_file: "../logs/web.out.log",
      error_file: "../logs/web.err.log",
      log_date_format: "YYYY-MM-DD HH:mm:ss",
    },
  ],
};
