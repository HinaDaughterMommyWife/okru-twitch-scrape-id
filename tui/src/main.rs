//! okru-tui — CRUD for okru users (channels) stored in SQLite.
//!
//! The DB path is shared with okru-backend (`okru_tui::db::default_db_path`):
//! dev builds use `<workspace>/data/okru.db`, release builds `okru.db` next to the binary.

mod app;
mod form;
mod input;
mod link;
mod ui;

use std::path::PathBuf;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use okru_tui::db::Store;
use okru_tui::config::SharedConfig;
use ratatui::crossterm::event::{self, Event, KeyEventKind};

use crate::app::App;
use crate::link::Link;

const USAGE: &str = "okru-tui [--db <ruta>] [--config <ruta>]

  --db <ruta>       SQLite a usar (default: OKRU_DB, o data/okru.db en dev / okru.db junto al binario)
  --config <ruta>   config.toml del backend: ipcPort y webURL (default: OKRU_CONFIG, o junto al binario)";

struct Args {
    db: PathBuf,
    config: PathBuf,
}

fn parse_args() -> Result<Args> {
    let mut args = std::env::args().skip(1);
    let mut db = None;
    let mut config = None;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--db" => db = Some(PathBuf::from(args.next().context("--db necesita una ruta")?)),
            "--config" => {
                config = Some(PathBuf::from(args.next().context("--config necesita una ruta")?))
            }
            "-h" | "--help" => {
                println!("{USAGE}");
                std::process::exit(0);
            }
            other => bail!("argumento desconocido: {other}\n\n{USAGE}"),
        }
    }
    Ok(Args {
        db: db.unwrap_or_else(okru_tui::db::default_db_path),
        config: config.unwrap_or_else(okru_tui::db::default_config_path),
    })
}

fn main() -> Result<()> {
    let Args { db: db_path, config } = parse_args()?;
    let store = Store::open(&db_path).with_context(|| format!("abrir {}", db_path.display()))?;
    let shared = SharedConfig::load(&config)
        .with_context(|| format!("leer {}", config.display()))?;
    let link = Link::new(shared.ipc_port);
    let mut app = App::new(store, db_path, shared, link)?;

    // Installs a panic hook that restores the terminal.
    let mut terminal = ratatui::init();
    let result = run(&mut terminal, &mut app);
    ratatui::restore();
    result
}

fn run(terminal: &mut ratatui::DefaultTerminal, app: &mut App) -> Result<()> {
    while !app.should_quit {
        terminal.draw(|frame| ui::draw(frame, app))?;
        if event::poll(Duration::from_millis(250))? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    app.on_key(key);
                }
            }
        }
        app.on_tick();
    }
    Ok(())
}
