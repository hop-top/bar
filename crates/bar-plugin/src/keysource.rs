//! Where a plugin's credential comes from: an owner-only file, the macOS
//! Keychain, or a command run without a shell. The key is read into a
//! `Secret` and nowhere else: never an argument, an environment variable,
//! a cache or an output.

use crate::fsio::{read_private, FileError};
use crate::secret::{wipe, Secret};
use std::io::Read;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// Longest key accepted, in bytes.
pub const MAX_KEY: usize = 1024;

/// Time a keychain or command read may take.
pub const BUDGET: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeySource {
    /// First line of an owner-only (0600) file.
    File(PathBuf),
    /// macOS generic password by service name.
    Keychain(String),
    /// Absolute program and arguments; stdout's first line.
    Command(Vec<String>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeyError {
    File(FileError),
    /// Nothing usable: empty, over `MAX_KEY`, or not printable ASCII.
    Invalid,
    /// The keychain or command did not finish in time.
    TimedOut,
    /// The keychain or command failed (the program's own message is never
    /// kept: it could echo the secret).
    Failed,
    /// Not available on this platform or not allowed (relative program).
    Unsupported(&'static str),
}

impl std::fmt::Display for KeyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            KeyError::File(e) => write!(f, "key file: {e}"),
            KeyError::Invalid => write!(f, "key is empty, too long or not printable ASCII"),
            KeyError::TimedOut => write!(f, "key source timed out"),
            KeyError::Failed => write!(f, "key source failed"),
            KeyError::Unsupported(w) => write!(f, "{w}"),
        }
    }
}

/// The key of `src`.
pub fn read(src: &KeySource) -> Result<Secret, KeyError> {
    match src {
        KeySource::File(p) => {
            let raw = read_private(p, MAX_KEY + 2).map_err(KeyError::File)?;
            first_line(raw.as_bytes())
        }
        KeySource::Keychain(service) => keychain(service),
        KeySource::Command(argv) => command(argv),
    }
}

/// The first line of `raw`, trimmed, when it is a plausible key.
pub fn first_line(raw: &[u8]) -> Result<Secret, KeyError> {
    let line = raw.split(|&b| b == b'\n').next().unwrap_or_default();
    let line = line.trim_ascii();
    if line.is_empty() || line.len() > MAX_KEY || line.iter().any(|&b| !(0x21..=0x7e).contains(&b))
    {
        return Err(KeyError::Invalid);
    }
    Ok(Secret::new(line.to_vec()))
}

#[cfg(target_os = "macos")]
fn keychain(service: &str) -> Result<Secret, KeyError> {
    if service.is_empty() || service.len() > 256 || service.chars().any(char::is_control) {
        return Err(KeyError::Invalid);
    }
    run(&[
        "/usr/bin/security".into(),
        "find-generic-password".into(),
        "-s".into(),
        service.into(),
        "-w".into(),
    ])
}

#[cfg(not(target_os = "macos"))]
fn keychain(_service: &str) -> Result<Secret, KeyError> {
    Err(KeyError::Unsupported(
        "the keychain is available on macOS only",
    ))
}

fn command(argv: &[String]) -> Result<Secret, KeyError> {
    let prog = argv.first().ok_or(KeyError::Invalid)?;
    if !std::path::Path::new(prog).is_absolute() {
        return Err(KeyError::Unsupported(
            "key command must be an absolute path",
        ));
    }
    run(argv)
}

/// Runs `argv` (no shell, no stdin, stderr dropped) and takes the first
/// line of its stdout, within `BUDGET`.
fn run(argv: &[String]) -> Result<Secret, KeyError> {
    let mut child = Command::new(&argv[0])
        .args(&argv[1..])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| KeyError::Failed)?;
    let mut out = child.stdout.take().ok_or(KeyError::Failed)?;
    let reader = std::thread::spawn(move || {
        let mut buf = Secret::with_capacity(MAX_KEY + 64);
        let mut chunk = [0u8; 256];
        loop {
            match out.read(&mut chunk) {
                Ok(0) => break,
                Ok(n) if buf.as_bytes().len() + n > MAX_KEY + 64 => {
                    wipe(&mut chunk);
                    return Err(KeyError::Invalid);
                }
                Ok(n) => buf.push(&chunk[..n]),
                Err(_) => break,
            }
        }
        wipe(&mut chunk);
        Ok(buf)
    });
    let end = Instant::now() + BUDGET;
    let status = loop {
        match child.try_wait() {
            Ok(Some(st)) => break st,
            Ok(None) if Instant::now() >= end => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = reader.join();
                return Err(KeyError::TimedOut);
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(10)),
            Err(_) => return Err(KeyError::Failed),
        }
    };
    let buf = reader.join().map_err(|_| KeyError::Failed)??;
    if !status.success() {
        return Err(KeyError::Failed);
    }
    first_line(buf.as_bytes())
}

/// A fresh random salt, hex: from the standard library's per-process
/// random hasher keys, mixed with the clock and process id.
pub fn new_salt() -> String {
    use std::hash::{BuildHasher, Hasher};
    let mut seed = Vec::new();
    for _ in 0..2 {
        let mut h = std::collections::hash_map::RandomState::new().build_hasher();
        h.write_u32(std::process::id());
        seed.extend_from_slice(&h.finish().to_le_bytes());
    }
    let t = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    seed.extend_from_slice(&t.to_le_bytes());
    crate::sha256::hex(&seed)[..32].to_string()
}

