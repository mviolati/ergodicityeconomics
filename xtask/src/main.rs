//! Project tasks: `cargo xtask <png|web|bench> ...`.

use coin_chart::{render, theme, Frame, Scene};
use coin_core::{
    ensemble::ensemble,
    sim::{path, simulate_parallel},
    stats::{picks, rich_ids, stats},
    Game,
};
use std::{
    path::{Path, PathBuf},
    process::Command,
    time::Instant,
};

fn arg<T: std::str::FromStr>(args: &[String], name: &str, default: T) -> T {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1))
        .map(|v| v.parse().unwrap_or_else(|_| panic!("bad value for {name}")))
        .unwrap_or(default)
}

fn threads() -> usize {
    std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4)
}

fn log_path(game: &Game, id: u64) -> Vec<f64> {
    let lat = game.lattice();
    path(game, id).iter().enumerate().map(|(t, &k)| lat.at(t as u32, k)).collect()
}

fn png(args: &[String]) {
    let players: usize = arg(args, "--players", 10_000);
    let game = Game::peters(arg(args, "--rounds", 1000), arg(args, "--seed", 2022));
    game.validate().expect("valid game");
    let dark = arg(args, "--theme", String::from("light")) == "dark";
    let out: String = arg(args, "--out", String::from("chart.png"));
    let t0 = Instant::now();
    let (counts, s) = simulate_parallel(&game, players, threads());
    let e = ensemble(&game, players as u64, &counts);
    let st = stats(&game, &s, &e);
    let hl: Vec<(Vec<f64>, usize)> = picks(&game, &s).iter().map(|p| (log_path(&game, p.id), p.role.slot())).collect();
    let rich: Vec<Vec<f64>> = rich_ids(&game, &s).iter().map(|&id| log_path(&game, id)).collect();
    let scene =
        Scene { game: &game, counts: &counts, ensemble: &e, highlighted: &hl, rich_paths: &rich, rich_full: false };
    let frame = Frame {
        css_w: arg(args, "--width", 1000.0),
        css_h: arg(args, "--height", 560.0),
        dpr: arg(args, "--dpr", 2.0),
    };
    let (pm, _) = render(&scene, frame, if dark { &theme::DARK } else { &theme::LIGHT });
    pm.save_png(&out).expect("write png");
    eprintln!(
        "{out}: {} players, {} reached the rich threshold ({} at most at once), {:.2}s",
        players,
        st.rich_ever,
        st.rich_most_at_once,
        t0.elapsed().as_secs_f64()
    );
}

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().expect("workspace root").to_path_buf()
}

