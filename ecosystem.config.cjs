module.exports = {
  apps: [
    {
      name: "okru-backend",
      cwd: "./backend",
      script: ".venv/bin/python",
      args: "run.py",
      interpreter: "none",
      watch: false,
      autorestart: true,
      env: {
        PYTHONUNBUFFERED: "1",
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

