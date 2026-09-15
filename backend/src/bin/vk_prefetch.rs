//! One-shot VK prefetch (same path as `vk::prefetch`).
//!
//! ```text
//! cargo run --bin vk-prefetch
//! cargo run --bin vk-prefetch -- id1117440596
//! cargo run --bin vk-prefetch -- --json id1117440596
//! cargo run --bin vk-prefetch -- --html /tmp/page.html
//! ```

// Shared with okru-backend; each debug bin only uses part of it.
#[path = "../vk.rs"]
#[allow(dead_code)]
mod vk;

use anyhow::{bail, Context, Result};
use std::time::Instant;

const DEFAULT_IDVK: &str = "id1117440596";

#[derive(Debug)]
struct Args {
    idvk: String,
    json: bool,
    googlebot: bool,
    html_path: Option<String>,
}

fn parse_args() -> Result<Option<Args>> {
    let mut json = false;
    let mut googlebot = false;
    let mut html_path = None;
    let mut positional = Vec::new();

    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        match a.as_str() {
            "-h" | "--help" => {
                eprintln!(
                    "Usage: vk-prefetch [options] [idvk]\n\
                     \n\
                     Fetch a VK profile and extract window.cur.apiPrefetchCache.\n\
                     Default idvk: {DEFAULT_IDVK}\n\
                     \n\
                     Options:\n\
                       --json         print the prefetch array as JSON\n\
                       --googlebot    use the old Googlebot UA (expected to miss prefetch)\n\
                       --html PATH    parse a saved HTML file instead of fetching\n\
                       -h, --help     this help"
                );
                return Ok(None);
            }
            "--json" => json = true,
            "--googlebot" => googlebot = true,
            "--html" => {
                html_path = Some(it.next().context("--html needs a path")?);
            }
            s if s.starts_with('-') => bail!("unknown flag: {s}"),
            _ => positional.push(a.clone()),
        }
    }

    Ok(Some(Args {
        idvk: positional
            .into_iter()
            .next()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| DEFAULT_IDVK.to_string()),
        json,
        googlebot,
        html_path,
    }))
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let Some(args) = parse_args()? else {
        return Ok(());
    };

    let (bytes, src, fetch_ms) = if let Some(path) = &args.html_path {
        let t0 = Instant::now();
        let bytes = std::fs::read(path).with_context(|| format!("read {path}"))?;
        (bytes, path.clone(), t0.elapsed().as_secs_f64() * 1000.0)
    } else {
        let http = reqwest::Client::builder().gzip(true).build()?;
        let t0 = Instant::now();
        let (final_url, bytes) =
            vk::fetch_profile_bytes(&http, &args.idvk, args.googlebot).await?;
        (bytes, final_url, t0.elapsed().as_secs_f64() * 1000.0)
    };

    println!("source={src}");
    println!("bytes={}", bytes.len());
    println!("fetch_ms={fetch_ms:.0}");

    let t1 = Instant::now();
    let (prefetch, json_at) = vk::extract_prefetch_value(&bytes)?;
    let parse_ms = t1.elapsed().as_secs_f64() * 1000.0;
    println!("json_byte_offset={json_at}");
    println!("parse_ms={parse_ms:.2}");
    println!();

    if args.json {
        println!("{}", serde_json::to_string_pretty(&prefetch)?);
        return Ok(());
    }

    let catalog = vk::catalog_from_prefetch(&prefetch)?;
    println!("listed={}", catalog.items.len());
    for (i, item) in catalog.items.iter().enumerate() {
        let mark = if item.live_status.eq_ignore_ascii_case("started") {
            " LIVE"
        } else {
            ""
        };
        println!(
            "  [{i}]{mark} id={} title={} duration={} live_status={} date={} thumb={}",
            item.id, item.title, item.duration, item.live_status, item.date, item.thumb
        );
    }
    println!();
    if catalog.found {
        println!("found=true");
        println!("vk_oid={}", catalog.vk_oid);
        println!("vk_id={}", catalog.vk_id);
    } else {
        println!("found=false");
        println!("vk_oid={}", catalog.vk_oid);
        println!("vk_id={}", catalog.vk_id);
        println!("note=no item with live_status=started");
    }
    Ok(())
}
