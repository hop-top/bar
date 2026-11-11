//! A second `bar-tray --lock <file>` exits at once while the first holds
//! the lock, without showing anything.

use std::io::{Read, Seek};
use std::process::Command;

#[test]
fn a_held_lock_makes_a_second_tray_exit_zero() {
    let dir = std::env::temp_dir().join(format!("bar-tray-single-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let lock = dir.join("tray.lock");
    let mut held = bar_tray::lock::acquire(&lock).unwrap().unwrap();
    let snapshot = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/snapshot.json");
    let o = Command::new(env!("CARGO_BIN_EXE_bar-tray"))
        .args(["--snapshot", snapshot, "--lock"])
        .arg(&lock)
        .output()
        .unwrap();
    assert!(o.status.success(), "{o:?}");
    assert_eq!(
        String::from_utf8_lossy(&o.stderr),
        "bar-tray: already running\n"
    );
    // the holder's pid is still there (read through its handle: Windows
    // refuses every other one while the lock is held)
    let mut pid = String::new();
    held.rewind().unwrap();
    held.read_to_string(&mut pid).unwrap();
    assert_eq!(pid, format!("{}\n", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
}