fn base64(bytes: &[u8]) -> String {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for c in bytes.chunks(3) {
        let n =
            (u32::from(c[0]) << 16) | (u32::from(*c.get(1).unwrap_or(&0)) << 8) | u32::from(*c.get(2).unwrap_or(&0));
        for i in 0..4 {
            if i <= c.len() {
                out.push(A[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

/// `cargo metadata` for the WebAssembly target, as JSON.
fn metadata(root: &Path) -> serde_json::Value {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let out = Command::new(cargo)
        .current_dir(root)
        .args(["metadata", "--format-version", "1", "--filter-platform", "wasm32-unknown-unknown"])
        .output()
        .expect("run cargo metadata");
    assert!(out.status.success(), "cargo metadata failed");
    serde_json::from_slice(&out.stdout).expect("cargo metadata output is JSON")
}

/// License notices of every third-party crate compiled into the WebAssembly module, and of the
/// embedded font, as an HTML <details> block.
fn notices(root: &Path, meta: &serde_json::Value) -> String {
    let packages = meta["packages"].as_array().expect("packages");
    // `cargo tree -p coin-web` resolves features for the WebAssembly build alone (the workspace
    // metadata would also count what only xtask enables, such as PNG output).
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let tree = Command::new(cargo)
        .current_dir(root)
        .args([
            "tree",
            "-p",
            "coin-web",
            "--target",
            "wasm32-unknown-unknown",
            "-e",
            "normal",
            "--prefix",
            "none",
            "-f",
            "{p}",
        ])
        .output()
        .expect("run cargo tree");
    assert!(tree.status.success(), "cargo tree failed");
    let used: std::collections::BTreeSet<(String, String)> = String::from_utf8_lossy(&tree.stdout)
        .lines()
        .filter(|l| !l.contains(" (/") && !l.contains(" (*)") && !l.trim().is_empty())
        .filter_map(|l| {
            let mut it = l.split_whitespace();
            Some((it.next()?.to_string(), it.next()?.trim_start_matches('v').to_string()))
        })
        .collect();
    let members: Vec<&str> =
        meta["workspace_members"].as_array().expect("members").iter().filter_map(|v| v.as_str()).collect();
    let mut body = String::new();
    let mut texts: Vec<(String, Vec<String>)> = Vec::new(); // license text -> crates
    for p in packages.iter().filter(|p| {
        let key = (p["name"].as_str().unwrap_or("").to_string(), p["version"].as_str().unwrap_or("").to_string());
        used.contains(&key) && !members.contains(&p["id"].as_str().unwrap_or(""))
    }) {
        let name = format!("{} {}", p["name"].as_str().unwrap_or("?"), p["version"].as_str().unwrap_or("?"));
        body.push_str(&format!(
            "<li>{} — {}</li>",
            html_escape(&name),
            html_escape(p["license"].as_str().unwrap_or("vedi testo"))
        ));
        let dir =
            Path::new(p["manifest_path"].as_str().expect("manifest path")).parent().expect("crate dir").to_path_buf();
        let mut files: Vec<PathBuf> = std::fs::read_dir(&dir)
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.path())
            .filter(|f| {
                let n = f.file_name().and_then(|n| n.to_str()).unwrap_or("").to_ascii_uppercase();
                f.is_file()
                    && (n.starts_with("LICENSE")
                        || n.starts_with("LICENCE")
                        || n.starts_with("COPYING")
                        || n.starts_with("NOTICE"))
            })
            .collect();
        files.sort();
        for f in files {
            let t = std::fs::read_to_string(&f).unwrap_or_default().trim().to_string();
            match texts.iter_mut().find(|(x, _)| *x == t) {
                Some((_, who)) => who.push(name.clone()),
                None => texts.push((t, vec![name.clone()])),
            }
        }
    }
    let font =
        std::fs::read_to_string(root.join("crates/coin-chart/assets/IBMPlexSans-LICENSE.txt")).expect("font license");
    texts.push((font.trim().to_string(), vec!["IBM Plex Sans (font del grafico)".into()]));
    let mut out = format!(
        "<details class=\"notices\"><summary>Licenze del software incluso nella pagina</summary><ul>{body}<li>IBM Plex Sans — OFL-1.1</li></ul>"
    );
    for (t, who) in texts {
        out.push_str(&format!("<p><b>{}</b></p><pre>{}</pre>", html_escape(&who.join(", ")), html_escape(&t)));
    }
    out.push_str("</details>");
    out
}

/// Builds dist/index.html (one file with the page and its WebAssembly module) and
/// dist/fragment.html (the same page without the document skeleton, for hosts that add their own).
fn web() {
    let root = root();
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let status = Command::new(cargo)
        .current_dir(&root)
        .args(["build", "--release", "-p", "coin-web", "--target", "wasm32-unknown-unknown"])
        .status()
        .expect("run cargo");
    assert!(status.success(), "building the WebAssembly module failed (rustup target add wasm32-unknown-unknown)");
    let meta = metadata(&root);
    // The target directory as cargo resolves it (CARGO_TARGET_DIR, build.target-dir, or target/).
    let target = PathBuf::from(meta["target_directory"].as_str().expect("target_directory"));
    let wasm = std::fs::read(target.join("wasm32-unknown-unknown/release/coin_web.wasm")).expect("read wasm");
    let template = std::fs::read_to_string(root.join("web/index.html")).expect("read web/index.html");
    for key in ["/*@TOKENS@*/", "@LEDE@", "@WASM@", "<!--@NOTICES@-->"] {
        assert_eq!(template.matches(key).count(), 1, "template must contain {key} exactly once");
    }
    let fragment = template
        .replace("/*@TOKENS@*/", &coin_chart::theme::css_tokens())
        .replace("@LEDE@", &html_escape(&coin_web::default_lede()))
        .replace("<!--@NOTICES@-->", &notices(&root, &meta))
        .replace("@WASM@", &base64(&wasm));
    let page = format!(
        "<!doctype html>\n<html lang=\"it\">\n<head>\n<meta charset=\"utf-8\">\n<meta name=\"viewport\" content=\"width=device-width, initial-scale=1, viewport-fit=cover\">\n<style>body{{margin:0}}[hidden]{{display:none!important}}</style>\n</head>\n<body>\n{fragment}\n</body>\n</html>\n"
    );
    let dist = root.join("dist");
    std::fs::create_dir_all(&dist).expect("create dist");
    std::fs::write(dist.join("index.html"), &page).expect("write index.html");
    std::fs::write(dist.join("fragment.html"), &fragment).expect("write fragment.html");
    eprintln!("dist/index.html: {} kB (WebAssembly {} kB)", page.len() / 1024, wasm.len() / 1024);
}

/// Native simulation speed, all cores.
fn bench() {
    for players in [10_000usize, 100_000, 1_000_000] {
        let game = Game::peters(1000, 2022);
        let t0 = Instant::now();
        let (_, s) = simulate_parallel(&game, players, threads());
        let sec = t0.elapsed().as_secs_f64();
        eprintln!(
            "{players:>9} players x 1000 rounds: {:.3}s ({:.2} ns per toss, {} threads), {} players",
            sec,
            1e9 * sec / (players as f64 * 1000.0),
            threads(),
            s.len()
        );
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("png") => png(&args[1..]),
        Some("web") => web(),
        Some("bench") => bench(),
        _ => {
            eprintln!("usage: cargo xtask web | bench | png [--players N] [--rounds R] [--seed S] [--theme light|dark] [--width W] [--height H] [--dpr D] [--out file.png]");
            std::process::exit(2);
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn base64_matches_rfc4648_vectors() {
        let cases = [
            ("", ""),
            ("f", "Zg=="),
            ("fo", "Zm8="),
            ("foo", "Zm9v"),
            ("foob", "Zm9vYg=="),
            ("fooba", "Zm9vYmE="),
            ("foobar", "Zm9vYmFy"),
        ];
        for (i, o) in cases {
            assert_eq!(super::base64(i.as_bytes()), o);
        }
    }
}
