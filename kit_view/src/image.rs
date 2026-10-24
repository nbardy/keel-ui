//! Images as values: a typed handle ([`Image`]) and the app's decoded pixels ([`ImagePixels`]). The kit decodes
//! nothing; a backend registers pixels (or, on the GPU, a game-owned texture) and the view tree carries only handles.
//!
//! Colour space: [`ImagePixels`] are **straight-alpha, sRGB-encoded RGBA8**, row-major, top row first: what a PNG
//! decoder gives, and the same encoding as every other kit colour (a `peniko::Color` is straight sRGB too). Each
//! backend converts them exactly as it converts a solid colour, so an image pixel and a fill of the same RGBA look the
//! same on every target.
use kit_core::kurbo::{Rect, RoundedRectRadii, Size};
use kit_core::peniko::Color;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

/// Process-unique identity of one image: pixels at `ImagePixels::new`, or a GPU texture at registration.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Ord, PartialOrd, Debug)]
pub struct ImageKey(u64);

impl ImageKey {
    fn mint() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        ImageKey(
            NEXT.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| n.checked_add(1))
                .expect("image key space exhausted"),
        )
    }
}

/// Where an image's texels live. The handle says it, so a backend that cannot draw one kind answers with a typed
/// error instead of a blank quad.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
pub enum ImageSource {
    /// App pixels ([`ImagePixels`]): drawable by every backend they are registered with.
    Pixels(ImageKey),
    /// A texture the game owns (a render target, a minimap): the GPU backend only.
    Texture(ImageKey),
}

/// Width and height in texels, both at least 1.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
pub struct ImageSize {
    width: u32,
    height: u32,
}

impl ImageSize {
    pub fn new(width: u32, height: u32) -> Result<Self, ImageSizeError> {
        match width == 0 || height == 0 {
            true => Err(ImageSizeError::Empty { width, height }),
            false => Ok(Self { width, height }),
        }
    }
    pub fn width(self) -> u32 {
        self.width
    }
    pub fn height(self) -> u32 {
        self.height
    }
    /// One texel per logical px: an image node with no size of its own is this big.
    pub fn logical(self) -> Size {
        Size::new(f64::from(self.width), f64::from(self.height))
    }
}

#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub enum ImageSizeError {
    Empty { width: u32, height: u32 },
}

/// The typed handle a view node carries. Only a registration makes one drawable; a handle is never invented by a
/// backend for a key it does not hold. Its size is fixed: new content of another size is a new registration.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
pub struct Image {
    source: ImageSource,
    size: ImageSize,
}

impl Image {
    /// A fresh handle for a GPU texture of `size`. Only the GPU backend's `register_texture` should call this: a
    /// handle nobody registered draws as `RenderError::ImageNotRegistered`.
    pub fn external(size: ImageSize) -> Self {
        Self {
            source: ImageSource::Texture(ImageKey::mint()),
            size,
        }
    }
    pub fn source(self) -> ImageSource {
        self.source
    }
    pub fn key(self) -> ImageKey {
        match self.source {
            ImageSource::Pixels(key) | ImageSource::Texture(key) => key,
        }
    }
    pub fn size(self) -> ImageSize {
        self.size
    }
}

#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub enum PixelsError {
    Size(ImageSizeError),
    /// `rgba` must hold exactly `width * height * 4` bytes.
    Length {
        expected: usize,
        got: usize,
    },
    /// Dimensions cannot be represented as an RGBA byte length on this platform.
    TooLarge,
}

/// Decoded app pixels: straight-alpha, sRGB-encoded RGBA8, row-major, top row first. Cheap to clone (shared bytes);
/// a clone is the same image (same [`ImageKey`]).
#[derive(Clone, Debug)]
pub struct ImagePixels {
    image: Image,
    rgba: Arc<[u8]>,
}

impl ImagePixels {
    pub fn new(width: u32, height: u32, rgba: impl Into<Arc<[u8]>>) -> Result<Self, PixelsError> {
        let size = ImageSize::new(width, height).map_err(PixelsError::Size)?;
        let rgba = rgba.into();
        let expected = (width as usize)
            .checked_mul(height as usize)
            .and_then(|n| n.checked_mul(4))
            .ok_or(PixelsError::TooLarge)?;
        match rgba.len() == expected {
            true => Ok(Self {
                image: Image {
                    source: ImageSource::Pixels(ImageKey::mint()),
                    size,
                },
                rgba,
            }),
            false => Err(PixelsError::Length {
                expected,
                got: rgba.len(),
            }),
        }
    }
    /// The handle these pixels are registered under, on every backend.
    pub fn image(&self) -> Image {
        self.image
    }
    pub fn size(&self) -> ImageSize {
        self.image.size
    }
    pub fn rgba(&self) -> &[u8] {
        &self.rgba
    }
}

/// How an image fills its node's box.
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub enum Fit {
    /// Exactly the box; the aspect ratio follows the box.
    Stretch,
    /// As large as fits inside the box with its aspect kept, centred (letterboxed).
    Contain,
}

/// How texels are filtered when the image is drawn at another size.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
pub enum Filter {
    /// Bilinear: photos, portraits, render targets.
    Linear,
    /// Nearest texel: pixel art.
    Nearest,
}

/// The look of an image node.
#[derive(Copy, Clone, PartialEq, Debug)]
pub struct ImageStyle {
    pub fit: Fit,
    /// Multiplies every texel (premultiplied). White is the image as it is.
    pub tint: Color,
    pub filter: Filter,
    /// Corner radii of the drawn image rect (after fitting), logical px.
    pub radii: RoundedRectRadii,
}

impl ImageStyle {
    pub fn new(fit: Fit) -> Self {
        Self {
            fit,
            tint: Color::WHITE,
            filter: Filter::Linear,
            radii: RoundedRectRadii::from_single_radius(0.0),
        }
    }
    pub fn stretch() -> Self {
        Self::new(Fit::Stretch)
    }
    pub fn contain() -> Self {
        Self::new(Fit::Contain)
    }
    pub fn tint(self, tint: Color) -> Self {
        Self { tint, ..self }
    }
    pub fn filter(self, filter: Filter) -> Self {
        Self { filter, ..self }
    }
    pub fn rounded(self, radii: impl Into<RoundedRectRadii>) -> Self {
        Self {
            radii: radii.into(),
            ..self
        }
    }
}

impl Fit {
    /// Destination inside the layout box. Empty boxes stay empty.
    pub fn rect(self, size: ImageSize, bounds: Rect) -> Rect {
        match self {
            Self::Stretch => bounds,
            Self::Contain => {
                let source = size.logical();
                let scale = (bounds.width().max(0.0) / source.width)
                    .min(bounds.height().max(0.0) / source.height);
                let fitted = Size::new(source.width * scale, source.height * scale);
                Rect::from_center_size(bounds.center(), fitted)
            }
        }
    }
}

/// An image leaf; size it through the usual node style methods. Without a size it measures one logical px per texel.
pub fn image<Msg>(image: Image, image_style: ImageStyle) -> crate::Node<Msg> {
    crate::Node::Image {
        style: crate::Style::column(),
        image,
        image_style,
    }
}
