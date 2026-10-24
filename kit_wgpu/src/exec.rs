//! Lowering a `Plan` to one GPU buffer plus a flat command list, and recording it into render passes.
use crate::atlas::Atlas;
use crate::geom::DevRect;
use crate::gpu::{Gpu, Pipelines};
use crate::plan::{Data, PassPlan, PassTarget, Source};

/// Offscreen colour for one group nesting depth.
struct GroupTarget {
    color_view: wgpu::TextureView,
    bind_group: wgpu::BindGroup,
}

pub(crate) struct Attachments {
    size: (u32, u32),
    groups: Vec<GroupTarget>,
}

fn group_target(gpu: &Gpu, size: (u32, u32)) -> GroupTarget {
    let color = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("kit group"),
        size: wgpu::Extent3d {
            width: size.0,
            height: size.1,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: gpu.format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    let color_view = color.create_view(&Default::default());
    GroupTarget {
        bind_group: gpu.texture_bind_group(&color_view, &gpu.nearest),
        color_view,
    }
}

impl Attachments {
    pub fn new() -> Self {
        Self {
            size: (1, 1),
            groups: Vec::new(),
        }
    }
    /// Offscreen targets are the size of the main target; a resize rebuilds them.
    pub fn ensure(&mut self, gpu: &Gpu, size: (u32, u32), groups: usize) {
        if self.size != size {
            self.size = size;
            self.groups.clear();
        }
        while self.groups.len() < groups {
            self.groups.push(group_target(gpu, size));
        }
    }
}

pub(crate) enum Cmd {
    Quads {
        scissor: DevRect,
        at: u64,
        count: u32,
    },
    Tess {
        scissor: DevRect,
        verts: u64,
        indices: u64,
        count: u32,
    },
    Tex {
        scissor: DevRect,
        source: Source,
        at: u64,
        count: u32,
    },
}

pub(crate) struct PassCmds {
    pub target: PassTarget,
    pub first: bool,
    pub cmds: Vec<Cmd>,
}

#[derive(Default)]
struct Arena(Vec<u8>);
impl Arena {
    fn put_f32<const N: usize>(&mut self, rows: &[[f32; N]]) -> u64 {
        let at = self.0.len() as u64;
        for v in rows.iter().flatten() {
            self.0.extend_from_slice(&v.to_le_bytes());
        }
        at
    }
    fn put_u32(&mut self, vals: &[u32]) -> u64 {
        let at = self.0.len() as u64;
        for v in vals {
            self.0.extend_from_slice(&v.to_le_bytes());
        }
        at
    }
}

/// Plan -> (commands per pass, vertex/instance/index bytes). Sections with an empty scissor vanish entirely;
/// a pass with no commands is dropped unless it is the first pass over an offscreen group, which must still run to
/// clear it (the group is composited whether or not anything was drawn into it).
pub(crate) fn lower(passes: Vec<PassPlan>) -> (Vec<PassCmds>, Vec<u8>) {
    let mut arena = Arena::default();
    let mut out = Vec::new();
    for pass in passes {
        let mut cmds = Vec::new();
        for sec in pass.sections.iter().filter(|s| !s.scissor.is_empty()) {
            let scissor = sec.scissor;
            for batch in &sec.batches {
                cmds.push(match &batch.data {
                    Data::Quads(v) => Cmd::Quads {
                        scissor,
                        at: arena.put_f32(v),
                        count: v.len() as u32,
                    },
                    Data::Tess { verts, indices } => Cmd::Tess {
                        scissor,
                        verts: arena.put_f32(verts),
                        indices: arena.put_u32(indices),
                        count: indices.len() as u32,
                    },
                    Data::Tex(source, v) => Cmd::Tex {
                        scissor,
                        source: *source,
                        at: arena.put_f32(v),
                        count: v.len() as u32,
                    },
                });
            }
        }
        let must_clear = pass.first && pass.target != PassTarget::Main;
        if must_clear || !cmds.is_empty() {
            out.push(PassCmds {
                target: pass.target,
                first: pass.first,
                cmds,
            });
        }
    }
    (out, arena.0)
}

#[derive(Copy, Clone, Eq, PartialEq, Default, Debug)]
pub struct FrameStats {
    /// `draw`/`draw_indexed` calls the renderer issued (group composites included).
    pub draw_calls: u32,
    /// Render passes begun (1 unless a translucent group forces an offscreen pass).
    pub passes: u32,
}

#[derive(Copy, Clone, Eq, PartialEq)]
enum Pipe {
    Quad,
    Tess,
    Tex,
}

pub(crate) struct Resources<'a> {
    pub pipelines: &'a Pipelines,
    pub atlas: &'a Atlas,
    pub attachments: &'a Attachments,
    pub view_bind: &'a wgpu::BindGroup,
    pub buffer: &'a wgpu::Buffer,
    pub main_view: &'a wgpu::TextureView,
}

