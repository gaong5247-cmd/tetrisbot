use anyhow::{Result, bail};
use std::path::PathBuf;
fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|s| s == "--help") {
        println!(
            "SilhouetteTetris {}\n  --gui (default)\n  --test-game\n  --headless [--seconds N] [--allow-input --fresh-game]\n  --config PATH\nF10: emergency stop. GUI never arms input on launch.",
            env!("CARGO_PKG_VERSION")
        );
        return Ok(());
    }
    let mut path = PathBuf::from("config/default.toml");
    let mut seconds = 15;
    for (i, a) in args.iter().enumerate() {
        match a.as_str() {
            "--config" => {
                path = PathBuf::from(
                    args.get(i + 1)
                        .ok_or_else(|| anyhow::anyhow!("missing config path"))?,
                )
            }
            "--seconds" => {
                seconds = args
                    .get(i + 1)
                    .ok_or_else(|| anyhow::anyhow!("missing seconds"))?
                    .parse()?
            }
            "--gui" | "--headless" | "--test-game" | "--allow-input" | "--fresh-game" => {}
            s if i > 0 && (args[i - 1] == "--config" || args[i - 1] == "--seconds") => {
                let _ = s;
            }
            other => bail!("unknown option {other}"),
        }
    }
    if args.contains(&"--test-game".to_string()) {
        return gui::lab::run();
    }
    let cfg = gui::Config::load(&path)?;
    if args.contains(&"--headless".to_string()) {
        gui::headless(
            cfg,
            seconds,
            args.contains(&"--allow-input".to_string()),
            args.contains(&"--fresh-game".to_string()),
        )
    } else {
        gui::run(cfg)
    }
}
