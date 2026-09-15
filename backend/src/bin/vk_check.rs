//! One-shot VK scrape: same path as the bot (`vk::prefetch`).
//! No bot, OAuth, scheduler, or worker POST.
//!
//! ```text
//! cargo run --bin vk-check -- id1117440596
//! cargo run --bin vk-check -- https://vk.com/id1117440596
//! ```

// Shared with okru-backend; each debug bin only uses part of it.
#[path = "../vk.rs"]
#[allow(dead_code)]
mod vk;

use anyhow::Result;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let arg = std::env::args().nth(1);
    if matches!(arg.as_deref(), Some("-h" | "--help")) {
        eprintln!(
            "Usage: vk-check [idvk]\n\
             \n\
             Runs vk::prefetch against a VK profile/community.\n\
             Default idvk: id1117440596\n\
             \n\
             Examples:\n\
               cargo run --bin vk-check\n\
               cargo run --bin vk-check -- id1117440596\n\
               cargo run --bin vk-check -- https://vk.com/id1117440596"
        );
        return Ok(());
    }

    let idvk = arg.filter(|s| !s.trim().is_empty()).unwrap_or_else(|| {
        "id1117440596".into()
    });

    let http = reqwest::Client::builder()
        .user_agent("okru-backend/0.1")
        .gzip(true)
        .build()?;

    tracing::info!("vk::prefetch idvk={idvk}");
    let result = vk::prefetch(&http, &idvk).await?;

    println!("found={}", result.found);
    println!("vk_oid={}", result.vk_oid);
    println!("vk_id={}", result.vk_id);
    println!("vods={}", result.items.len());
    Ok(())
}
