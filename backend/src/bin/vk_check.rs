//! One-shot VK scrape: same path as `vk::check_and_ids`.
//! No bot, OAuth, scheduler, or worker POST.
//!
//! ```text
//! cargo run --bin vk-check -- id1117440596
//! cargo run --bin vk-check -- https://vk.com/id1117440596
//! ```

#[path = "../vk.rs"]
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
             Runs vk::check_and_ids against a VK profile/community.\n\
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

    tracing::info!("vk::check_and_ids idvk={idvk}");
    let (found, oid, vid) = vk::check_and_ids(&http, &idvk).await?;

    println!("found={found}");
    println!("vk_oid={oid}");
    println!("vk_id={vid}");
    Ok(())
}
