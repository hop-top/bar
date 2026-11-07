//! Reference plugins driven directly: JSON on stdin, JSON on stdout, fakes
//! for `tlc`, `security` and `curl` first on `PATH`.
mod common;
#[path = "common/plugins_fx.rs"]
mod plugins_fx;
#[allow(unused_imports)]
use plugins_fx::*;

use serde_json::json;
use std::process::Command;

// ---- bar-claude-credits ----

#[test]
fn credits_ext_info_declares_placeholder() {
    let o = Command::new(script("bar-claude-credits"))
        .arg("--ext-info")
        .output()
        .unwrap();
    assert!(o.status.success());
    let v = out_json(&o);
    assert_eq!(v["name"], "claude-credits");
    assert!(v["capabilities"]
        .as_array()
        .unwrap()
        .contains(&json!("placeholder")));
}

fn iso(ts: i64) -> String {
    let (days, rem) = (ts.div_euclid(DAY), ts.rem_euclid(DAY));
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
}

fn credits_body(exp: i64, granted_at: i64) -> String {
    json!({
        "amount": 14906,
        "amount_without_scoped_credits": 100,
        "promo_tranches": [
            {"remaining_amount_minor_units": 4807, "granted_amount_minor_units": 5000,
             "expires_at": iso(exp + 30 * DAY), "granted_at": iso(granted_at)},
            {"remaining_amount_minor_units": 10000, "granted_amount_minor_units": 20000,
             "expires_at": iso(exp), "granted_at": iso(granted_at)}
        ]
    })
    .to_string()
}

#[test]
fn credits_sum_promo_tranches() {
    let fx = Fx::new();
    let exp = now() + 14 * DAY + 3600;
    let granted_at = exp - 30 * DAY;
    fx.credits_fakes(&credits_body(exp, granted_at), "200");
    let o = fx.credits();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let v = out_json(&o);
    assert_eq!(v["text"], "14d $149.07");
    assert_eq!(
        v["metric"],
        json!({"remaining":14907,"granted":20000,"expires_at":exp,"granted_at":granted_at})
    );
    assert!(v.get("style").is_none());
    assert!(fx
        .log("curl.log")
        .contains("/api/organizations/org-1/prepaid/credits"));
    assert!(fx.log("security.log").contains("claude-console-session-x"));
}

#[test]
fn credits_401_shows_question_mark() {
    let fx = Fx::new();
    fx.credits_fakes("{}", "401");
    let o = fx.credits();
    assert!(o.status.success());
    let v = out_json(&o);
    assert_eq!(v["text"], "?");
    assert_eq!(v["style"], "bad");
}

#[test]
fn credits_503_emits_nothing_so_cache_keeps_value() {
    let fx = Fx::new();
    for code in ["503", "fail"] {
        fx.credits_fakes("{}", code);
        let o = fx.credits();
        assert!(!o.status.success(), "{code}");
        assert!(o.stdout.is_empty(), "{code}");
    }
}

#[test]
fn credits_no_keychain_entry_omitted() {
    let fx = Fx::new();
    fx.credits_fakes("{}", "200");
    std::fs::remove_file(fx.fake.join("cookie")).unwrap();
    let o = fx.credits();
    assert!(!o.status.success());
    assert!(o.stdout.is_empty());
    assert!(fx.log("curl.log").is_empty(), "curl must not run");
}

#[test]
fn cookie_never_in_argv() {
    let fx = Fx::new();
    let exp = now() + 14 * DAY + 3600;
    fx.credits_fakes(&credits_body(exp, exp - 30 * DAY), "200");
    let o = fx.credits();
    assert!(o.status.success());
    let argv = fx.log("curl.log");
    assert!(!argv.is_empty());
    assert!(!argv.contains(SENTINEL), "argv leaked: {argv}");
    assert!(!fx.log("security.log").contains(SENTINEL));
    assert!(!fx.log("curl.env").contains(SENTINEL), "env leaked");
    assert!(
        fx.log("curl.stdin").contains(SENTINEL),
        "cookie must arrive on stdin"
    );
    assert!(!String::from_utf8_lossy(&o.stdout).contains(SENTINEL));
}

#[test]
fn credits_403_shows_question_mark() {
    let fx = Fx::new();
    fx.credits_fakes("{}", "403");
    let v = out_json(&fx.credits());
    assert_eq!(v["text"], "?");
    assert_eq!(v["style"], "bad");
}

#[test]
fn credits_invalid_org_or_keychain_label_omitted() {
    let fx = Fx::new();
    fx.credits_fakes("{}", "200");
    for (org, kc) in [
        ("../x", "k"),
        ("a b", "k"),
        ("", "k"),
        ("org-1", ""),
        ("org-1", "a;b"),
        ("org-1", "-w"),
    ] {
        let o = fx.credits_with(org, kc);
        assert!(!o.status.success(), "{org}/{kc}");
        assert!(o.stdout.is_empty(), "{org}/{kc}");
    }
    assert!(fx.log("security.log").is_empty());
    assert!(fx.log("curl.log").is_empty());
}

#[test]
fn credits_invalid_cookie_charset_omitted() {
    let fx = Fx::new();
    fx.credits_fakes("{}", "200");
    fx.put("cookie", "abc\"; evil=1\n");
    let o = fx.credits();
    assert!(!o.status.success());
    assert!(o.stdout.is_empty());
    assert!(fx.log("curl.log").is_empty());
}

#[test]
fn credits_inherited_cookie_var_never_reaches_curl_env() {
    let mut fx = Fx::new();
    let exp = now() + 14 * DAY + 3600;
    fx.credits_fakes(&credits_body(exp, exp - 30 * DAY), "200");
    fx.extra.push(("cookie", "inherited-secret-zz".into()));
    fx.put("cookie", "inherited-secret-zz\n");
    let o = fx.credits();
    assert!(o.status.success());
    assert!(!fx.log("curl.env").contains("inherited-secret-zz"));
}
