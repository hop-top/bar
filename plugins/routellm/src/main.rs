//! `bar-routellm`: `--ext-info`, `--check`, `--action`, a render (JSON on
//! stdin), or the `status`, `on`, `off`, `tiers` and `setup` commands.

use bar_plugin::keysource::{self, KeySource};
use bar_plugin::proto::{CheckReport, Output, RenderInput};
use bar_plugin::secret::Secret;
use bar_plugin::url::Origin;
use bar_routellm::collect::{self, Ctx, Status};
use bar_routellm::config::{self, instance_id, Config, Instance};
use bar_routellm::{cache, render, toggle};
use std::io::Read;

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("--ext-info") => println!(
            "{}",
            serde_json::to_string(&render::ext_info()).unwrap_or_default()
        ),
        Some("--check") => println!(
            "{}",
            serde_json::to_string(&check()).unwrap_or_else(|_| r#"{"warnings":[]}"#.into())
        ),
        Some("--menu") => println!("{}", menu().to_json()),
        Some("--action") => std::process::exit(action(args.get(1).map(String::as_str))),
        Some("status" | "on" | "off" | "tiers" | "setup") => std::process::exit(cli(&args)),
        None => println!("{}", run().finish().to_json()),
        Some(other) => {
            eprintln!("bar routellm: unknown argument `{other}`");
            std::process::exit(2);
        }
    }
}

/// The key for `inst`; the reason when there is none.
fn key_for(inst: &Instance) -> Result<Secret, String> {
    let src = inst.key_source()?;
    keysource::read(&src).map_err(|e| format!("{}: {e}", inst.origin))
}

fn run() -> Output {
    let mut raw = String::new();
    let _ = std::io::stdin().take(1 << 20).read_to_string(&mut raw);
    let input: RenderInput = serde_json::from_str(&raw).unwrap_or_default();
    let Ok(cfg) = config::load(&config::path()) else {
        return Output::hidden();
    };
    let url = input
        .env
        .get("ANTHROPIC_BASE_URL")
        .cloned()
        .or_else(|| input.opt_str("base-url").map(String::from))
        .or_else(|| std::env::var("ANTHROPIC_BASE_URL").ok())
        .filter(|u| !u.is_empty());
    let Some(inst) = url
        .as_deref()
        .and_then(Origin::parse)
        .and_then(|o| cfg.instance_for(&o))
    else {
        return Output::hidden();
    };
    // never call with an empty key: the proxy counts that as a failure
    let Ok(key) = key_for(inst) else {
        return Output::styled("RL key?", "bad");
    };
    let state = bar_plugin::dirs::bar_state();
    let t = now();
    let snap = collect::collect(&Ctx {
        inst,
        key: &key,
        state_dir: &state,
        now: t,
        want_tiers: false,
        force: false,
    });
    render::output(
        &snap,
        inst,
        t,
        &render::Opts::from_input(&input),
        &input.session.model_id,
    )
}

/// `--menu`: answered from the cache; the proxy is not called.
fn menu() -> bar_plugin::menu::Menu {
    let Ok(cfg) = config::load(&config::path()) else {
        return Default::default();
    };
    let state = bar_plugin::dirs::bar_state();
    let t = now();
    let snaps: Vec<Option<collect::Snapshot>> = cfg
        .instances
        .iter()
        .map(|inst| {
            let key = key_for(inst).ok()?;
            Some(collect::cached(&Ctx {
                inst,
                key: &key,
                state_dir: &state,
                now: t,
                want_tiers: false,
                force: false,
            }))
        })
        .collect();
    bar_routellm::menu::build(&cfg.instances, &snaps, t)
}

fn check() -> CheckReport {
    let mut w = Vec::new();
    match config::load(&config::path()) {
        Err(config::LoadError::Missing) => w.push("no routellm.conf: nothing is configured".into()),
        Err(e) => w.push(e.to_string()),
        Ok(cfg) => {
            w.extend(cfg.errors.iter().map(|e| format!("routellm.conf: {e}")));
            if cfg.instances.is_empty() && cfg.errors.is_empty() {
                w.push("routellm.conf has no instance".into());
            }
            let state = bar_plugin::dirs::bar_state();
            for i in &cfg.instances {
                check_instance(i, &state, &mut w);
            }
        }
    }
    CheckReport { warnings: w }
}

