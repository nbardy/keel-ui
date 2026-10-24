//! Glyph atlas: R8 coverage pages and RGBA8 colour pages, packed by etagere, filled lazily from
//! `CosmicText::rasterize`. Eviction is page-granular LRU by frame generation: when nothing fits, the page whose
//! most recent use is oldest is reset, provided that use is in an encoder already submitted (`submitted`).
//! `Queue::write_texture` is ordered before the *next submission*, so resetting and refilling a page that an
//! unsubmitted encoder still samples would repaint that earlier draw with the new glyphs.
use crate::gpu::Gpu;
use etagere::BucketedAtlasAllocator;
use kit_paint::RenderError;
use kit_view::CosmicText;
use kit_view::kit_text::{CacheKey, GlyphImage, GlyphMask, GlyphRaster};
use std::collections::HashMap;

const PAGE: u32 = 1024;
const MAX_PAGES_PER_KIND: usize = 4;
const PAD: u32 = 1;

#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
pub(crate) struct PageId {
    pub colour: bool,
    pub index: usize,
}

/// Where a glyph's pixels live and how to place them.
#[derive(Copy, Clone, Debug)]
pub(crate) struct Placed {
    pub page: PageId,
    /// u0 v0 u1 v1
    pub uv: [f32; 4],
    pub width: u32,
    pub height: u32,
    pub left: i32,
    pub top: i32,
}

#[derive(Copy, Clone, Debug)]
pub(crate) enum Slot {
    Placed(Placed),
    /// Whitespace: nothing to draw.
    Empty,
    /// Rasteriser does not know the glyph: draw tofu.
    Missing,
}

/// Per-page rectangle packer. `BucketedAtlasAllocator` rather than `AtlasAllocator`: glyphs are many, small and of
/// similar height, and are never freed one by one (eviction recycles a whole page via `clear`), so per-item
/// bookkeeping for fragmentation handling would be pure cost. Every rect is reserved `PAD` larger on the right and
/// bottom so linear sampling at a glyph edge never bleeds into a neighbour.
struct Packer(BucketedAtlasAllocator);

impl Packer {
    fn new() -> Self {
        Self(BucketedAtlasAllocator::new(etagere::size2(
            PAGE as i32,
            PAGE as i32,
        )))
    }

    fn reset(&mut self) {
        self.0.clear();
    }

    /// `None` when the padded rect does not fit what is left of the page.
    fn alloc(&mut self, w: u32, h: u32) -> Option<(u32, u32)> {
        let padded = etagere::size2((w + PAD) as i32, (h + PAD) as i32);
        let min = self.0.allocate(padded)?.rectangle.min;
        Some((min.x as u32, min.y as u32))
    }
}

struct Page {
    texture: wgpu::Texture,
    pub bind_group: wgpu::BindGroup,
    packer: Packer,
    last_used: u64,
}

impl Page {
    fn new(gpu: &Gpu, colour: bool) -> Self {
        let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("kit glyph page"),
            size: wgpu::Extent3d {
                width: PAGE,
                height: PAGE,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: if colour {
                wgpu::TextureFormat::Rgba8Unorm
            } else {
                wgpu::TextureFormat::R8Unorm
            },
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let view = texture.create_view(&Default::default());
        let bind_group = gpu.texture_bind_group(&view, &gpu.nearest);
        Self {
            texture,
            bind_group,
            packer: Packer::new(),
            last_used: 0,
        }
    }
}

pub(crate) struct Atlas {
    alpha: Vec<Page>,
    colour: Vec<Page>,
    entries: HashMap<CacheKey, Slot>,
    /// Highest generation whose encoders are known to be submitted: pages last used at or before it may be recycled.
    submitted_through: u64,
}

impl Atlas {
    pub fn new() -> Self {
        Self {
            alpha: Vec::new(),
            colour: Vec::new(),
            entries: HashMap::new(),
            submitted_through: 0,
        }
    }

    /// Everything recorded up to and including `generation` has been submitted.
    pub fn submitted(&mut self, generation: u64) {
        self.submitted_through = generation;
    }

    pub fn bind_group(&self, id: PageId) -> &wgpu::BindGroup {
        let pages = if id.colour { &self.colour } else { &self.alpha };
        &pages[id.index].bind_group
    }

    pub fn page_count(&self) -> usize {
        self.alpha.len() + self.colour.len()
    }

    fn pages(&mut self, colour: bool) -> &mut Vec<Page> {
        if colour {
            &mut self.colour
        } else {
            &mut self.alpha
        }
    }

    pub fn slot(
        &mut self,
        gpu: &Gpu,
        text: &mut CosmicText,
        key: CacheKey,
        generation: u64,
    ) -> Result<Slot, RenderError> {
        if let Some(slot) = self.entries.get(&key).copied() {
            if let Slot::Placed(p) = slot {
                self.pages(p.page.colour)[p.page.index].last_used = generation;
            }
            return Ok(slot);
        }
        let slot = match text.rasterize(key) {
            GlyphRaster::Empty => Slot::Empty,
            GlyphRaster::Missing => Slot::Missing,
            GlyphRaster::Image(img) => self.place(gpu, &img, generation)?,
        };
        self.entries.insert(key, slot);
        Ok(slot)
    }

