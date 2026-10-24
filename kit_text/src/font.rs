//! Explicit font sourcing. wasm has no system fonts, so `Static` (bytes compiled into the binary) and `Embedded` (bytes
//! the host fetched) are the primary paths; `System` is for desktop/mobile hosts. All name the three generic families up
//! front and construction fails with a typed error if a name is not in the loaded database (no silent "some other font").
//!
//! A [`SharedFonts`] is the loaded database plus cosmic-text's shaping state. Several `CosmicText`s (one per canvas in
//! the browser, since each wgpu device needs its own atlas) can be built over one, so the face data is parsed and
//! cached once.

use cosmic_text::{FontSystem, fontdb};
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

/// Concrete family names the generic `FontFamily::{Sans, Serif, Mono}` resolve to.
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct GenericFamilies {
    pub sans: String,
    pub serif: String,
    pub mono: String,
}

#[derive(Clone, Debug)]
pub enum FontSource {
    /// Font files compiled into the binary (`include_bytes!`), loaded in order. The database borrows the static bytes:
    /// nothing is copied, however many text systems are built from it. Works on every target including wasm.
    Static {
        faces: &'static [&'static [u8]],
        generics: GenericFamilies,
    },
    /// Font file bytes (ttf/otf/ttc) the host owns at runtime (fetched, read from disk), loaded in order.
    Embedded {
        faces: Vec<Vec<u8>>,
        generics: GenericFamilies,
    },
    /// Platform font database (not available on wasm: yields `FontError::NoFaces`).
    System { generics: GenericFamilies },
}

#[derive(Clone, Eq, PartialEq, Debug)]
pub enum FontError {
    /// The source produced a database with no faces at all.
    NoFaces,
    /// A generic family names a family that is not present in the database.
    MissingFamily(String),
}

/// The font database and shaping state, shared by every [`crate::CosmicText`] built over it. Single-threaded by
/// construction (`Rc`): the text systems borrow it one call at a time and never across a call into another.
#[derive(Clone)]
pub struct SharedFonts(pub(crate) Rc<RefCell<FontSystem>>);

impl FontSource {
    /// Load the faces and check the generic families; `locale` picks script fallbacks (never read from the platform).
    pub fn into_shared(self, locale: String) -> Result<SharedFonts, FontError> {
        let db = self.into_database()?;
        Ok(SharedFonts(Rc::new(RefCell::new(
            FontSystem::new_with_locale_and_db(locale, db),
        ))))
    }

    fn into_database(self) -> Result<fontdb::Database, FontError> {
        let (mut db, generics) = match self {
            Self::Static { faces, generics } => {
                let mut db = fontdb::Database::new();
                for bytes in faces {
                    db.load_font_source(fontdb::Source::Binary(Arc::new(*bytes)));
                }
                (db, generics)
            }
            Self::Embedded { faces, generics } => {
                let mut db = fontdb::Database::new();
                faces.into_iter().for_each(|bytes| db.load_font_data(bytes));
                (db, generics)
            }
            Self::System { generics } => {
                let mut db = fontdb::Database::new();
                db.load_system_fonts();
                (db, generics)
            }
        };
        if db.is_empty() {
            return Err(FontError::NoFaces);
        }
        for name in [&generics.sans, &generics.serif, &generics.mono] {
            let present = db
                .faces()
                .any(|face| face.families.iter().any(|(family, _)| family == name));
            if !present {
                return Err(FontError::MissingFamily(name.clone()));
            }
        }
        db.set_sans_serif_family(generics.sans);
        db.set_serif_family(generics.serif);
        db.set_monospace_family(generics.mono);
        Ok(db)
    }
}

/// The UI typeface the hosts compile in: Inter Regular and Bold, subset to what the game draws
/// (`kit_text/fonts/README.md` has the subsetting command and the SHA-256s are in `THIRD_PARTY.md`). Also the
/// font of every kit test that lays out text, so tests measure what ships (goldens move when cosmic-text or these
/// bytes change; re-bless them in one commit).
pub mod inter {
    use super::{FontSource, GenericFamilies};

    /// Font files, loaded in order.
    pub const FACES: &[&[u8]] = &[
        include_bytes!("../fonts/subset/Inter-Regular.latin.ttf"),
        include_bytes!("../fonts/subset/Inter-Bold.latin.ttf"),
    ];

    /// The family name every face above registers, which is also the sans, serif and mono the UI asks for.
    pub const FAMILY: &str = "Inter";

    /// [`FACES`] as a `FontSource` whose three generic families are all [`FAMILY`].
    pub fn source() -> FontSource {
        FontSource::Static {
            faces: FACES,
            generics: GenericFamilies {
                sans: FAMILY.to_owned(),
                serif: FAMILY.to_owned(),
                mono: FAMILY.to_owned(),
            },
        }
    }
}