fn check_instance(i: &Instance, state: &std::path::Path, w: &mut Vec<String>) {
    // key problems and remembered failures; no network, and a keychain or
    // command source is not run
    let mut key: Option<Secret> = None;
    match i.key_source() {
        Err(e) => w.push(format!("{}: {e}", i.origin)),
        Ok(KeySource::File(p)) => match keysource::read(&KeySource::File(p)) {
            Ok(k) => key = Some(k),
            Err(e) => w.push(format!("{}: {e}", i.origin)),
        },
        Ok(_) => {}
    }
    let guard = bar_plugin::keyguard::Guard {
        state_dir: state,
        mgmt: &i.management,
    };
    let path = cache::file(state, &i.origin.to_string());
    if !cache::writable(&path) || !guard.writable() {
        w.push(format!(
            "{}: the state directory is not writable; the proxy is not called without a place to remember a refused key",
            i.origin
        ));
    }
    let n = now();
    if let Some(st) = key.as_ref().and_then(|k| guard.refusal(k, n)) {
        w.push(format!(
            "{}: management refused the key ({st}); no calls until the key changes",
            i.origin
        ));
    }
    let c = cache::load(&path);
    if n < c.off_until {
        w.push(format!(
            "{}: management API is off (404); enable it in the proxy's config",
            i.origin
        ));
    }
    match (c.problem, &c.entry) {
        (Some(cache::Problem::Unconfigured), _) => w.push(format!(
            "{}: the proxy has no `{}` provider entry; run `bar rtellm setup` and add it",
            i.origin, i.provider
        )),
        (Some(cache::Problem::Mismatch), _) => w.push(format!(
            "{}: the `{}` entry does not point at {}; toggles are refused",
            i.origin, i.provider, i.routellm
        )),
        (None, Some(_)) if !collect::armed(&c) => w.push(format!(
            "{}: some aliases have no claude row, so OFF fails for them; run `bar rtellm setup`",
            i.origin
        )),
        (None, None) if c.at == 0 => w.push(format!(
            "{}: not read yet; `bar rtellm status --probe` reads the proxy once",
            i.origin
        )),
        _ => {}
    }
    for a in collect::private_with_row(&c) {
        w.push(format!(
            "{}: `{a}` is a private tier but has a claude row, so OFF sends it to the frontier",
            i.origin
        ));
    }
    if i.on_priority <= 0 {
        w.push(format!(
            "{}: on-priority {} does not outrank a frontier account",
            i.origin, i.on_priority
        ));
    }
}

fn usage(m: &str) -> i32 {
    eprintln!("bar routellm: {m}");
    eprintln!("usage: bar routellm status [--instance URL] [--probe] [--json]");
    eprintln!("       bar routellm on [--instance URL] [--dry-run] [--force]");
    eprintln!("       bar routellm off [--instance URL] [--dry-run]");
    eprintln!("       bar routellm tiers [--all] [--instance URL]");
    eprintln!("       bar routellm setup [--layout v8|legacy] [--instance URL]");
    2
}

fn pick<'a>(cfg: &'a Config, explicit: Option<&str>) -> Result<&'a Instance, String> {
    let env = std::env::var("ANTHROPIC_BASE_URL").ok();
    if let Some(u) = explicit.or(env.as_deref()).filter(|u| !u.is_empty()) {
        return Origin::parse(u)
            .and_then(|o| cfg.instance_for(&o))
            .ok_or_else(|| "no configured instance for that URL".to_string());
    }
    match cfg.instances.as_slice() {
        [] => Err("no instance configured".into()),
        [one] => Ok(one),
        _ => Err("several instances are configured; name one with --instance".into()),
    }
}

