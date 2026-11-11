use super::*;
use crate::model;

fn fixture() -> Menu {
    let s = model::parse(include_str!("../../../tests/fixtures/snapshot.json")).unwrap();
    Menu::build(&s, 1_762_800_000)
}

#[test]
fn fixture_maps_to_rows() {
    let e = entries(&fixture());
    let Entry::Sub {
        label, children, ..
    } = &e[0]
    else {
        panic!("first row is the Claude submenu: {:?}", e[0]);
    };
    assert_eq!(label, "Claude · :8317");
    // the three accounts are one radio group, alice (index 0) selected
    let Entry::Radio { selected, options } = &children[0] else {
        panic!("{:?}", children[0]);
    };
    assert_eq!((*selected, options.len()), (0, 3));
    assert_eq!(options[0].id, "cliproxyapi.claude@8317.3");
    assert_eq!(options[0].label, "alice — 5h 59% ↻2h10m · 7d 71%");
    assert_eq!(options[0].tone, Tone::Normal);
    assert_eq!(children.len(), 1);
    // buttons after the submenu stay plain rows; the info row is disabled
    assert!(matches!(&e[1], Entry::Item { enabled: true, .. }));
    let Entry::Item {
        id, enabled, tone, ..
    } = &e[3]
    else {
        panic!("{:?}", e[3]);
    };
    assert_eq!(id, "cliproxyapi.info.auth");
    assert!(!enabled);
    assert_eq!(*tone, Tone::Alert);
}

#[test]
fn separators_toggles_and_quit() {
    let e = entries(&fixture());
    assert!(e.contains(&Entry::Separator));
    let check = e
        .iter()
        .find_map(|r| match r {
            Entry::Check { id, checked, .. } if id == "tray.view.5h" => Some(*checked),
            _ => None,
        })
        .expect("view toggle row");
    assert!(check);
    assert!(matches!(
        e.last(),
        Some(Entry::Item { id, enabled: true, .. }) if id == "tray.quit"
    ));
}

#[test]
fn disabled_toggle_stays_a_check_row() {
    let e = entries(&fixture());
    let found = e.iter().any(|r| {
        matches!(
            r,
            Entry::Check { id, enabled: false, checked: false, .. }
                if id == "routellm.rtellm@8317.toggle@8318"
        )
    });
    assert!(found);
}

#[test]
fn radio_without_checked_option_selects_nothing() {
    let mut m = fixture();
    m.find_mut("cliproxyapi.claude@8317.3").unwrap().checked = false;
    let Entry::Sub { children, .. } = &entries(&m)[0] else {
        panic!()
    };
    let Entry::Radio { selected, .. } = &children[0] else {
        panic!()
    };
    assert_eq!(*selected, usize::MAX);
}

#[test]
fn underscores_are_escaped() {
    let mut m = fixture();
    m.find_mut("tray.quit").unwrap().label = "snake_case _x".into();
    let last = entries(&m).pop().unwrap();
    let Entry::Item { label, .. } = last else {
        panic!()
    };
    assert_eq!(label, "snake__case __x");
}

#[test]
fn tones_follow_style() {
    assert_eq!(Tone::of(None), Tone::Normal);
    assert_eq!(Tone::of(Some(Style::Good)), Tone::Normal);
    assert_eq!(Tone::of(Some(Style::Warn)), Tone::Warning);
    assert_eq!(Tone::of(Some(Style::Bad)), Tone::Alert);
    assert_eq!(Tone::of(Some(Style::Dim)), Tone::Informative);
}

#[test]
fn pixmaps_are_argb_squares_with_ink() {
    const INK: [u8; 4] = [0xFF, 0x90, 0x90, 0x90];
    for size in PIXMAP_SIZES {
        let px = pixmap(size);
        assert_eq!(px.len(), (size * size * 4) as usize);
        let (px4, rest) = px.as_chunks::<4>();
        assert!(rest.is_empty());
        let inked = px4.iter().filter(|p| **p == INK).count();
        let clear = px4.iter().filter(|p| p[0] == 0).count();
        assert!(inked > 0 && clear > inked, "size {size}");
        assert_eq!(inked + clear, px4.len(), "only ink or transparent");
    }
}

#[test]
fn bad_pixmap_adds_a_red_dot() {
    for s in PIXMAP_SIZES {
        let (a, b) = (pixmap(s), pixmap_bad(s));
        assert_eq!(a.len(), b.len());
        assert_ne!(a, b);
        assert!(b.chunks(4).any(|p| p == [0xFF, 0xE0, 0x30, 0x30]));
        assert!(!a.chunks(4).any(|p| p == [0xFF, 0xE0, 0x30, 0x30]));
    }
}
