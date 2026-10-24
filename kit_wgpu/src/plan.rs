//! DrawList -> per-pass, per-clip-section batches.
//!
//! Items keep draw order wherever it is visible: inside one clip section an item may join an earlier batch of the
//! same kind only if no batch it hops over overlaps it. A typical HUD (panels, text, icons) therefore collapses to
//! one batch per kind regardless of interleaving.
//!
//! Layer model (see crate docs): a lone item's opacity is multiplied into its colour (`effective_opacities`); rect
//! clips are scissors; a translucent layer with two or more drawing items renders into an offscreen texture and is
//! composited back once.
use crate::atlas::{Atlas, PageId, Slot};
use crate::color::Output;
use crate::geom::{DevRect, rects_overlap};
use crate::gpu::{Gpu, QUAD_FLOATS, TESS_FLOATS, TEX_FLOATS};
use crate::tess::{Mesh, TessCache};
use kit_core::kurbo::{Affine, Point, Rect, RoundedRectRadii};
use kit_core::peniko::{Brush, Color};
use kit_core::{Border, Clip, Shadow};
use kit_paint::{Draw, DrawList, RenderError, TOFU_STROKE, caret_bar, em, layout_glyphs, tofu_box};
use kit_view::{CosmicText, TextLayoutId};

#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
pub(crate) enum Source {
    Glyph(PageId),
    /// Offscreen group `depth` (1-based), composited source-over.
    Group(usize),
}

pub(crate) enum Item {
    Quad([f32; QUAD_FLOATS]),
    Tess {
        verts: Vec<[f32; TESS_FLOATS]>,
        indices: Vec<u32>,
    },
    Tex(Source, [f32; TEX_FLOATS]),
}

pub(crate) enum Data {
    Quads(Vec<[f32; QUAD_FLOATS]>),
    Tess {
        verts: Vec<[f32; TESS_FLOATS]>,
        indices: Vec<u32>,
    },
    Tex(Source, Vec<[f32; TEX_FLOATS]>),
}

pub(crate) struct Batch {
    pub data: Data,
    pub bounds: Vec<Rect>,
    union: Rect,
}

impl Batch {
    fn accepts(&self, item: &Item) -> bool {
        match (&self.data, item) {
            (Data::Quads(_), Item::Quad(_)) | (Data::Tess { .. }, Item::Tess { .. }) => true,
            (Data::Tex(a, _), Item::Tex(b, _)) => a == b,
            (Data::Quads(_) | Data::Tess { .. } | Data::Tex(..), _) => false,
        }
    }
    fn overlaps(&self, r: Rect) -> bool {
        rects_overlap(self.union, r) && self.bounds.iter().any(|b| rects_overlap(*b, r))
    }
    fn push(&mut self, item: Item, bounds: Rect) {
        match (&mut self.data, item) {
            (Data::Quads(v), Item::Quad(q)) => v.push(q),
            (
                Data::Tess { verts, indices },
                Item::Tess {
                    verts: nv,
                    indices: ni,
                },
            ) => {
                let base = verts.len() as u32;
                verts.extend(nv);
                indices.extend(ni.into_iter().map(|i| i + base));
            }
            (Data::Tex(_, v), Item::Tex(_, t)) => v.push(t),
            // `accepts` is checked by the only caller; a mismatch is a planner bug.
            (Data::Quads(_) | Data::Tess { .. } | Data::Tex(..), _) => {
                unreachable!("batch/item kind mismatch")
            }
        }
        self.union = self.union.union(bounds);
        self.bounds.push(bounds);
    }
    fn new(item: Item, bounds: Rect) -> Self {
        let data = match item {
            Item::Quad(q) => Data::Quads(vec![q]),
            Item::Tess { verts, indices } => Data::Tess { verts, indices },
            Item::Tex(s, t) => Data::Tex(s, vec![t]),
        };
        Self {
            data,
            bounds: vec![bounds],
            union: bounds,
        }
    }
}

/// A run of items sharing one scissor.
pub(crate) struct Section {
    pub scissor: DevRect,
    pub batches: Vec<Batch>,
}

impl Section {
    fn add(&mut self, item: Item, bounds: Rect) {
        let mut join = None;
        for (i, b) in self.batches.iter().enumerate().rev() {
            let hit = b.overlaps(bounds);
            if b.accepts(&item) {
                join = Some(i);
            }
            if hit {
                break;
            }
        }
        match join {
            Some(i) => self.batches[i].push(item, bounds),
            None => self.batches.push(Batch::new(item, bounds)),
        }
    }
}

