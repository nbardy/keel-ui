//! Device handles, bind-group layouts and the render pipelines.
pub(crate) const QUAD_FLOATS: usize = 28;
pub(crate) const TEX_FLOATS: usize = 16;
pub(crate) const TESS_FLOATS: usize = 7;

pub(crate) struct Gpu {
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub format: wgpu::TextureFormat,
    pub view_layout: wgpu::BindGroupLayout,
    pub tex_layout: wgpu::BindGroupLayout,
    pub nearest: wgpu::Sampler,
}

impl Gpu {
    pub fn new(device: wgpu::Device, queue: wgpu::Queue, format: wgpu::TextureFormat) -> Self {
        let view_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("kit view"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let tex_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("kit texture"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let nearest = device.create_sampler(&wgpu::SamplerDescriptor {
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });
        Self {
            device,
            queue,
            format,
            view_layout,
            tex_layout,
            nearest,
        }
    }

    pub fn texture_bind_group(
        &self,
        view: &wgpu::TextureView,
        sampler: &wgpu::Sampler,
    ) -> wgpu::BindGroup {
        self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("kit texture"),
            layout: &self.tex_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(sampler),
                },
            ],
        })
    }
}

pub(crate) struct Pipelines {
    pub quad: wgpu::RenderPipeline,
    pub tess: wgpu::RenderPipeline,
    /// Glyph atlas pages and offscreen group composites.
    pub tex: wgpu::RenderPipeline,
}

const PREMULT_OVER: wgpu::BlendState = wgpu::BlendState {
    color: wgpu::BlendComponent {
        src_factor: wgpu::BlendFactor::One,
        dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
        operation: wgpu::BlendOperation::Add,
    },
    alpha: wgpu::BlendComponent {
        src_factor: wgpu::BlendFactor::One,
        dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
        operation: wgpu::BlendOperation::Add,
    },
};

fn vec4_attrs<const N: usize>() -> [wgpu::VertexAttribute; N] {
    std::array::from_fn(|i| wgpu::VertexAttribute {
        format: wgpu::VertexFormat::Float32x4,
        offset: (i * 16) as u64,
        shader_location: i as u32,
    })
}

struct Spec<'a> {
    label: &'a str,
    module: &'a wgpu::ShaderModule,
    layout: &'a wgpu::PipelineLayout,
    buffer: wgpu::VertexBufferLayout<'a>,
    topology: wgpu::PrimitiveTopology,
}

/// Premultiplied source-over into the target; the UI passes carry no depth or stencil.
fn pipeline(gpu: &Gpu, s: Spec<'_>) -> wgpu::RenderPipeline {
    gpu.device
        .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some(s.label),
            layout: Some(s.layout),
            vertex: wgpu::VertexState {
                module: s.module,
                entry_point: Some("vs"),
                compilation_options: Default::default(),
                buffers: &[Some(s.buffer)],
            },
            fragment: Some(wgpu::FragmentState {
                module: s.module,
                entry_point: Some("fs"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: gpu.format,
                    blend: Some(PREMULT_OVER),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                topology: s.topology,
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: Default::default(),
            multiview_mask: None,
            cache: None,
        })
}

impl Pipelines {
    pub fn new(gpu: &Gpu) -> Self {
        let module = |label, src: &str| {
            gpu.device
                .create_shader_module(wgpu::ShaderModuleDescriptor {
                    label: Some(label),
                    source: wgpu::ShaderSource::Wgsl(src.into()),
                })
        };
        let quad_module = module("kit quad", include_str!("shaders/quad.wgsl"));
        let tess_module = module("kit tess", include_str!("shaders/tess.wgsl"));
        let tex_module = module("kit tex", include_str!("shaders/tex.wgsl"));
        let flat_layout = gpu
            .device
            .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("kit flat"),
                bind_group_layouts: &[Some(&gpu.view_layout)],
                immediate_size: 0,
            });
        let tex_pipeline_layout =
            gpu.device
                .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                    label: Some("kit textured"),
                    bind_group_layouts: &[Some(&gpu.view_layout), Some(&gpu.tex_layout)],
                    immediate_size: 0,
                });
        let quad_attrs = vec4_attrs::<{ QUAD_FLOATS / 4 }>();
        let tex_attrs = vec4_attrs::<{ TEX_FLOATS / 4 }>();
        let tess_attrs = [
            wgpu::VertexAttribute {
                format: wgpu::VertexFormat::Float32x2,
                offset: 0,
                shader_location: 0,
            },
            wgpu::VertexAttribute {
                format: wgpu::VertexFormat::Float32,
                offset: 8,
                shader_location: 1,
            },
            wgpu::VertexAttribute {
                format: wgpu::VertexFormat::Float32x4,
                offset: 12,
                shader_location: 2,
            },
        ];
        fn instance(
            stride: usize,
            attrs: &[wgpu::VertexAttribute],
        ) -> wgpu::VertexBufferLayout<'_> {
            wgpu::VertexBufferLayout {
                array_stride: (stride * 4) as u64,
                step_mode: wgpu::VertexStepMode::Instance,
                attributes: attrs,
            }
        }
        let tess_buffer = wgpu::VertexBufferLayout {
            array_stride: (TESS_FLOATS * 4) as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &tess_attrs,
        };
        Self {
            quad: pipeline(
                gpu,
                Spec {
                    label: "kit quad",
                    module: &quad_module,
                    layout: &flat_layout,
                    buffer: instance(QUAD_FLOATS, &quad_attrs),
                    topology: wgpu::PrimitiveTopology::TriangleStrip,
                },
            ),
            tess: pipeline(
                gpu,
                Spec {
                    label: "kit tess",
                    module: &tess_module,
                    layout: &flat_layout,
                    buffer: tess_buffer,
                    topology: wgpu::PrimitiveTopology::TriangleList,
                },
            ),
            tex: pipeline(
                gpu,
                Spec {
                    label: "kit tex",
                    module: &tex_module,
                    layout: &tex_pipeline_layout,
                    buffer: instance(TEX_FLOATS, &tex_attrs),
                    topology: wgpu::PrimitiveTopology::TriangleStrip,
                },
            ),
        }
    }
}
