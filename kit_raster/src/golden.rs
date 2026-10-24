//! Golden-image comparison. Goldens live in `tests/golden/<name>.png` of this crate.
//!
//! - A missing golden is a FAILURE with instructions; it is never created implicitly.
//! - `KIT_BLESS=1` writes (or overwrites) the golden from the actual pixels instead of comparing.
//! - On mismatch, `<name>.actual.png` and `<name>.diff.png` (differing pixels in red over a faded golden) are written
//!   next to the golden.
//!
//! Both sides go through PNG (premultiplied -> straight alpha -> premultiplied) before comparing, so the comparison
//! sees exactly what a blessed file can hold.
use std::fmt;
use std::path::{Path, PathBuf};
use tiny_skia::Pixmap;

/// A pixel differs when any channel differs by more than `channel`; the image passes when at most `pixels` differ.
/// Non-zero tolerances absorb SIMD / platform rounding in anti-aliased edges.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct Tolerance {
    pub channel: u8,
    pub pixels: usize,
}
impl Tolerance {
    pub const EXACT: Self = Self {
        channel: 0,
        pixels: 0,
    };
    /// Anti-aliasing / rounding noise only: +-2 per channel, no pixel may exceed it.
    pub const ROUNDING: Self = Self {
        channel: 2,
        pixels: 0,
    };
}

#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub enum Mode {
    Check,
    Bless,
}
impl Mode {
    /// `Bless` only when the environment variable `KIT_BLESS` is exactly `1`.
    pub fn from_env() -> Self {
        match std::env::var("KIT_BLESS").as_deref() {
            Ok("1") => Mode::Bless,
            _ => Mode::Check,
        }
    }
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum GoldenError {
    /// Names are `[a-z0-9_]+` so they cannot escape the golden directory.
    BadName(String),
    Missing {
        golden: PathBuf,
        actual: PathBuf,
    },
    SizeMismatch {
        expected: (u32, u32),
        actual: (u32, u32),
        actual_png: PathBuf,
    },
    Mismatch {
        differing: usize,
        worst: u8,
        tolerance: Tolerance,
        actual_png: PathBuf,
        diff_png: PathBuf,
    },
    Io {
        path: PathBuf,
        message: String,
    },
    Decode {
        path: PathBuf,
        message: String,
    },
}

impl fmt::Display for GoldenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            GoldenError::BadName(n) => write!(f, "golden name {n:?} must match [a-z0-9_]+"),
            GoldenError::Missing { golden, actual } => write!(
                f,
                "golden image {} does not exist. Goldens are never created implicitly. Inspect the render at {} and, \
                 if it is what you intend, bless it with: KIT_BLESS=1 cargo test -p kit_raster",
                golden.display(),
                actual.display()
            ),
            GoldenError::SizeMismatch {
                expected,
                actual,
                actual_png,
            } => write!(
                f,
                "golden is {}x{} but the render is {}x{} (render written to {}); re-bless with KIT_BLESS=1 if intended",
                expected.0,
                expected.1,
                actual.0,
                actual.1,
                actual_png.display()
            ),
            GoldenError::Mismatch {
                differing,
                worst,
                tolerance,
                actual_png,
                diff_png,
            } => write!(
                f,
                "{differing} pixels differ by more than {} (allowed {}, worst channel delta {worst}); see {} and {}; \
                 re-bless with KIT_BLESS=1 if the change is intended",
                tolerance.channel,
                tolerance.pixels,
                actual_png.display(),
                diff_png.display()
            ),
            GoldenError::Io { path, message } => write!(f, "{}: {message}", path.display()),
            GoldenError::Decode { path, message } => write!(f, "{}: {message}", path.display()),
        }
    }
}
impl std::error::Error for GoldenError {}

/// `tests/golden` of this crate.
pub fn golden_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("golden")
}

/// Panics with the `GoldenError` message unless `actual` matches `tests/golden/<name>.png` within `tolerance`
/// (or, under `KIT_BLESS=1`, writes that golden).
pub fn assert_matches(name: &str, actual: &Pixmap, tolerance: Tolerance) {
    if let Err(e) = compare(&golden_dir(), name, actual, tolerance, Mode::from_env()) {
        panic!("golden `{name}`: {e}");
    }
}

fn io(path: &Path, e: impl fmt::Display) -> GoldenError {
    GoldenError::Io {
        path: path.to_path_buf(),
        message: e.to_string(),
    }
}