#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub(crate) enum PassTarget {
    Main,
    /// Offscreen group at nesting depth (1-based).
    Group(usize),
}

pub(crate) struct PassPlan {
    pub target: PassTarget,
    /// First pass over this target in the frame: clears the colour of an offscreen group.
    pub first: bool,
    pub sections: Vec<Section>,
}

enum ClipChange {
    Same,
    Scissor(DevRect),
}

struct Builder {
    target: PassTarget,
    first: bool,
    sections: Vec<Section>,
    scissor: DevRect,
    /// The scissor outside each open layer, innermost last.
    outer: Vec<DevRect>,
}

impl Builder {
    fn new(target: PassTarget, size: (u32, u32)) -> Self {
        let mut b = Self {
            target,
            first: true,
            sections: Vec::new(),
            scissor: DevRect::full(size.0, size.1),
            outer: Vec::new(),
        };
        b.open();
        b
    }
    fn open(&mut self) {
        self.sections.push(Section {
            scissor: self.scissor,
            batches: Vec::new(),
        });
    }
    fn enter(&mut self, change: ClipChange) {
        let outer = self.scissor;
        self.outer.push(outer);
        match change {
            ClipChange::Same => {}
            ClipChange::Scissor(r) => {
                self.scissor = outer.intersect(r);
                self.open();
            }
        }
    }
    fn leave(&mut self) -> Result<(), RenderError> {
        let outer = self.outer.pop().ok_or(RenderError::UnbalancedLayers)?;
        let inner = std::mem::replace(&mut self.scissor, outer);
        if inner != outer {
            self.open();
        }
        Ok(())
    }
    /// Closes the pass recorded so far; the builder continues the same target (and clip state) as a loading pass.
    fn cut(&mut self) -> PassPlan {
        let done = PassPlan {
            target: self.target,
            first: self.first,
            sections: std::mem::take(&mut self.sections),
        };
        self.first = false;
        self.open();
        done
    }
    fn section(&mut self) -> &mut Section {
        // `new`/`open` always leave at least one section.
        self.sections.last_mut().expect("builder has a section")
    }
}

/// Per list item: is it a `PushLayer` that needs an offscreen group for correct group opacity? Opacity below one with
/// two or more drawing items inside (at any depth) means children can overlap, and multiplying alpha per item would
/// let the overlap show through; a lone item cannot overlap itself, so it keeps the cheap per-item multiply.
fn translucent_groups(list: &DrawList) -> Vec<bool> {
    let mut needs = vec![false; list.items.len()];
    // (index of the open PushLayer, its opacity, drawing items seen inside it)
    let mut open: Vec<(usize, f32, usize)> = Vec::new();
    for (i, item) in list.items.iter().enumerate() {
        match item {
            Draw::PushLayer { opacity, .. } => open.push((i, *opacity, 0)),
            Draw::PopLayer => {
                if let Some((at, opacity, drawn)) = open.pop() {
                    needs[at] = opacity < 1.0 && drawn >= 2;
                    if let Some(parent) = open.last_mut() {
                        parent.2 += drawn;
                    }
                }
            }
            Draw::Fill { .. }
            | Draw::Stroke { .. }
            | Draw::RoundRect { .. }
            | Draw::Glyphs { .. }
            | Draw::Caret { .. } => {
                if let Some(top) = open.last_mut() {
                    top.2 += 1;
                }
            }
        }
    }
    needs
}

/// The game paints solid colours only; any other brush is a typed error, never a substitute colour.
fn solid(brush: &Brush) -> Result<Color, RenderError> {
    match brush {
        Brush::Solid(c) => Ok(*c),
        Brush::Gradient(_) | Brush::Image(_) => Err(RenderError::UnsupportedBrush),
    }
}

fn scaled(r: Rect, s: f64) -> Rect {
    Rect::new(r.x0 * s, r.y0 * s, r.x1 * s, r.y1 * s)
}

fn mesh_bounds(mesh: &Mesh) -> Rect {
    let mut r = Rect::new(f64::MAX, f64::MAX, f64::MIN, f64::MIN);
    for v in &mesh.verts {
        r.x0 = r.x0.min(f64::from(v[0]));
        r.y0 = r.y0.min(f64::from(v[1]));
        r.x1 = r.x1.max(f64::from(v[0]));
        r.y1 = r.y1.max(f64::from(v[1]));
    }
    r
}

