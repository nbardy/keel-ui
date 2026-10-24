//! The golden harness itself: a missing golden must fail loudly, only KIT_BLESS-mode creates one.
use kit_raster::golden::{GoldenError, Mode, Tolerance, compare};
use kit_raster::tiny_skia::{Color, Pixmap};
use std::path::PathBuf;

fn scratch(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("kit_raster_harness_{}_{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}
fn flat(v: u8) -> Pixmap {
    let mut p = Pixmap::new(6, 6).expect("size");
    p.fill(Color::from_rgba8(v, v, v, 255));
    p
}

#[test]
fn a_missing_golden_fails_with_instructions_and_is_never_created_implicitly() {
    let dir = scratch("missing");
    let err =
        compare(&dir, "shot", &flat(10), Tolerance::EXACT, Mode::Check).expect_err("must fail");
    assert!(matches!(err, GoldenError::Missing { .. }));
    assert!(err.to_string().contains("KIT_BLESS=1"));
    assert!(
        !dir.join("shot.png").exists(),
        "check mode must not write the golden"
    );
    assert!(
        dir.join("shot.actual.png").exists(),
        "the render is left for inspection"
    );
}

#[test]
fn bless_then_check_passes_and_a_change_writes_actual_and_diff() {
    let dir = scratch("bless");
    compare(&dir, "shot", &flat(10), Tolerance::EXACT, Mode::Bless).expect("bless");
    compare(&dir, "shot", &flat(10), Tolerance::EXACT, Mode::Check).expect("identical");

    // Off by one: inside the rounding tolerance, outside EXACT.
    compare(&dir, "shot", &flat(11), Tolerance::ROUNDING, Mode::Check).expect("within tolerance");
    let err = compare(&dir, "shot", &flat(11), Tolerance::EXACT, Mode::Check).expect_err("exact");
    assert!(
        matches!(
            err,
            GoldenError::Mismatch {
                differing: 36,
                worst: 1,
                ..
            }
        ),
        "{err:?}"
    );

    let mut changed = flat(10);
    changed.pixels_mut()[7] = tiny_skia_red();
    let err = compare(&dir, "shot", &changed, Tolerance::ROUNDING, Mode::Check)
        .expect_err("one red pixel");
    assert!(
        matches!(err, GoldenError::Mismatch { differing: 1, .. }),
        "{err:?}"
    );
    let diff = Pixmap::load_png(dir.join("shot.diff.png")).expect("diff written");
    assert_eq!(diff.pixels()[7].red(), 255);
    assert_eq!(
        diff.pixels()[7].green(),
        0,
        "the differing pixel is marked red"
    );
    assert!(dir.join("shot.actual.png").exists());

    let wide = Pixmap::new(7, 6).expect("size");
    assert!(matches!(
        compare(&dir, "shot", &wide, Tolerance::ROUNDING, Mode::Check),
        Err(GoldenError::SizeMismatch { .. })
    ));
    compare(&dir, "shot", &flat(10), Tolerance::EXACT, Mode::Check).expect("passes again");
    assert!(
        !dir.join("shot.diff.png").exists(),
        "stale failure artefacts are removed on success"
    );
}

#[test]
fn names_cannot_escape_the_golden_directory() {
    let dir = scratch("names");
    for bad in ["", "../x", "a/b", "Shot", "x.png"] {
        assert!(matches!(
            compare(&dir, bad, &flat(0), Tolerance::EXACT, Mode::Bless),
            Err(GoldenError::BadName(_))
        ));
    }
}

fn tiny_skia_red() -> kit_raster::tiny_skia::PremultipliedColorU8 {
    kit_raster::tiny_skia::PremultipliedColorU8::from_rgba(255, 0, 0, 255).expect("opaque")
}