/// A salted digest of the key (SHA-256 over `salt`, a zero byte and the
/// key; the first 16 bytes, hex): stands for the key in a cache that
/// keeps the salt beside it, without a bare hash of the key to look up.
pub fn fingerprint(key: &Secret, salt: &str) -> String {
    let mut m = Secret::with_capacity(salt.len() + 1 + key.as_bytes().len());
    m.push(salt.as_bytes());
    m.push(&[0]);
    m.push(key.as_bytes());
    crate::sha256::hex_wiped(m.as_bytes())[..32].to_string()
}

/// Splits a command line into arguments: whitespace separates, `"` and `'`
/// quote (a backslash escapes `"` and `\` inside `"`), nothing else is
/// special: no variables, globs, `~` or pipes. `None` for an unclosed
/// quote or an empty line.
pub fn split_argv(s: &str) -> Option<Vec<String>> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut have = false;
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        match c {
            c if c.is_whitespace() => {
                if have {
                    out.push(std::mem::take(&mut cur));
                    have = false;
                }
            }
            '\'' => {
                have = true;
                loop {
                    match chars.next()? {
                        '\'' => break,
                        c => cur.push(c),
                    }
                }
            }
            '"' => {
                have = true;
                loop {
                    match chars.next()? {
                        '"' => break,
                        '\\' => match chars.next()? {
                            c @ ('"' | '\\') => cur.push(c),
                            c => {
                                cur.push('\\');
                                cur.push(c);
                            }
                        },
                        c => cur.push(c),
                    }
                }
            }
            c => {
                have = true;
                cur.push(c);
            }
        }
    }
    if have {
        out.push(cur);
    }
    (!out.is_empty()).then_some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_line_is_trimmed_and_checked() {
        assert_eq!(
            first_line(b"  abc123 \nsecond").unwrap().as_bytes(),
            b"abc123"
        );
        assert_eq!(first_line(b"abc\r\n").unwrap().as_bytes(), b"abc");
        for bad in [&b""[..], b"\n", b"   ", b"a b", b"a\x01b", "é".as_bytes()] {
            assert_eq!(first_line(bad).unwrap_err(), KeyError::Invalid, "{bad:?}");
        }
        assert!(first_line(&vec![b'a'; MAX_KEY]).is_ok());
        assert!(first_line(&vec![b'a'; MAX_KEY + 1]).is_err());
    }

    #[test]
    fn argv_splitting_has_no_shell_features() {
        assert_eq!(
            split_argv(r#"/bin/echo "a b" 'c d' e\f $HOME"#).unwrap(),
            ["/bin/echo", "a b", "c d", r"e\f", "$HOME"]
        );
        assert_eq!(split_argv(r#""" x"#).unwrap(), ["", "x"]);
        assert_eq!(split_argv(r#""a\"b\\c""#).unwrap(), [r#"a"b\c"#]);
        assert_eq!(split_argv("a | b").unwrap(), ["a", "|", "b"]);
        assert!(split_argv(r#"a "b"#).is_none());
        assert!(split_argv("   ").is_none());
    }

    #[test]
    fn fingerprint_is_salted_and_short() {
        let k = Secret::new(b"abc".to_vec());
        let a = fingerprint(&k, "s1");
        assert_eq!(a.len(), 32);
        assert_eq!(a, fingerprint(&k, "s1"));
        assert_ne!(a, fingerprint(&k, "s2"));
        assert!(!"ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad".contains(&a));
        assert_ne!(new_salt(), new_salt());
        assert_eq!(new_salt().len(), 32);
    }

    #[cfg(unix)]
    #[test]
    fn command_source_reads_stdout_without_a_shell() {
        let k = read(&KeySource::Command(vec![
            "/bin/echo".into(),
            "k3y-value".into(),
            "$HOME".into(),
        ]))
        .unwrap_err();
        // "k3y-value $HOME" has a space: not a key
        assert_eq!(k, KeyError::Invalid);
        let k = read(&KeySource::Command(vec![
            "/bin/echo".into(),
            "k3y-value".into(),
        ]))
        .unwrap();
        assert_eq!(k.as_bytes(), b"k3y-value");
        assert_eq!(
            read(&KeySource::Command(vec!["echo".into(), "x".into()])).unwrap_err(),
            KeyError::Unsupported("key command must be an absolute path")
        );
        assert_eq!(
            read(&KeySource::Command(vec!["/usr/bin/false".into()])).unwrap_err(),
            KeyError::Failed
        );
        assert_eq!(
            read(&KeySource::Command(vec!["/nonexistent/prog".into()])).unwrap_err(),
            KeyError::Failed
        );
    }

    #[cfg(unix)]
    #[test]
    fn file_source_refuses_loose_modes() {
        use std::os::unix::fs::PermissionsExt;
        let p = std::env::temp_dir().join(format!("bar-plugin-key-{}", std::process::id()));
        std::fs::write(&p, "my-key\nignored\n").unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o604)).unwrap();
        assert_eq!(
            read(&KeySource::File(p.clone())).unwrap_err(),
            KeyError::File(FileError::Mode(0o604))
        );
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o600)).unwrap();
        assert_eq!(
            read(&KeySource::File(p.clone())).unwrap().as_bytes(),
            b"my-key"
        );
        let _ = std::fs::remove_file(&p);
    }
}