/// Pre-resolved description of one rounded-rect quad, all in device px / premultiplied colour.
struct QuadSpec {
    rect: Rect,
    radii: [f32; 4],
    fill: [f32; 4],
    border_width: f32,
    border: [f32; 4],
    shadow: [f32; 4],
    shadow_blur: f32,
    shadow_spread: f32,
    shadow_offset: [f32; 2],
}

impl QuadSpec {
    fn plain(rect: Rect, fill: [f32; 4]) -> Self {
        Self {
            rect,
            radii: [0.0; 4],
            fill,
            border_width: 0.0,
            border: [0.0; 4],
            shadow: [0.0; 4],
            shadow_blur: 0.0,
            shadow_spread: 0.0,
            shadow_offset: [0.0; 2],
        }
    }
    fn item(&self) -> (Item, Rect) {
        let r = self.rect;
        let mut f = [0.0f32; QUAD_FLOATS];
        f[0..4].copy_from_slice(&[
            r.x0 as f32,
            r.y0 as f32,
            r.width() as f32,
            r.height() as f32,
        ]);
        f[4..8].copy_from_slice(&self.fill);
        f[8..12].copy_from_slice(&self.border);
        f[12..16].copy_from_slice(&self.shadow);
        f[16..20].copy_from_slice(&self.radii);
        f[20..24].copy_from_slice(&[self.border_width, self.shadow_blur, self.shadow_spread, 0.0]);
        f[24..26].copy_from_slice(&self.shadow_offset);
        let reach = f64::from(self.shadow_blur + self.shadow_spread);
        let off = Rect::new(
            r.x0 + f64::from(self.shadow_offset[0]) - reach,
            r.y0 + f64::from(self.shadow_offset[1]) - reach,
            r.x1 + f64::from(self.shadow_offset[0]) + reach,
            r.y1 + f64::from(self.shadow_offset[1]) + reach,
        );
        (Item::Quad(f), r.union(off).inflate(1.0, 1.0))
    }
}

fn tex_inst(rect: Rect, uv: [f32; 4], tint: [f32; 4], kind: f32) -> [f32; TEX_FLOATS] {
    let mut f = [0.0f32; TEX_FLOATS];
    f[0..4].copy_from_slice(&[
        rect.x0 as f32,
        rect.y0 as f32,
        rect.width() as f32,
        rect.height() as f32,
    ]);
    f[4..8].copy_from_slice(&uv);
    f[8..12].copy_from_slice(&tint);
    f[12] = kind;
    f
}

const KIND_MASK: f32 = 0.0;
const KIND_PREMUL: f32 = 1.0;

enum Frame {
    Plain,
    Group {
        parent: Builder,
        change: ClipChange,
        parent_div: f32,
        base: f32,
        depth: usize,
    },
}

pub(crate) struct Plan {
    pub passes: Vec<PassPlan>,
    /// Deepest offscreen group nesting used.
    pub groups: usize,
}

/// Everything the planner reads from the renderer.
pub(crate) struct PlanEnv<'a> {
    pub gpu: &'a Gpu,
    pub tess: &'a mut TessCache,
    pub atlas: &'a mut Atlas,
    pub text: &'a mut CosmicText,
    pub scale: f64,
    pub size: (u32, u32),
    pub out: Output,
    pub generation: u64,
}

pub(crate) struct Planner<'a> {
    gpu: &'a Gpu,
    tess: &'a mut TessCache,
    atlas: &'a mut Atlas,
    text: &'a mut CosmicText,
    scale: f64,
    size: (u32, u32),
    out: Output,
    generation: u64,
    passes: Vec<PassPlan>,
    cur: Builder,
    frames: Vec<Frame>,
    /// Cumulative opacity of the innermost offscreen group: items inside it are drawn relative to it.
    div: f32,
    groups_open: usize,
    max_group: usize,
}

impl<'a> Planner<'a> {
    pub fn new(env: PlanEnv<'a>) -> Self {
        Self {
            gpu: env.gpu,
            tess: env.tess,
            atlas: env.atlas,
            text: env.text,
            scale: env.scale,
            size: env.size,
            out: env.out,
            generation: env.generation,
            passes: Vec::new(),
            cur: Builder::new(PassTarget::Main, env.size),
            frames: Vec::new(),
            div: 1.0,
            groups_open: 0,
            max_group: 0,
        }
    }

