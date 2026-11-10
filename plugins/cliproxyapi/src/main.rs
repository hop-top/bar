//! `bar-cliproxyapi`: `--ext-info`, `--check`, or a render (JSON on stdin).

use bar_cliproxyapi::{collect, config, render, switch};
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
        Some("--action") => std::process::exit(action(std::env::args().nth(2).as_deref())),
        Some("list") => std::process::exit(cli(&std::env::args().skip(1).collect::<Vec<_>>())),
        Some("use") => std::process::exit(cli(&std::env::args().skip(1).collect::<Vec<_>>())),
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
                let cpath = bar_cliproxyapi::cache::file(&state, &i.origin.to_string());
                let c = bar_cliproxyapi::cache::load(&cpath);
                if !bar_cliproxyapi::cache::writable(&cpath) {
                    w.push(format!(
                        "{}: the state directory is not writable; the proxy is not called without a place to remember a refused key",
                        i.origin
                    ));
                }
                let n = now();
                let held = key
                    .as_ref()
                    .ok()
                    .and_then(|k| c.refusal(k, collect::AUTH_HOLD, n));
                if let Some(st) = held {
                    w.push(format!(
                        "{}: management refused the key ({st}); no calls until the key changes",
                        i.origin
                    ));
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

/// Prints the lines, or the refusal on stderr; the exit code.
fn finish(r: Result<Vec<String>, switch::Refusal>) -> i32 {
    match r {
        Ok(lines) => {
            lines.iter().for_each(|l| println!("{l}"));
            0
        }
        Err(e) => {
            eprintln!("bar-cliproxyapi: {e}");
            1
        }
    }
}

fn load_config() -> Result<config::Config, switch::Refusal> {
    config::load(&config::path()).map_err(|e| switch::Refusal(e.to_string()))
}

/// `list` and `use`: exit 0 done, 1 refused or failed, 2 bad usage.
fn cli(args: &[String]) -> i32 {
    let usage = |m: &str| {
        eprintln!("bar-cliproxyapi: {m}");
        eprintln!("usage: bar-cliproxyapi list [--provider P] [--instance URL]");
        eprintln!(
            "       bar-cliproxyapi use <account> [--provider P] [--instance URL] [--dry-run]"
        );
        eprintln!("       bar-cliproxyapi use --reset [<account>] [--instance URL] [--dry-run]");
        2
    };
    let (mut provider, mut instance, mut reset, mut dry, mut account) =
        (None, None, false, false, None);
    let mut it = args.iter().skip(1);
    while let Some(a) = it.next() {
        match a.as_str() {
            "--reset" => reset = true,
            "--dry-run" => dry = true,
            "--provider" | "--instance" => {
                let Some(v) = it.next() else {
                    return usage(&format!("{a} needs a value"));
                };
                if a == "--provider" {
                    provider = Some(v.clone());
                } else {
                    instance = Some(v.clone());
                }
            }
            s if s.starts_with("--") => return usage(&format!("unknown option `{s}`")),
            s if account.is_none() => account = Some(s.to_string()),
            _ => return usage("one account at a time"),
        }
    }
    let is_use = args[0] == "use";
    if !is_use && (reset || dry || account.is_some()) {
        return usage("list takes only --provider and --instance");
    }
    if is_use && account.is_none() && !reset {
        return usage("name an account");
    }
    if let Some(p) = &provider {
        if !config::PROVIDERS.contains(&p.as_str()) {
            return usage("provider is claude or codex");
        }
    }
    finish(session(instance.as_deref(), None, |live| {
        if is_use {
            switch::run_use(live, account.as_deref(), provider.as_deref(), reset, dry)
        } else {
            Ok(switch::run_list(live, provider.as_deref()))
        }
    }))
}

/// Connects to the picked instance and runs `f`.
fn session(
    instance: Option<&str>,
    known: Option<(&str, &str)>,
    f: impl FnOnce(&switch::Live) -> Result<Vec<String>, switch::Refusal>,
) -> Result<Vec<String>, switch::Refusal> {
    let cfg = load_config()?;
    let state = bar_plugin::dirs::bar_state();
    let env = std::env::var("ANTHROPIC_BASE_URL").ok();
    let inst = switch::pick_instance(&cfg, instance, env.as_deref(), &state, known)?;
    let key = switch::key_for(inst)?;
    let live = switch::connect(inst, &key, &state, now())?;
    f(&live)
}

/// `--action use`: `{"action","tokens"}` on stdin, from a click. A token
/// must name an account of the instance's current listing.
fn action(name: Option<&str>) -> i32 {
    let mut raw = String::new();
    let _ = std::io::stdin().take(1 << 16).read_to_string(&mut raw);
    if name != Some("use") {
        eprintln!("bar-cliproxyapi: unknown action");
        return 2;
    }
    finish(switch::action_tokens(&raw).and_then(|(account, provider)| {
        session(None, Some((&account, &provider)), |live| {
            // an `auth_index` only: the link carries nothing else
            let target = live
                .entries
                .iter()
                .find(|e| e.index == account && e.provider == provider)
                .ok_or_else(|| switch::Refusal("no such account".into()))?;
            switch::run_use(live, Some(&target.index), Some(&provider), false, false)
        })
    }))
}
