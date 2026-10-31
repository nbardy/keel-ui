//! Lyon tessellation of `Fill`/`Stroke` shapes in device px, with egui-style feathered anti-aliasing and a
//! generation-evicted cache keyed by the exact shape bits.
//!
//! Strokes: kurbo turns the stroke (width, joins, caps, dashes) into an outline path, which lyon fills with the
//! non-zero rule. That keeps one tessellation path for both draw kinds.
//!
//! Feathering (egui `Tessellator` idea): every boundary edge of the triangle mesh gets a one-pixel fringe. The
//! boundary vertex moves half a pixel inward at full coverage and a twin moves half a pixel outward at zero
//! coverage; the rasteriser's interpolation of coverage is the anti-aliasing ramp. No MSAA.
use kit_core::Shape;
use kit_core::kurbo::{Affine, BezPath, PathEl, Stroke, StrokeOpts};
use kit_paint::RenderError;
use lyon::path::Path;
use lyon::tessellation::{
    BuffersBuilder, FillOptions, FillRule, FillTessellator, FillVertex, VertexBuffers,
};
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

const FRINGE: f32 = 0.5;
const MITER_CLAMP: f32 = 3.0;
const TOLERANCE: f64 = 0.1;
/// Generations a cached mesh survives without being requested.
const KEEP_GENERATIONS: u64 = 120;

/// Device-px triangle mesh. Vertex = `[x, y, coverage]`.
#[derive(Debug)]
pub(crate) struct Mesh {
    pub verts: Vec<[f32; 3]>,
    pub indices: Vec<u32>,
    /// Sub-pixel strokes are widened to one pixel and dimmed by this factor; fills use 1.
    pub alpha_scale: f32,
}

#[derive(Clone, Eq, PartialEq, Hash)]
struct MeshKey(Vec<u64>);

struct KeyBuilder(Vec<u64>);
impl KeyBuilder {
    fn tag(&mut self, t: u64) {
        self.0.push(t);
    }
    fn f(&mut self, v: f64) {
        self.0.push(v.to_bits());
    }
    fn affine(&mut self, a: Affine) {
        for c in a.as_coeffs() {
            self.f(c);
        }
    }
    fn path(&mut self, path: &BezPath) {
        for el in path.elements() {
            match *el {
                PathEl::MoveTo(p) => {
                    self.tag(1);
                    self.f(p.x);
                    self.f(p.y);
                }
                PathEl::LineTo(p) => {
                    self.tag(2);
                    self.f(p.x);
                    self.f(p.y);
                }
                PathEl::QuadTo(a, b) => {
                    self.tag(3);
                    for p in [a, b] {
                        self.f(p.x);
                        self.f(p.y);
                    }
                }
                PathEl::CurveTo(a, b, c) => {
                    self.tag(4);
                    for p in [a, b, c] {
                        self.f(p.x);
                        self.f(p.y);
                    }
                }
                PathEl::ClosePath => self.tag(5),
            }
        }
    }
    fn shape(&mut self, shape: &Shape) {
        match shape {
            Shape::Rect(r) => {
                self.tag(10);
                for v in [r.x0, r.y0, r.x1, r.y1] {
                    self.f(v);
                }
            }
            Shape::RoundRect(r) => {
                self.tag(11);
                let rr = r.radii();
                let rect = r.rect();
                for v in [
                    rect.x0,
                    rect.y0,
                    rect.x1,
                    rect.y1,
                    rr.top_left,
                    rr.top_right,
                    rr.bottom_right,
                    rr.bottom_left,
                ] {
                    self.f(v);
                }
            }
            Shape::Ellipse(e) => {
                self.tag(12);
                let c = e.center();
                let r = e.radii();
                for v in [c.x, c.y, r.x, r.y, e.rotation()] {
                    self.f(v);
                }
            }
            Shape::Path(p) => {
                self.tag(13);
                self.path(p);
            }
        }
    }
    fn stroke(&mut self, s: &Stroke) {
        self.f(s.width);
        self.f(s.miter_limit);
        self.f(s.dash_offset);
        self.tag(s.join as u64);
        self.tag(s.start_cap as u64);
        self.tag(s.end_cap as u64);
        for d in &s.dash_pattern {
            self.f(*d);
        }
    }
}

fn shape_path(shape: &Shape, scale: f64) -> BezPath {
    shape.to_bez(TOLERANCE / scale)
}