    fn mul(&self, effective: f32) -> f32 {
        if self.div > 0.0 {
            effective / self.div
        } else {
            0.0
        }
    }

    fn add(&mut self, item: Item, bounds: Rect) {
        let sec = self.cur.section();
        if sec.scissor.is_empty() || !rects_overlap(bounds, sec.scissor.as_rect()) {
            return;
        }
        sec.add(item, bounds);
    }

    pub fn plan(mut self, list: &DrawList) -> Result<Plan, RenderError> {
        let effective = list.effective_opacities();
        let groups = translucent_groups(list);
        for ((item, &eff), &group) in list.items.iter().zip(&effective).zip(&groups) {
            self.item(item, eff, group)?;
        }
        let last = self.cur.cut();
        self.passes.push(last);
        Ok(Plan {
            passes: self.passes,
            groups: self.max_group,
        })
    }
    fn item(&mut self, item: &Draw, eff: f32, group: bool) -> Result<(), RenderError> {
        let m = self.mul(eff);
        match item {
            Draw::PushLayer { opacity, clip } => self.push_layer(eff * opacity, clip, group),
            Draw::PopLayer => self.pop_layer(),
            _ if m <= 0.0 => Ok(()),
            Draw::Fill {
                shape,
                brush,
                transform,
            } => {
                let color = solid(brush)?;
                let to_dev = Affine::scale(self.scale) * *transform;
                let mesh = self.tess.fill(shape, to_dev, self.scale, self.generation)?;
                self.mesh_item(&mesh, color, m);
                Ok(())
            }
            Draw::Stroke {
                shape,
                stroke,
                brush,
                transform,
            } => {
                let color = solid(brush)?;
                let to_dev = Affine::scale(self.scale) * *transform;
                let mesh = self
                    .tess
                    .stroke(shape, stroke, to_dev, self.scale, self.generation)?;
                self.mesh_item(&mesh, color, m);
                Ok(())
            }
            Draw::RoundRect {
                rect,
                radii,
                fill,
                border,
                shadow,
            } => self.round_rect(*rect, *radii, fill, border, shadow, m),
            Draw::Glyphs {
                layout,
                origin,
                brush,
            } => self.glyphs(*layout, *origin, brush, m),
            Draw::Caret { rect, brush, width } => self.caret(*rect, brush, *width, m),
        }
    }

    fn clip_change(&self, clip: &Clip) -> ClipChange {
        match clip {
            Clip::Unclipped => ClipChange::Same,
            Clip::ToRect(r) => ClipChange::Scissor(DevRect::rounded(scaled(*r, self.scale))),
        }
    }

    fn push_layer(&mut self, base: f32, clip: &Clip, group: bool) -> Result<(), RenderError> {
        let change = self.clip_change(clip);
        match group {
            false => {
                self.cur.enter(change);
                self.frames.push(Frame::Plain);
            }
            true => {
                let depth = self.groups_open + 1;
                self.groups_open = depth;
                self.max_group = self.max_group.max(depth);
                let mut parent = std::mem::replace(
                    &mut self.cur,
                    Builder::new(PassTarget::Group(depth), self.size),
                );
                self.passes.push(parent.cut());
                self.frames.push(Frame::Group {
                    parent,
                    change,
                    parent_div: self.div,
                    base,
                    depth,
                });
                self.div = base;
            }
        }
        Ok(())
    }

    fn pop_layer(&mut self) -> Result<(), RenderError> {
        match self.frames.pop().ok_or(RenderError::UnbalancedLayers)? {
            Frame::Plain => self.cur.leave(),
            Frame::Group {
                parent,
                change,
                parent_div,
                base,
                depth,
            } => {
                let mut done = std::mem::replace(&mut self.cur, parent);
                self.passes.push(done.cut());
                self.groups_open -= 1;
                self.div = parent_div;
                self.cur.enter(change);
                let m = if parent_div > 0.0 {
                    base / parent_div
                } else {
                    0.0
                };
                if m > 0.0 {
                    let full = DevRect::full(self.size.0, self.size.1).as_rect();
                    let inst = tex_inst(full, [0.0, 0.0, 1.0, 1.0], [m; 4], KIND_PREMUL);
                    self.add(Item::Tex(Source::Group(depth), inst), full);
                }
                self.cur.leave()
            }
        }
    }

