//! `bar-cliproxyapi`: `--ext-info`, `--check`, or a render (JSON on stdin).

use bar_cliproxyapi::{collect, config, rank, render, switch};
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
        Some("--menu") => menu(),
        Some("--action") => std::process::exit(action(std::env::args().nth(2).as_deref())),
        Some("list") => std::process::exit(cli(&std::env::args().skip(1).collect::<Vec<_>>())),
        Some("use") | Some("pick") => {
            std::process::exit(cli(&std::env::args().skip(1).collect::<Vec<_>>()))
        }
        None => emit(run()),
        Some(other) => {
            eprintln!("bar cliproxyapi: unknown argument `{other}`");
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
    let Ok(mut cfg) = config::load(&config::path()) else {
        return Output::hidden();
    };
    cfg.set_rules(&input.accounts);
    let url = base_url(&input).filter(|u| !u.is_empty());
    let Some(inst) = url
        .as_deref()
        .and_then(Origin::parse)
        .and_then(|o| cfg.instance_for(&o).cloned())
    else {
        // a session without a proxy, or with one that is not configured:
        // nothing to show and no tokens, so `#{account}` keeps the
        // signed-in account unlinked (switching the proxy would not change
        // this session's account)
        return Output::hidden();
    };
    let provider = match input.opt_str("provider") {
        Some(p @ ("claude" | "codex")) => p,
        _ => match bar_cliproxyapi::model::provider_of_model(
            &input.session.model_id,
            &input.session.model,
            input.opt_str("route-provider").unwrap_or("claude"),
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

/// `--menu`: the groups for every configured instance, from the caches.
fn menu() {
    let mut raw = String::new();
    let _ = std::io::stdin().take(1 << 20).read_to_string(&mut raw);
    let input: bar_plugin::menu::MenuInput = serde_json::from_str(&raw).unwrap_or_default();
    let m = match config::load(&config::path()) {
        Ok(mut cfg) => {
            cfg.set_rules(&input.accounts);
            bar_cliproxyapi::menu::build(&cfg, &bar_plugin::dirs::bar_state(), now())
        }
        Err(_) => bar_plugin::menu::Menu::default(),
    };
    println!("{}", m.to_json());
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
                let guard = bar_plugin::keyguard::Guard {
                    state_dir: &state,
                    mgmt: &i.management,
                };
                if !bar_cliproxyapi::cache::writable(&cpath) || !guard.writable() {
                    w.push(format!(
                        "{}: the state directory is not writable; the proxy is not called without a place to remember a refused key",
                        i.origin
                    ));
                }
                let n = now();
                let held = key.as_ref().ok().and_then(|k| {
                    c.refusal(k, collect::AUTH_HOLD, n)
                        .or_else(|| guard.refusal(k, n))
                });
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
            eprintln!("bar cliproxyapi: {e}");
            1
        }
    }
}

/// The config, with the user's `account` lines (`rules`) on its instances.
fn load_config(
    rules: &[bar_plugin::proto::AccountRule],
) -> Result<config::Config, switch::Refusal> {
    let mut cfg = config::load(&config::path()).map_err(|e| switch::Refusal(e.to_string()))?;
    cfg.set_rules(rules);
    Ok(cfg)
}

/// `list`, `use` and `pick`: exit 0 done, 1 refused or failed, 2 bad usage.
fn cli(args: &[String]) -> i32 {
    let usage = |m: &str| {
        eprintln!("bar cliproxyapi: {m}");
        eprintln!("usage: bar cliproxyapi list [--provider P] [--instance URL]");
        eprintln!(
            "       bar cliproxyapi use <account> [--provider P] [--instance URL] [--dry-run]"
        );
        eprintln!("       bar cliproxyapi use --next [--strategy most-left|soonest-reset] [--provider P] [--instance URL] [--dry-run]");
        eprintln!("       bar cliproxyapi use --reset [<account>] [--instance URL] [--dry-run]");
        eprintln!("       bar cliproxyapi pick [--strategy most-left|soonest-reset] [--provider P] [--instance URL]");
        2
    };
    let (mut provider, mut instance, mut reset, mut dry, mut account) =
        (None, None, false, false, None);
    let (mut next, mut strategy) = (false, None);
    let mut it = args.iter().skip(1);
    while let Some(a) = it.next() {
        match a.as_str() {
            "--reset" => reset = true,
            "--dry-run" => dry = true,
            "--next" => next = true,
            "--provider" | "--instance" | "--strategy" => {
                let Some(v) = it.next() else {
                    return usage(&format!("{a} needs a value"));
                };
                match a.as_str() {
                    "--provider" => provider = Some(v.clone()),
                    "--instance" => instance = Some(v.clone()),
                    _ => strategy = Some(v.clone()),
                }
            }
            s if s.starts_with("--") => return usage(&format!("unknown option `{s}`")),
            s if account.is_none() => account = Some(s.to_string()),
            _ => return usage("one account at a time"),
        }
    }
    let cmd = args[0].as_str();
    let is_use = cmd == "use";
    let is_pick = cmd == "pick";
    if !is_use && (reset || dry || account.is_some() || next) {
        return usage(&format!("{cmd} does not take that"));
    }
    if !is_use && !is_pick && strategy.is_some() {
        return usage("list does not take --strategy");
    }
    if is_use && next && (reset || account.is_some()) {
        return usage("--next takes no account and no --reset");
    }
    if is_use && account.is_none() && !reset && !next {
        return usage("name an account");
    }
    if strategy.is_some() && !next && !is_pick {
        return usage("--strategy goes with --next or pick");
    }
    if is_pick && account.is_some() {
        return usage("pick takes no account");
    }
    let strat = match strategy
        .as_deref()
        .map_or(Some(rank::Strategy::MostLeft), rank::Strategy::parse)
    {
        Some(s) => s,
        None => return usage("strategy is most-left or soonest-reset"),
    };
    if let Some(p) = &provider {
        if !config::PROVIDERS.contains(&p.as_str()) {
            return usage("provider is claude or codex");
        }
    }
    let rules = bar_plugin::proto::accounts_from_env();
    finish(session(&rules, instance.as_deref(), None, |live| {
        if is_pick {
            use std::io::IsTerminal;
            let tty = std::io::stdin().is_terminal() && std::io::stdout().is_terminal();
            let stdin = std::io::stdin();
            let mut lock = stdin.lock();
            let input: Option<&mut dyn std::io::BufRead> = tty.then_some(&mut lock);
            switch::run_pick(
                live,
                provider.as_deref(),
                strat,
                input,
                &mut std::io::stdout(),
            )?;
            Ok(vec![])
        } else if next {
            switch::run_next(live, provider.as_deref(), strat, dry)
        } else if is_use {
            switch::run_use(live, account.as_deref(), provider.as_deref(), reset, dry)
        } else {
            Ok(switch::run_list(live, provider.as_deref()))
        }
    }))
}

/// Connects to the picked instance and runs `f`.
fn session(
    rules: &[bar_plugin::proto::AccountRule],
    instance: Option<&str>,
    known: Option<(&str, &str)>,
    f: impl FnOnce(&switch::Live) -> Result<Vec<String>, switch::Refusal>,
) -> Result<Vec<String>, switch::Refusal> {
    let cfg = load_config(rules)?;
    let state = bar_plugin::dirs::bar_state();
    let env = std::env::var("ANTHROPIC_BASE_URL").ok();
    let inst = switch::pick_instance(&cfg, instance, env.as_deref(), &state, known)?;
    let key = switch::key_for(inst)?;
    let live = switch::connect(inst, &key, &state, now())?;
    f(&live)
}

/// `--action <use|next|next-soon|pick>`: `{"action","tokens"}` on stdin, from a
/// click. `use` names an account of the instance's current listing; the
/// `next` actions name a provider and an instance (by its id).
fn action(name: Option<&str>) -> i32 {
    let mut raw = String::new();
    let _ = std::io::stdin().take(1 << 16).read_to_string(&mut raw);
    let rules = bar_plugin::proto::accounts_of_action(&raw);
    let strategy = match name {
        Some("next") => rank::Strategy::MostLeft,
        Some("next-soon") => rank::Strategy::SoonestReset,
        Some("use") => return action_use(&raw, &rules),
        Some("pick") => {
            return finish(
                switch::next_tokens(&raw, "pick").and_then(|(provider, id)| {
                    let cfg = load_config(&rules)?;
                    let inst = switch::instance_by_id(&cfg, &id)?;
                    let state = bar_plugin::dirs::bar_state();
                    let key = switch::key_for(inst)?;
                    let live = switch::connect(inst, &key, &state, now())?;
                    switch::run_choose(&live, &provider, rank::Strategy::MostLeft)
                }),
            )
        }
        _ => {
            eprintln!("bar cliproxyapi: unknown action");
            return 2;
        }
    };
    let name = name.unwrap_or_default();
    finish(switch::next_tokens(&raw, name).and_then(|(provider, id)| {
        let cfg = load_config(&rules)?;
        let inst = switch::instance_by_id(&cfg, &id)?;
        let state = bar_plugin::dirs::bar_state();
        let key = switch::key_for(inst)?;
        let live = switch::connect(inst, &key, &state, now())?;
        switch::run_next(&live, Some(&provider), strategy, false)
    }))
}

/// `use` from a click or a menu: the account by its `auth_index` only, in
/// the instance the `instance` token names, else in the one whose
/// listing holds it.
fn action_use(raw: &str, rules: &[bar_plugin::proto::AccountRule]) -> i32 {
    finish(switch::action_tokens(raw).and_then(|t| {
        let apply = |live: &switch::Live| {
            let target = live
                .entries
                .iter()
                .find(|e| e.index == t.account && e.provider == t.provider)
                .ok_or_else(|| switch::Refusal("no such account".into()))?;
            switch::run_use(live, Some(&target.index), Some(&t.provider), false, false)
        };
        let Some(id) = &t.instance else {
            return session(rules, None, Some((&t.account, &t.provider)), apply);
        };
        let cfg = load_config(rules)?;
        let inst = switch::instance_by_id(&cfg, id)?;
        let state = bar_plugin::dirs::bar_state();
        let key = switch::key_for(inst)?;
        apply(&switch::connect(inst, &key, &state, now())?)
    }))
}