fn lyon_path(path: &BezPath) -> Path {
    let mut b = Path::builder();
    let mut open = false;
    let pt = |p: kit_core::kurbo::Point| lyon::math::point(p.x as f32, p.y as f32);
    for el in path.elements() {
        match *el {
            PathEl::MoveTo(p) => {
                if open {
                    b.end(true);
                }
                b.begin(pt(p));
                open = true;
            }
            PathEl::LineTo(p) => {
                b.line_to(pt(p));
            }
            PathEl::QuadTo(c, p) => {
                b.quadratic_bezier_to(pt(c), pt(p));
            }
            PathEl::CurveTo(c1, c2, p) => {
                b.cubic_bezier_to(pt(c1), pt(c2), pt(p));
            }
            PathEl::ClosePath => {
                b.end(true);
                open = false;
            }
        }
    }
    if open {
        b.end(true);
    }
    b.build()
}

fn fill_mesh(tess: &mut FillTessellator, path: &BezPath) -> Result<Mesh, RenderError> {
    let mut buffers: VertexBuffers<[f32; 2], u32> = VertexBuffers::new();
    tess.tessellate_path(
        &lyon_path(path),
        &FillOptions::tolerance(TOLERANCE as f32).with_fill_rule(FillRule::NonZero),
        &mut BuffersBuilder::new(&mut buffers, |v: FillVertex| v.position().to_array()),
    )
    .map_err(|e| RenderError::Backend(format!("tessellation failed: {e:?}")))?;
    Ok(feather(&buffers.vertices, &buffers.indices))
}

struct BoundaryEdge {
    a: u32,
    b: u32,
    /// Unit outward normal.
    normal: [f32; 2],
}

fn boundary_edges(points: &[[f32; 2]], indices: &[u32]) -> Vec<BoundaryEdge> {
    // Edges as first seen, in triangle order, with a use count; `slot_of` only looks up, it is never iterated.
    // Regression (keel_gallery, 2026-11-01): this was a HashMap drained with `into_values()`. Std's per-map random
    // seed made the edge order, hence the float sum order of the per-vertex normals and the fringe triangle draw
    // order, differ between processes: one anti-aliased icon pixel flipped by 1/255 between otherwise identical
    // runs (`independent_runs_produce_identical_png_bytes`). Never iterate a HashMap on a path that builds meshes.
    let mut counted: Vec<(u32, BoundaryEdge)> = Vec::new();
    let mut slot_of: HashMap<(u32, u32), usize> = HashMap::new();
    for tri in indices.chunks_exact(3) {
        let [a, b, c] = [tri[0], tri[1], tri[2]];
        let (pa, pb, pc) = (points[a as usize], points[b as usize], points[c as usize]);
        let cross = (pb[0] - pa[0]) * (pc[1] - pa[1]) - (pb[1] - pa[1]) * (pc[0] - pa[0]);
        if cross == 0.0 {
            continue;
        }
        for (u, v) in [(a, b), (b, c), (c, a)] {
            let (pu, pv) = (points[u as usize], points[v as usize]);
            let (dx, dy) = (pv[0] - pu[0], pv[1] - pu[1]);
            let len = (dx * dx + dy * dy).sqrt();
            if len == 0.0 {
                continue;
            }
            // Counter-clockwise (cross > 0): interior is left of u->v, so outward is right.
            let normal = if cross > 0.0 {
                [dy / len, -dx / len]
            } else {
                [-dy / len, dx / len]
            };
            let key = (u.min(v), u.max(v));
            match slot_of.get(&key) {
                Some(&slot) => counted[slot].0 += 1,
                None => {
                    slot_of.insert(key, counted.len());
                    counted.push((1, BoundaryEdge { a: u, b: v, normal }));
                }
            }
        }
    }
    counted
        .into_iter()
        .filter(|(count, _)| *count == 1)
        .map(|(_, e)| e)
        .collect()
}

fn feather(points: &[[f32; 2]], indices: &[u32]) -> Mesh {
    let edges = boundary_edges(points, indices);
    // Accumulated edge normals and edge count per boundary vertex.
    // Ordered by vertex id: the outer vertices' indices and the sums above must not depend on hash seeds.
    let mut acc: BTreeMap<u32, ([f32; 2], f32)> = BTreeMap::new();
    for e in &edges {
        for v in [e.a, e.b] {
            let slot = acc.entry(v).or_insert(([0.0, 0.0], 0.0));
            slot.0[0] += e.normal[0];
            slot.0[1] += e.normal[1];
            slot.1 += 1.0;
        }
    }
    let mut verts: Vec<[f32; 3]> = points.iter().map(|p| [p[0], p[1], 1.0]).collect();
    let mut outer: HashMap<u32, u32> = HashMap::new();
    for (&v, &(n, count)) in &acc {
        let len2 = n[0] * n[0] + n[1] * n[1];
        // Miter offset: for two unit normals n/(1+n1.n2) == n * 2/|n|^2; clamped against needle corners.
        let (mut ox, mut oy) = if len2 > 1e-6 {
            (n[0] * count / len2, n[1] * count / len2)
        } else {
            (0.0, 0.0)
        };
        let len = (ox * ox + oy * oy).sqrt();
        if len > MITER_CLAMP {
            ox *= MITER_CLAMP / len;
            oy *= MITER_CLAMP / len;
        }
        let p = points[v as usize];
        verts[v as usize] = [p[0] - ox * FRINGE, p[1] - oy * FRINGE, 1.0];
        outer.insert(v, verts.len() as u32);
        verts.push([p[0] + ox * FRINGE, p[1] + oy * FRINGE, 0.0]);
    }
    let mut out_indices = indices.to_vec();
    for e in &edges {
        let (ia, ib) = (e.a, e.b);
        let (oa, ob) = (outer[&ia], outer[&ib]);
        out_indices.extend_from_slice(&[ia, ib, ob, ia, ob, oa]);
    }
    Mesh {
        verts,
        indices: out_indices,
        alpha_scale: 1.0,
    }
}