    fn mesh_item(&mut self, mesh: &Mesh, color: Color, m: f32) {
        let c = self.out.premul(color, m * mesh.alpha_scale);
        let verts = mesh
            .verts
            .iter()
            .map(|v| [v[0], v[1], v[2], c[0], c[1], c[2], c[3]])
            .collect();
        let indices = mesh.indices.clone();
        self.add(Item::Tess { verts, indices }, mesh_bounds(mesh));
    }

    fn round_rect(
        &mut self,
        rect: Rect,
        radii: RoundedRectRadii,
        fill: &Brush,
        border: &Border,
        shadow: &Shadow,
        m: f32,
    ) -> Result<(), RenderError> {
        let r = scaled(rect, self.scale);
        if r.width() <= 0.0 || r.height() <= 0.0 {
            return Ok(());
        }
        // One SDF quad draws shadow, fill and border together.
        let color = solid(fill)?;
        let q = self.quad(r, radii, self.out.premul(color, m), border, shadow, m);
        self.add_quad(q);
        Ok(())
    }

    /// SDF quad over device-px rect `r` (non-empty).
    fn quad(
        &self,
        r: Rect,
        radii: RoundedRectRadii,
        fill: [f32; 4],
        border: &Border,
        shadow: &Shadow,
        m: f32,
    ) -> QuadSpec {
        let s = self.scale;
        let cap = r.width().min(r.height()) / 2.0;
        let rad = |v: f64| (v * s).clamp(0.0, cap) as f32;
        let mut q = QuadSpec::plain(r, fill);
        q.radii = [
            rad(radii.top_left),
            rad(radii.top_right),
            rad(radii.bottom_right),
            rad(radii.bottom_left),
        ];
        match border {
            Border::None => {}
            Border::Solid { width, color } => {
                q.border_width = (*width * s) as f32;
                q.border = self.out.premul(*color, m);
            }
        }
        match shadow {
            Shadow::None => {}
            Shadow::Drop {
                offset,
                blur,
                spread,
                color,
            } => {
                q.shadow = self.out.premul(*color, m);
                q.shadow_blur = (*blur * s) as f32;
                q.shadow_spread = (*spread * s) as f32;
                q.shadow_offset = [(offset.x * s) as f32, (offset.y * s) as f32];
            }
        }
        q
    }

    fn add_quad(&mut self, q: QuadSpec) {
        let (item, bounds) = q.item();
        self.add(item, bounds);
    }

    fn glyphs(
        &mut self,
        layout: TextLayoutId,
        origin: Point,
        brush: &Brush,
        m: f32,
    ) -> Result<(), RenderError> {
        let placed = layout_glyphs(self.text, layout, origin, self.scale as f32, brush)?;
        for (at, ink) in placed {
            let color = self.out.premul(solid(&ink)?, m);
            match self
                .atlas
                .slot(self.gpu, &mut *self.text, at.cache_key, self.generation)?
            {
                Slot::Empty => {}
                Slot::Placed(pl) => {
                    let (left, top) = (f64::from(at.x + pl.left), f64::from(at.y + pl.top));
                    let rect = Rect::new(
                        left,
                        top,
                        left + f64::from(pl.width),
                        top + f64::from(pl.height),
                    );
                    let (tint, kind) = if pl.page.colour {
                        ([m; 4], KIND_PREMUL)
                    } else {
                        (color, KIND_MASK)
                    };
                    self.add(
                        Item::Tex(Source::Glyph(pl.page), tex_inst(rect, pl.uv, tint, kind)),
                        rect,
                    );
                }
                Slot::Missing => {
                    // Visible tofu: the shared hollow box, drawn as a border ring around its centre line.
                    let centre = tofu_box(f64::from(at.x), f64::from(at.y), em(&at));
                    let half = TOFU_STROKE / 2.0;
                    let mut q = QuadSpec::plain(centre.inflate(half, half), [0.0; 4]);
                    q.border = color;
                    q.border_width = TOFU_STROKE as f32;
                    self.add_quad(q);
                }
            }
        }
        Ok(())
    }

    fn caret(&mut self, at: Rect, brush: &Brush, width: f64, m: f32) -> Result<(), RenderError> {
        let rect = scaled(caret_bar(at, width, self.scale), self.scale);
        let q = QuadSpec::plain(rect, self.out.premul(solid(brush)?, m));
        self.add_quad(q);
        Ok(())
    }
}