fn encode(path: &Path, pixmap: &Pixmap) -> Result<Vec<u8>, GoldenError> {
    pixmap.encode_png().map_err(|e| io(path, e))
}

fn decode(path: &Path, bytes: &[u8]) -> Result<Pixmap, GoldenError> {
    Pixmap::decode_png(bytes).map_err(|e| GoldenError::Decode {
        path: path.to_path_buf(),
        message: e.to_string(),
    })
}

fn write(path: &Path, bytes: &[u8]) -> Result<(), GoldenError> {
    std::fs::write(path, bytes).map_err(|e| io(path, e))
}

fn valid(name: &str) -> bool {
    !name.is_empty()
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

/// The comparison with an explicit directory and mode (what `assert_matches` calls).
pub fn compare(
    dir: &Path,
    name: &str,
    actual: &Pixmap,
    tolerance: Tolerance,
    mode: Mode,
) -> Result<(), GoldenError> {
    if !valid(name) {
        return Err(GoldenError::BadName(name.to_owned()));
    }
    std::fs::create_dir_all(dir).map_err(|e| io(dir, e))?;
    let golden = dir.join(format!("{name}.png"));
    let actual_png = dir.join(format!("{name}.actual.png"));
    let diff_png = dir.join(format!("{name}.diff.png"));
    let bytes = encode(&actual_png, actual)?;

    match mode {
        Mode::Bless => {
            write(&golden, &bytes)?;
            clear(&[&actual_png, &diff_png]);
            Ok(())
        }
        Mode::Check => {
            let expected_bytes = match std::fs::read(&golden) {
                Ok(b) => b,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    write(&actual_png, &bytes)?;
                    return Err(GoldenError::Missing {
                        golden,
                        actual: actual_png,
                    });
                }
                Err(e) => return Err(io(&golden, e)),
            };
            let expected = decode(&golden, &expected_bytes)?;
            let seen = decode(&actual_png, &bytes)?;
            let outcome = diff(&expected, &seen, tolerance);
            match outcome {
                Diff::Same => {
                    clear(&[&actual_png, &diff_png]);
                    Ok(())
                }
                Diff::Size => {
                    write(&actual_png, &bytes)?;
                    Err(GoldenError::SizeMismatch {
                        expected: (expected.width(), expected.height()),
                        actual: (seen.width(), seen.height()),
                        actual_png,
                    })
                }
                Diff::Pixels {
                    differing,
                    worst,
                    picture,
                } => {
                    write(&actual_png, &bytes)?;
                    write(&diff_png, &encode(&diff_png, &picture)?)?;
                    Err(GoldenError::Mismatch {
                        differing,
                        worst,
                        tolerance,
                        actual_png,
                        diff_png,
                    })
                }
            }
        }
    }
}

/// Stale `.actual`/`.diff` files from an earlier failure; a missing file is already the desired state.
fn clear(paths: &[&Path]) {
    for p in paths {
        let _ = std::fs::remove_file(p);
    }
}

enum Diff {
    Same,
    Size,
    Pixels {
        differing: usize,
        worst: u8,
        picture: Pixmap,
    },
}

fn diff(expected: &Pixmap, actual: &Pixmap, tolerance: Tolerance) -> Diff {
    if (expected.width(), expected.height()) != (actual.width(), actual.height()) {
        return Diff::Size;
    }
    let mut picture = expected.clone();
    let (mut differing, mut worst) = (0usize, 0u8);
    let pixels = expected
        .data()
        .chunks_exact(4)
        .zip(actual.data().chunks_exact(4));
    for ((e, a), out) in pixels.zip(picture.data_mut().chunks_exact_mut(4)) {
        let delta = e
            .iter()
            .zip(a)
            .map(|(x, y)| x.abs_diff(*y))
            .max()
            .unwrap_or(0);
        worst = worst.max(delta);
        let bad = delta > tolerance.channel;
        differing += usize::from(bad);
        let faded = |c: u8| ((u16::from(c) + 2 * 255) / 3) as u8;
        out.copy_from_slice(&match bad {
            true => [255, 0, 0, 255],
            false => [faded(e[0]), faded(e[1]), faded(e[2]), 255],
        });
    }
    match differing > tolerance.pixels {
        true => Diff::Pixels {
            differing,
            worst,
            picture,
        },
        false => Diff::Same,
    }
}