pub(crate) struct TessCache {
    tess: FillTessellator,
    meshes: HashMap<MeshKey, (Arc<Mesh>, u64)>,
}

impl TessCache {
    pub fn new() -> Self {
        Self {
            tess: FillTessellator::new(),
            meshes: HashMap::new(),
        }
    }

    pub fn evict(&mut self, generation: u64) {
        self.meshes
            .retain(|_, (_, used)| used.saturating_add(KEEP_GENERATIONS) >= generation);
    }

    pub fn len(&self) -> usize {
        self.meshes.len()
    }

    /// `to_device` maps the shape's own coordinates to device px (dpr scale composed with the draw transform).
    pub fn fill(
        &mut self,
        shape: &Shape,
        to_device: Affine,
        scale: f64,
        generation: u64,
    ) -> Result<Arc<Mesh>, RenderError> {
        let mut key = KeyBuilder(vec![0]);
        key.affine(to_device);
        key.f(scale);
        key.shape(shape);
        let key = MeshKey(key.0);
        if let Some((mesh, used)) = self.meshes.get_mut(&key) {
            *used = generation;
            return Ok(mesh.clone());
        }
        let path = to_device * shape_path(shape, scale);
        let mesh = Arc::new(fill_mesh(&mut self.tess, &path)?);
        self.meshes.insert(key, (mesh.clone(), generation));
        Ok(mesh)
    }

    pub fn stroke(
        &mut self,
        shape: &Shape,
        stroke: &Stroke,
        to_device: Affine,
        scale: f64,
        generation: u64,
    ) -> Result<Arc<Mesh>, RenderError> {
        let mut key = KeyBuilder(vec![1]);
        key.affine(to_device);
        key.f(scale);
        key.stroke(stroke);
        key.shape(shape);
        let key = MeshKey(key.0);
        if let Some((mesh, used)) = self.meshes.get_mut(&key) {
            *used = generation;
            return Ok(mesh.clone());
        }
        let path = to_device * shape_path(shape, scale);
        // Stroke in device space: width and dashes scale with dpr; a sub-pixel line becomes one pixel, dimmed.
        let mut dev = stroke.clone();
        dev.width = stroke.width * scale;
        dev.dash_offset = stroke.dash_offset * scale;
        for d in &mut dev.dash_pattern {
            *d *= scale;
        }
        let alpha_scale = dev.width.min(1.0) as f32;
        dev.width = dev.width.max(1.0);
        let outline = kit_core::kurbo::stroke(path.iter(), &dev, &StrokeOpts::default(), TOLERANCE);
        let mut mesh = fill_mesh(&mut self.tess, &outline)?;
        mesh.alpha_scale = alpha_scale;
        let mesh = Arc::new(mesh);
        self.meshes.insert(key, (mesh.clone(), generation));
        Ok(mesh)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kit_core::kurbo::{Circle, Shape as _};

    /// Regression (keel_gallery, 2026-11-01): feathering iterated std HashMaps, whose random seeds differ per map,
    /// so the same path produced differently ordered meshes (and float sums) on every call. Each `HashMap::new()`
    /// draws a fresh seed, so repeated calls in ONE process expose it without needing separate processes.
    #[test]
    fn feathering_is_bit_identical_across_calls() {
        let mut path = Circle::new((40.0, 30.0), 17.3).to_path(0.1);
        path.extend(Circle::new((60.0, 33.0), 9.1).to_path(0.1));
        let mut tess = FillTessellator::new();
        let key = |mesh: &Mesh| {
            (
                mesh.verts
                    .iter()
                    .flatten()
                    .map(|v| v.to_bits())
                    .collect::<Vec<_>>(),
                mesh.indices.clone(),
            )
        };
        let first = key(&fill_mesh(&mut tess, &path).expect("tessellates"));
        for _ in 0..16 {
            assert!(key(&fill_mesh(&mut tess, &path).expect("tessellates")) == first);
        }
    }
}
