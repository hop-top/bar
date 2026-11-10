//! `bar-cliproxyapi`: `--ext-info`, `--check`, or a render (JSON on stdin).

use bar_cliproxyapi::{collect, config, render};
use bar_plugin::keysource;
use bar_plugin::proto::{CheckReport, Output, RenderInput};
use bar_plugin::url::Origin;
use std::io::Read;

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

fn emit(o: Output) {
    println!("{}", o.finish().to_json());
}

fn main() {
    match std::env::args().nth(1).as_deref() {
        Some("--ext-info") => {
            println!(
                "{}",
                serde_json::to_string(&render::ext_info()).unwrap_or_default()
            )
        }
        Some("--check") => println!(
            "{}",
            serde_json::to_string(&check()).unwrap_or_else(|_| r#"{"warnings":[]}"#.into())
        ),
        None => emit(run()),
        Some(other) => {
            eprintln!("bar-cliproxyapi: unknown argument `{other}`");
            std::process::exit(2);
        }
    }
}

/// The base URL the session talks to: the render input's environment
/// (`ANTHROPIC_BASE_URL` is declared as an input), else the `base-url`
/// option, else the inherited environment.
fn base_url(input: &RenderInput) -> Option<String> {
    input
        .env
        .get("ANTHROPIC_BASE_URL")
        .cloned()
        .or_else(|| {
            input
                .opt_str("base-url")
                .filter(|s| !s.is_empty())
                .map(String::from)
        })
        .or_else(|| std::env::var("ANTHROPIC_BASE_URL").ok())
}

fn run() -> Output {
    let mut raw = String::new();
    let _ = std::io::stdin().take(1 << 20).read_to_string(&mut raw);
    let input: RenderInput = serde_json::from_str(&raw).unwrap_or_default();
    let Ok(cfg) = config::load(&config::path()) else {
        return Output::hidden();
    };
    let Some(inst) = base_url(&input)
        .and_then(|u| Origin::parse(&u))
        .and_then(|o| cfg.instance_for(&o).cloned())
    else {
        return Output::hidden();
    };
    let provider = match input.opt_str("provider") {
        Some(p @ ("claude" | "codex")) => p,
        _ => match bar_cliproxyapi::model::provider_of_model(
            &input.session.model_id,
            &input.session.model,
        ) {
            Some(p) if inst.providers.iter().any(|x| x == p) => p,
            _ => return Output::hidden(),
        },
    };
    // never call with an empty key: the proxy counts that as a failure
    let Ok(key) = keysource::read(&inst.key) else {
        return Output::styled("key?", "bad");
    };
    let state = bar_plugin::dirs::bar_state();
    let now = now();
    let snap = collect::collect(&collect::Ctx {
        inst: &inst,
        key: &key,
        state_dir: &state,
        now,
        fetch: &[provider],
    });
    render::output(
        &snap,
        provider,
        &render::Opts::from_input(&input),
        &inst.origin.to_string(),
        now,
    )
}

fn check() -> CheckReport {
    let mut w = Vec::new();
    let path = config::path();
    match config::load(&path) {
        Err(config::LoadError::Missing) => {
            w.push("no cliproxyapi.conf: nothing is configured".into())
        }
        Err(e) => w.push(e.to_string()),
        Ok(cfg) => {
            w.extend(cfg.errors.iter().map(|e| format!("cliproxyapi.conf: {e}")));
            if cfg.instances.is_empty() && cfg.errors.is_empty() {
                w.push("cliproxyapi.conf has no instance".into());
            }
            let state = bar_plugin::dirs::bar_state();
            for i in &cfg.instances {
                // key problems and remembered failures; no network
                let key = keysource::read(&i.key);
                if let Err(e) = &key {
                    w.push(format!("{}: {e}", i.origin));
                }
                let c = bar_cliproxyapi::cache::load(&bar_cliproxyapi::cache::file(
                    &state,
                    &i.origin.to_string(),
                ));
                let n = now();
                if let Some(f) = &c.auth_fail {
                    let same = key
                        .as_ref()
                        .is_ok_and(|k| keysource::fingerprint(k) == f.fp);
                    if same && n - f.at < collect::AUTH_HOLD {
                        w.push(format!(
                            "{}: management refused the key ({}); no calls until the key changes",
                            i.origin, f.status
                        ));
                    }
                }
                if n < c.off_until {
                    w.push(format!(
                        "{}: management API is off (404); enable it in the proxy's config",
                        i.origin
                    ));
                }
            }
        }
    }
    CheckReport { warnings: w }
}
