mod driver;
mod log;
mod mcp;
mod scripts;

use anyhow::Result;

#[tokio::main]
async fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let cmd = args.get(1).map(String::as_str).unwrap_or("snapshot");
    if cmd == "serve" {
        return mcp::serve().await;
    }
    let url = args.get(2).cloned().unwrap_or_else(|| "about:blank".into());
    let (_browser, page, _profile) = driver::launch().await?;
    if url != "about:blank" {
        driver::navigate(&page, &url).await?;
    }
    match cmd {
        "snapshot" => println!("{}", serde_json::to_string_pretty(&driver::eval(&page, driver::SNAPSHOT_JS).await?)?),
        "eval" => {
            let expr = args.get(3).cloned().unwrap_or_else(|| "document.title".into());
            println!("{}", driver::eval(&page, &expr).await?);
        }
        "shot" => {
            let out = args.get(3).cloned().unwrap_or_else(|| "C:/temp/zepi_browser_shot.jpg".into());
            std::fs::write(&out, driver::screenshot(&page).await?)?;
            println!("saved {out}");
        }
        "click" => {
            let x: f64 = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(0.0);
            let y: f64 = args.get(4).and_then(|s| s.parse().ok()).unwrap_or(0.0);
            driver::click(&page, x, y).await?;
            println!("clicked {x},{y}");
        }
        other => anyhow::bail!("unknown command {other}"),
    }
    Ok(())
}