    fn place(&mut self, gpu: &Gpu, img: &GlyphImage, generation: u64) -> Result<Slot, RenderError> {
        let colour = match img.mask {
            GlyphMask::Alpha => false,
            GlyphMask::Color => true,
        };
        let bytes_per_px = if colour { 4 } else { 1 };
        if img.data.len() != (img.width * img.height * bytes_per_px) as usize {
            return Err(RenderError::Backend(format!(
                "rasterizer returned {} bytes for a {}x{} glyph",
                img.data.len(),
                img.width,
                img.height
            )));
        }
        if img.width == 0 || img.height == 0 {
            return Ok(Slot::Empty);
        }
        let (index, (x, y)) = self.find_room(gpu, colour, img.width, img.height)?;
        let page = &mut self.pages(colour)[index];
        page.last_used = generation;
        gpu.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &page.texture,
                mip_level: 0,
                origin: wgpu::Origin3d { x, y, z: 0 },
                aspect: wgpu::TextureAspect::All,
            },
            &img.data,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(img.width * bytes_per_px),
                rows_per_image: None,
            },
            wgpu::Extent3d {
                width: img.width,
                height: img.height,
                depth_or_array_layers: 1,
            },
        );
        let px = PAGE as f32;
        Ok(Slot::Placed(Placed {
            page: PageId { colour, index },
            uv: [
                x as f32 / px,
                y as f32 / px,
                (x + img.width) as f32 / px,
                (y + img.height) as f32 / px,
            ],
            width: img.width,
            height: img.height,
            left: img.left,
            top: img.top,
        }))
    }

    fn find_room(
        &mut self,
        gpu: &Gpu,
        colour: bool,
        w: u32,
        h: u32,
    ) -> Result<(usize, (u32, u32)), RenderError> {
        let too_big = || {
            RenderError::Backend(format!(
                "glyph {w}x{h} does not fit an atlas page ({PAGE}px)"
            ))
        };
        if let Some(found) = self
            .pages(colour)
            .iter_mut()
            .enumerate()
            .find_map(|(i, p)| p.packer.alloc(w, h).map(|at| (i, at)))
        {
            return Ok(found);
        }
        if self.pages(colour).len() < MAX_PAGES_PER_KIND {
            let pages = self.pages(colour);
            pages.push(Page::new(gpu, colour));
            let index = pages.len() - 1;
            return pages[index]
                .packer
                .alloc(w, h)
                .map(|at| (index, at))
                .ok_or_else(too_big);
        }
        // All pages full: reset the least recently used one if every encoder that sampled it has been submitted.
        let (index, oldest) = self
            .pages(colour)
            .iter()
            .enumerate()
            .map(|(i, p)| (i, p.last_used))
            .min_by_key(|(_, used)| *used)
            .ok_or_else(too_big)?;
        if oldest > self.submitted_through {
            return Err(RenderError::Backend(
                "glyph atlas full of pages used by unsubmitted encoders (call WgpuUi::frame_submitted after submit)"
                    .to_string(),
            ));
        }
        self.pages(colour)[index].packer.reset();
        self.entries.retain(|_, slot| match slot {
            Slot::Placed(p) => !(p.page.colour == colour && p.page.index == index),
            Slot::Empty | Slot::Missing => true,
        });
        self.pages(colour)[index]
            .packer
            .alloc(w, h)
            .map(|at| (index, at))
            .ok_or_else(too_big)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A page filled past capacity must refuse (so `find_room` opens a new page) and never hand out overlapping
    /// rects, padding included: overlap would make one glyph's pixels overwrite or bleed into another's.
    #[test]
    fn packer_refuses_when_full_and_never_overlaps() {
        let mut packer = Packer::new();
        let mut placed: Vec<(u32, u32, u32, u32)> = Vec::new();
        let sizes = [(13, 17), (30, 40), (7, 9), (64, 64), (21, 33)];
        let mut i = 0;
        while let Some((x, y)) = packer.alloc(sizes[i % 5].0, sizes[i % 5].1) {
            let (w, h) = sizes[i % 5];
            assert!(
                x + w + PAD <= PAGE && y + h + PAD <= PAGE,
                "rect leaves the page"
            );
            for &(px, py, pw, ph) in &placed {
                let apart = x + w + PAD <= px
                    || px + pw + PAD <= x
                    || y + h + PAD <= py
                    || py + ph + PAD <= y;
                assert!(apart, "({x},{y},{w},{h}) overlaps ({px},{py},{pw},{ph})");
            }
            placed.push((x, y, w, h));
            i += 1;
            assert!(i < 100_000, "packer never filled");
        }
        assert!(placed.len() > 100);
        packer.reset();
        assert!(packer.alloc(64, 64).is_some(), "reset must free the page");
    }
}