fn cli(args: &[String]) -> i32 {
    let cmd = args[0].as_str();
    let (mut instance, mut probe, mut json, mut dry, mut force, mut all) =
        (None, false, false, false, false, false);
    let mut layout = None;
    let mut it = args.iter().skip(1);
    while let Some(a) = it.next() {
        match a.as_str() {
            "--probe" => probe = true,
            "--json" => json = true,
            "--dry-run" => dry = true,
            "--force" => force = true,
            "--all" => all = true,
            "--layout" => match it.next().map(|v| render::Layout::parse(v)) {
                Some(Some(l)) => layout = Some(l),
                _ => return usage("--layout takes v8 or legacy"),
            },
            "--instance" => match it.next() {
                Some(v) => instance = Some(v.clone()),
                None => return usage("--instance needs a value"),
            },
            s => return usage(&format!("unknown option `{s}`")),
        }
    }
    if layout.is_some() && cmd != "setup" {
        return usage(&format!("{cmd} does not take that"));
    }
    let ok = match cmd {
        "status" => !(dry || force || all),
        "on" => !(probe || json || all),
        "off" => !(probe || json || all || force),
        "tiers" => !(probe || json || dry || force),
        _ => !(probe || json || dry || force || all),
    };
    if !ok {
        return usage(&format!("{cmd} does not take that"));
    }
    let cfg = match config::load(&config::path()) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("bar routellm: {e}");
            return 1;
        }
    };
    let inst = match pick(&cfg, instance.as_deref()) {
        Ok(i) => i,
        Err(e) => {
            eprintln!("bar routellm: {e}");
            return 1;
        }
    };
    let state = bar_plugin::dirs::bar_state();
    let path = cache::file(&state, &inst.origin.to_string());
    let t = now();
    if cmd == "setup" {
        render::setup_snippet(
            inst,
            &cache::load(&path),
            layout.unwrap_or(render::Layout::V8),
        )
        .iter()
        .for_each(|l| println!("{l}"));
        return 0;
    }
    let key = match key_for(inst) {
        Ok(k) => k,
        Err(e) => {
            eprintln!("bar routellm: {e}");
            return 1;
        }
    };
    match cmd {
        "on" | "off" => {
            let r = toggle::run(&toggle::Req {
                inst,
                key: &key,
                state_dir: &state,
                now: t,
                to_on: cmd == "on",
                dry_run: dry,
                force,
            });
            finish(r.map_err(|e| e.0))
        }
        _ => {
            let ctx = Ctx {
                inst,
                key: &key,
                state_dir: &state,
                // a listing command or an explicit probe makes the calls;
                // plain `status` answers from the cache
                now: t,
                want_tiers: cmd == "tiers" || probe,
                force: cmd == "tiers" || probe,
            };
            let snap = if cmd == "tiers" || probe {
                collect::collect(&ctx)
            } else {
                collect::cached(&ctx)
            };
            if let Status::AuthStopped(_) = snap.status {
                if cmd == "tiers" {
                    eprintln!(
                        "bar routellm: management refused the key; no calls until it changes"
                    );
                    return 1;
                }
            }
            let lines = if cmd == "tiers" {
                render::tier_lines(&snap, inst, all)
            } else if json {
                vec![serde_json::to_string(&snap.cache).unwrap_or_default()]
            } else {
                render::status_lines(&snap, inst, t)
            };
            lines.iter().for_each(|l| println!("{l}"));
            0
        }
    }
}

fn finish(r: Result<Vec<String>, String>) -> i32 {
    match r {
        Ok(lines) => {
            lines.iter().for_each(|l| println!("{l}"));
            0
        }
        Err(e) => {
            eprintln!("bar routellm: {e}");
            1
        }
    }
}

/// `--action <toggle|on|off>`: `{"action","tokens"}` on stdin, from a click.
fn action(name: Option<&str>) -> i32 {
    let mut raw = String::new();
    let _ = std::io::stdin().take(1 << 16).read_to_string(&mut raw);
    let v: serde_json::Value = serde_json::from_str(&raw).unwrap_or_default();
    let to_on = match (name, v["tokens"]["to"].as_str()) {
        (Some("on"), _) | (Some("toggle"), Some("on")) => true,
        (Some("off"), _) | (Some("toggle"), Some("off")) => false,
        (Some("toggle"), _) => {
            eprintln!("bar routellm: toggle needs the `to` token");
            return 2;
        }
        _ => {
            eprintln!("bar routellm: unknown action");
            return 2;
        }
    };
    let Some(id) = v["tokens"]["instance"].as_str() else {
        eprintln!("bar routellm: needs the instance token");
        return 2;
    };
    finish((|| {
        let cfg = config::load(&config::path()).map_err(|e| e.to_string())?;
        let inst = cfg
            .instances
            .iter()
            .find(|i| instance_id(&i.origin) == id)
            .ok_or("no such instance")?;
        let key = key_for(inst)?;
        toggle::run(&toggle::Req {
            inst,
            key: &key,
            state_dir: &bar_plugin::dirs::bar_state(),
            now: now(),
            to_on,
            dry_run: false,
            force: false,
        })
        .map_err(|e| e.0)
    })())
}