pub(crate) fn run(
    encoder: &mut wgpu::CommandEncoder,
    res: &Resources<'_>,
    passes: &[PassCmds],
) -> FrameStats {
    let mut stats = FrameStats::default();
    for p in passes {
        stats.passes += 1;
        let color_view = match p.target {
            PassTarget::Main => res.main_view,
            PassTarget::Group(d) => &res.attachments.groups[d - 1].color_view,
        };
        let color_load = match (p.target, p.first) {
            (PassTarget::Group(_), true) => wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
            (PassTarget::Main, _) | (PassTarget::Group(_), false) => wgpu::LoadOp::Load,
        };
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("kit ui"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: color_view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: color_load,
                    store: wgpu::StoreOp::Store,
                },
            })],
            ..Default::default()
        });
        pass.set_bind_group(0, res.view_bind, &[]);
        let mut current: Option<Pipe> = None;
        for cmd in &p.cmds {
            match cmd {
                Cmd::Quads { scissor, at, count } => {
                    use_pipe(&mut pass, &mut current, Pipe::Quad, &res.pipelines.quad);
                    set_scissor(&mut pass, *scissor);
                    pass.set_vertex_buffer(0, res.buffer.slice(*at..));
                    pass.draw(0..4, 0..*count);
                    stats.draw_calls += 1;
                }
                Cmd::Tess {
                    scissor,
                    verts,
                    indices,
                    count,
                } => {
                    use_pipe(&mut pass, &mut current, Pipe::Tess, &res.pipelines.tess);
                    set_scissor(&mut pass, *scissor);
                    pass.set_vertex_buffer(0, res.buffer.slice(*verts..));
                    pass.set_index_buffer(res.buffer.slice(*indices..), wgpu::IndexFormat::Uint32);
                    pass.draw_indexed(0..*count, 0, 0..1);
                    stats.draw_calls += 1;
                }
                Cmd::Tex {
                    scissor,
                    source,
                    at,
                    count,
                } => {
                    let bind = match *source {
                        Source::Glyph(page) => res.atlas.bind_group(page),
                        Source::Group(d) => &res.attachments.groups[d - 1].bind_group,
                    };
                    use_pipe(&mut pass, &mut current, Pipe::Tex, &res.pipelines.tex);
                    pass.set_bind_group(1, bind, &[]);
                    set_scissor(&mut pass, *scissor);
                    pass.set_vertex_buffer(0, res.buffer.slice(*at..));
                    pass.draw(0..4, 0..*count);
                    stats.draw_calls += 1;
                }
            }
        }
    }
    stats
}

fn use_pipe(
    pass: &mut wgpu::RenderPass<'_>,
    current: &mut Option<Pipe>,
    want: Pipe,
    pipeline: &wgpu::RenderPipeline,
) {
    if *current != Some(want) {
        pass.set_pipeline(pipeline);
        *current = Some(want);
    }
}

fn set_scissor(pass: &mut wgpu::RenderPass<'_>, r: DevRect) {
    let (x, y, w, h) = r.scissor();
    pass.set_scissor_rect(x, y, w, h);
}
