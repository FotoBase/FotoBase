//! TASK-10-02-03: wgpu GPU pipeline for the per-pixel recipe ops
//! (docs/edit-recipe-spec-v1.md §2; PLAN §7 F2 item 3). The CPU engine in
//! `fotobase_core::apply` stays the golden reference — the shaders here
//! re-implement the same point math in WGSL, and the per-op tolerance is
//! asserted by `crates/render/tests/gpu.rs`.
//!
//! Scope: only per-pixel (point) ops run on the GPU. Geometry ops
//! (crop/straighten/keystone/…), neighborhood ops (sharpen, noise
//! reduction, soft focus), statistical ops (auto_color/auto_contrast) and
//! the native Lensfun op return `None` and stay on the CPU path — the same
//! [`fotobase_core::apply::OpBackend`] contract the Lensfun backend uses. `undo_all` is handled by
//! the recipe fold before backends are consulted. The strength-0 no-op rule
//! (bit-identical buffer, TASK-04-009/011) is honored by declining the op.

use fotobase_core::apply::OpBackend;
use fotobase_core::buffer::LinearImage;
use fotobase_core::recipe::Op;

/// WGSL for the single per-pixel kernel. One invocation per pixel; `op`
/// selects the math, `p0..p9` carry the op's scalar params and `points`
/// carries the curves control points (x,y pairs, sorted, `n` = pair count).
/// The WGSL mirrors `fotobase_core::apply` exactly; keep both in sync.
const SHADER: &str = r#"
struct Params {
    op      : u32,
    n       : u32,
    width   : u32,
    height  : u32,
    p0      : f32, p1 : f32, p2 : f32, p3 : f32, p4 : f32,
    p5      : f32, p6 : f32, p7 : f32, p8 : f32, p9 : f32,
    points  : array<f32, 64>, // up to 32 (x, y) control points
};

@group(0) @binding(0) var<storage, read> prm : Params;
@group(0) @binding(1) var<storage, read> src : array<f32>;
@group(0) @binding(2) var<storage, read_write> dst : array<f32>;

fn clamp01(v : f32) -> f32 { return clamp(v, 0.0, 1.0); }
fn lum(px : vec3<f32>) -> f32 {
    return 0.2126 * px.x + 0.7152 * px.y + 0.0722 * px.z;
}
fn lift(v : f32, a : f32) -> f32 {
    return clamp01(v + a * (1.0 - v) * (1.0 - v) * 0.5);
}
fn highlight_fn(v : f32, a : f32) -> f32 {
    let over = max(v - 0.5, 0.0) * 2.0;
    return clamp01(v - a * over * over * 0.35);
}
fn shadow_fn(v : f32, a : f32) -> f32 {
    let under = max(0.5 - v, 0.0) * 2.0;
    return clamp01(v + a * under * under * 0.35);
}
fn rgb_to_hsl(px : vec3<f32>) -> vec3<f32> {
    let mx = max(px.x, max(px.y, px.z));
    let mn = min(px.x, min(px.y, px.z));
    let l = (mx + mn) * 0.5;
    let d = mx - mn;
    if (d <= 1e-6) { return vec3<f32>(0.0, 0.0, l); }
    var h : f32;
    if (mx == px.x) {
        h = (px.y - px.z) / d;
    } else if (mx == px.y) {
        h = (px.z - px.x) / d + 2.0;
    } else {
        h = (px.x - px.y) / d + 4.0;
    }
    h = h / 6.0;
    if (h < 0.0) { h = h + 1.0; }
    let s = d / max(1.0 - abs(2.0 * l - 1.0), 1e-6);
    return vec3<f32>(h, clamp(s, 0.0, 1.0), l);
}
fn hsl_to_rgb(hsl : vec3<f32>) -> vec3<f32> {
    let h = hsl.x; let s = hsl.y; let l = hsl.z;
    if (s <= 1e-6) { return vec3<f32>(l, l, l); }
    let c = (1.0 - abs(2.0 * l - 1.0)) * s;
    let hp = h * 6.0;
    let x = c * (1.0 - abs(hp % 2.0 - 1.0));
    var rgb : vec3<f32>;
    if (hp < 1.0) { rgb = vec3<f32>(c, x, 0.0); }
    else if (hp < 2.0) { rgb = vec3<f32>(x, c, 0.0); }
    else if (hp < 3.0) { rgb = vec3<f32>(0.0, c, x); }
    else if (hp < 4.0) { rgb = vec3<f32>(0.0, x, c); }
    else if (hp < 5.0) { rgb = vec3<f32>(x, 0.0, c); }
    else { rgb = vec3<f32>(c, 0.0, x); }
    let m = l - c * 0.5;
    return clamp(rgb + vec3<f32>(m), vec3<f32>(0.0), vec3<f32>(1.0));
}
fn curve_eval(x : f32) -> f32 {
    // Mirrors `curves` in fotobase_core::apply: sorted points, linear
    // interpolation between neighbors, clamped outside the first/last x.
    let n = prm.n;
    if (n < 2u) { return x; }
    if (x <= prm.points[0]) { return prm.points[1]; }
    let last = n - 1u;
    if (x >= prm.points[last * 2u]) { return prm.points[last * 2u + 1u]; }
    var y = prm.points[1];
    var i = 0u;
    loop {
        if (i >= last) { break; }
        let x0 = prm.points[i * 2u];
        let y0 = prm.points[i * 2u + 1u];
        let x1 = prm.points[i * 2u + 2u];
        let y1 = prm.points[i * 2u + 3u];
        if (x <= x1) {
            let t = clamp((x - x0) / max(x1 - x0, 1e-9), 0.0, 1.0);
            y = y0 + (y1 - y0) * t;
            break;
        }
        i = i + 1u;
    }
    return y;
}

@compute @workgroup_size(16, 16)
fn main(@builtin(global_invocation_id) gid : vec3<u32>) {
    if (gid.x >= prm.width || gid.y >= prm.height) { return; }
    let idx = (gid.y * prm.width + gid.x) * 3u;
    let px = vec3<f32>(src[idx], src[idx + 1u], src[idx + 2u]);
    var out : vec3<f32>;

    switch prm.op {
        case 0u: { // exposure: v * 2^ev
            out = clamp(px * exp2(prm.p0), vec3<f32>(0.0), vec3<f32>(1.0));
        }
        case 1u: { // brightness
            let a = prm.p0;
            if (a >= 0.0) {
                out = clamp(px + a * (1.0 - px), vec3<f32>(0.0), vec3<f32>(1.0));
            } else {
                out = clamp(px * (1.0 + a), vec3<f32>(0.0), vec3<f32>(1.0));
            }
        }
        case 2u: { // fill_light: shadow lift with soft rolloff
            out = vec3<f32>(lift(px.x, prm.p0), lift(px.y, prm.p0), lift(px.z, prm.p0));
        }
        case 3u: { // temperature (kelvin_shift; warm/cool colors fold in)
            let s = prm.p0;
            out = vec3<f32>(
                clamp01(px.x + 0.25 * s),
                clamp01(px.y + 0.05 * s * (1.0 - abs(s))),
                clamp01(px.z - 0.25 * s));
        }
        case 4u: { // tint (green_magenta)
            let s = prm.p0;
            out = vec3<f32>(
                clamp01(px.x + 0.15 * s),
                clamp01(px.y - 0.15 * s),
                clamp01(px.z + 0.15 * s));
        }
        case 5u: { // tint_color: wash toward color, keep luminance
            let color = vec3<f32>(prm.p0, prm.p1, prm.p2);
            let a = prm.p3;
            let scale = lum(px) / max(lum(color), 1e-4);
            let goal = clamp(color * scale, vec3<f32>(0.0), vec3<f32>(1.0));
            out = clamp(px + (goal - px) * a, vec3<f32>(0.0), vec3<f32>(1.0));
        }
        case 6u: { // warmify = saturation(temperature(px, 0.5a), 0.25a)
            let a = prm.p0;
            let s = 0.5 * a;
            let t = vec3<f32>(
                clamp01(px.x + 0.25 * s),
                clamp01(px.y + 0.05 * s * (1.0 - abs(s))),
                clamp01(px.z - 0.25 * s));
            let l = vec3<f32>(lum(t));
            out = clamp(l + (t - l) * (1.0 + 0.25 * a), vec3<f32>(0.0), vec3<f32>(1.0));
        }
        case 7u: { // saturation
            let l = vec3<f32>(lum(px));
            out = clamp(l + (px - l) * (1.0 + prm.p0), vec3<f32>(0.0), vec3<f32>(1.0));
        }
        case 8u: { // vibrance: saturation weighted by (1 - chroma)
            let a = prm.p0;
            let l = vec3<f32>(lum(px));
            let mx = max(px.x, max(px.y, px.z));
            let mn = min(px.x, min(px.y, px.z));
            let chroma = clamp(mx - mn, 0.0, 1.0);
            out = clamp(l + (px - l) * (1.0 + a * (1.0 - chroma)), vec3<f32>(0.0), vec3<f32>(1.0));
        }
        case 9u: { // sepia
            let a = prm.p0;
            let l = lum(px);
            let goal = vec3<f32>(
                clamp01(l * 1.07 + 0.03), clamp01(l * 0.94 + 0.02), clamp01(l * 0.72));
            out = clamp(px * (1.0 - a) + goal * a, vec3<f32>(0.0), vec3<f32>(1.0));
        }
        case 10u: { // grayscale
            out = vec3<f32>(lum(px));
        }
        case 11u: { // bw: blend toward grayscale by amount
            let a = prm.p0;
            let l = vec3<f32>(lum(px));
            out = clamp(px * (1.0 - a) + l * a, vec3<f32>(0.0), vec3<f32>(1.0));
        }
        case 12u: { // highlight_recovery
            out = vec3<f32>(
                highlight_fn(px.x, prm.p0), highlight_fn(px.y, prm.p0), highlight_fn(px.z, prm.p0));
        }
        case 13u: { // shadow_recovery
            out = vec3<f32>(
                shadow_fn(px.x, prm.p0), shadow_fn(px.y, prm.p0), shadow_fn(px.z, prm.p0));
        }
        case 14u: { // curves
            out = clamp(vec3<f32>(curve_eval(px.x), curve_eval(px.y), curve_eval(px.z)),
                vec3<f32>(0.0), vec3<f32>(1.0));
        }
        case 15u: { // levels: black, span, 1/gamma
            let stretched = clamp((px - vec3<f32>(prm.p0)) / prm.p1, vec3<f32>(0.0), vec3<f32>(1.0));
            out = pow(stretched, vec3<f32>(prm.p2));
        }
        case 16u: { // hsl
            var hsl = rgb_to_hsl(px);
            hsl.x = (hsl.x + prm.p0) % 1.0;
            if (hsl.x < 0.0) { hsl.x = hsl.x + 1.0; }
            hsl.y = clamp(hsl.y * (1.0 + prm.p1), 0.0, 1.0);
            hsl.z = clamp(hsl.z + prm.p2, 0.0, 1.0);
            out = hsl_to_rgb(hsl);
        }
        case 17u: { // vignette (amount > 0; 0 is a CPU no-op before dispatch)
            let w = f32(prm.width);
            let h = f32(prm.height);
            let nx = ((f32(gid.x) + 0.5) / w - 0.5) * 2.0;
            let ny = ((f32(gid.y) + 0.5) / h - 0.5) * 2.0;
            let d = sqrt(nx * nx + ny * ny) / 1.4142135381698608;
            let t = clamp((d - 0.35) / 0.65, 0.0, 1.0);
            let strength = 0.8 * prm.p0;
            out = px * (1.0 - strength * t * t * (3.0 - 2.0 * t));
        }
        default: { out = px; }
    }
    dst[idx] = out.x;
    dst[idx + 1u] = out.y;
    dst[idx + 2u] = out.z;
}
"#;

// Op ids used in the shader's `switch`. Keep in sync with `SHADER`.
const OP_EXPOSURE: u32 = 0;
const OP_BRIGHTNESS: u32 = 1;
const OP_FILL_LIGHT: u32 = 2;
const OP_TEMPERATURE: u32 = 3;
const OP_TINT: u32 = 4;
const OP_TINT_COLOR: u32 = 5;
const OP_WARMIFY: u32 = 6;
const OP_SATURATION: u32 = 7;
const OP_VIBRANCE: u32 = 8;
const OP_SEPIA: u32 = 9;
const OP_GRAYSCALE: u32 = 10;
const OP_BW: u32 = 11;
const OP_HIGHLIGHT_RECOVERY: u32 = 12;
const OP_SHADOW_RECOVERY: u32 = 13;
const OP_CURVES: u32 = 14;
const OP_LEVELS: u32 = 15;
const OP_HSL: u32 = 16;
const OP_VIGNETTE: u32 = 17;

/// Max curve control points the params buffer can carry (2 f32 each).
const MAX_POINTS: usize = 32;

/// Scalar params + op id for one dispatch.
struct Params {
    op: u32,
    scalars: [f32; 10],
    points: Vec<f32>,
}

/// GPU execution of the per-pixel recipe ops. `None` from [`OpBackend::apply`]
/// means "not handled here" — the caller falls back to the CPU path.
pub struct GpuBackend {
    device: wgpu::Device,
    queue: wgpu::Queue,
    pipeline: wgpu::ComputePipeline,
    bind_layout: wgpu::BindGroupLayout,
}

impl GpuBackend {
    /// `None` when no GPU adapter is available (headless CI, no drivers).
    pub fn new() -> Option<Self> {
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::default());
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::LowPower,
            compatible_surface: None,
            force_fallback_adapter: false,
        }))
        .ok()?;
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("fotobase-recipe-gpu"),
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::default(),
            memory_hints: wgpu::MemoryHints::default(),
            trace: wgpu::Trace::Off,
        }))
        .ok()?;
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("recipe-ops"),
            source: wgpu::ShaderSource::Wgsl(std::borrow::Cow::Borrowed(SHADER)),
        });
        let bind_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("recipe-bindings"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: false },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("recipe-ops-pipeline"),
            layout: Some(&device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("recipe-ops-layout"),
                bind_group_layouts: &[&bind_layout],
                push_constant_ranges: &[],
            })),
            module: &module,
            entry_point: Some("main"),
            compilation_options: Default::default(),
            cache: None,
        });
        Some(Self { device, queue, pipeline, bind_layout })
    }

    /// Run one per-pixel op on the GPU. Returns `None` for ops the GPU does
    /// not implement (the caller must fall back to the CPU path).
    pub fn apply(&self, img: &LinearImage, op: &Op) -> Option<LinearImage> {
        let params = shader_op(op)?;
        self.dispatch(img, params)
    }

    fn dispatch(&self, img: &LinearImage, params: Params) -> Option<LinearImage> {
        if img.width == 0 || img.height == 0 {
            return None;
        }
        let expected_samples =
            (img.width as usize).checked_mul(img.height as usize)?.checked_mul(3)?;
        let expected_size = expected_samples.checked_mul(std::mem::size_of::<f32>())? as u64;
        let limits = self.device.limits();
        if expected_size > u64::from(limits.max_storage_buffer_binding_size)
            || expected_size > limits.max_buffer_size
        {
            return None;
        }
        if expected_samples != img.data.len() {
            return None;
        }
        // Params buffer must match the WGSL `Params` struct: 4 u32 header
        // words, 10 scalars, then the fixed-size f32 points array.
        let mut words: Vec<u8> = Vec::with_capacity((14 + MAX_POINTS * 2) * 4);
        for v in [params.op, (params.points.len() / 2) as u32, img.width, img.height] {
            words.extend_from_slice(&v.to_le_bytes());
        }
        for s in params.scalars {
            words.extend_from_slice(&s.to_le_bytes());
        }
        let mut pts = params.points.clone();
        pts.resize(MAX_POINTS * 2, 0.0);
        for v in pts {
            words.extend_from_slice(&v.to_le_bytes());
        }
        let params_buf = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("recipe-params"),
            size: words.len() as u64,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        self.queue.write_buffer(&params_buf, 0, &words);

        let mut src_bytes = Vec::with_capacity(img.data.len() * 4);
        for v in &img.data {
            src_bytes.extend_from_slice(&v.to_le_bytes());
        }
        let src = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("recipe-src"),
            size: src_bytes.len() as u64,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        self.queue.write_buffer(&src, 0, &src_bytes);
        let dst = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("recipe-dst"),
            size: src_bytes.len() as u64,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let bind = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("recipe-bind"),
            layout: &self.bind_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: params_buf.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: src.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 2, resource: dst.as_entire_binding() },
            ],
        });
        let work_x = img.width.div_ceil(16);
        let work_y = img.height.div_ceil(16);
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("recipe-enc") });
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("recipe-pass"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &bind, &[]);
            pass.dispatch_workgroups(work_x, work_y, 1);
        }
        let readback = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("recipe-readback"),
            size: dst.size(),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        encoder.copy_buffer_to_buffer(&dst, 0, &readback, 0, dst.size());
        let submission = self.queue.submit(Some(encoder.finish()));
        let slice = readback.slice(..);
        let (mapped_tx, mapped_rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |result| {
            let _ = mapped_tx.send(result);
        });
        self.device.poll(wgpu::PollType::wait_for(submission)).ok()?;
        mapped_rx.recv().ok()?.ok()?;
        let view = slice.get_mapped_range();
        let mut out = img.clone();
        for (o, chunk) in out.data.iter_mut().zip(view.as_chunks::<4>().0) {
            *o = f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
        }
        drop(view);
        readback.unmap();
        Some(out)
    }
}

fn fnum(op: &Op, key: &str, default: f64) -> f64 {
    op.params
        .as_ref()
        .and_then(|p| p.get(key))
        .and_then(serde_json::Value::as_f64)
        .unwrap_or(default)
}

impl OpBackend for GpuBackend {
    fn apply(&self, img: &LinearImage, op: &Op) -> Option<LinearImage> {
        GpuBackend::apply(self, img, op)
    }
}

/// Map a recipe op to its shader id + packed params, or `None` when the op
/// is not a per-pixel op (caller falls back to the CPU path).
fn shader_op(op: &Op) -> Option<Params> {
    use serde_json::Value;
    let scalars =
        |op_id: u32, p: [f32; 10]| Some(Params { op: op_id, scalars: p, points: Vec::new() });
    let one =
        |op_id: u32, p0: f32| scalars(op_id, [p0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]);
    let color = |op: &Op, key: &str| -> Option<[f32; 3]> {
        op.params
            .as_ref()
            .and_then(|p| p.get(key))
            .and_then(Value::as_str)
            .and_then(fotobase_core::apply::parse_hex_color)
    };
    match op.op_type.as_str() {
        "exposure" => one(OP_EXPOSURE, fnum(op, "ev", 0.0) as f32),
        "brightness" => one(OP_BRIGHTNESS, fnum(op, "amount", 0.0) as f32),
        "fill_light" => one(OP_FILL_LIGHT, fnum(op, "amount", 0.5) as f32),
        "temperature" => one(OP_TEMPERATURE, fnum(op, "kelvin_shift", 0.0) as f32),
        "warm_colors" => one(OP_TEMPERATURE, 0.6 * fnum(op, "amount", 0.0) as f32),
        "cool_colors" => one(OP_TEMPERATURE, -0.6 * fnum(op, "amount", 0.0) as f32),
        "tint" => match color(op, "color") {
            // Effects-tab wash; amount <= 0 is a CPU no-op — keep that
            // behavior on the GPU path too by declining the op.
            Some(c) => {
                let a = fnum(op, "amount", 0.5) as f32;
                if a <= 0.0 {
                    return None;
                }
                scalars(OP_TINT_COLOR, [c[0], c[1], c[2], a, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0])
            }
            None => one(OP_TINT, fnum(op, "green_magenta", 0.0) as f32),
        },
        "warmify" => {
            let a = fnum(op, "amount", 0.5) as f32;
            if a <= 0.0 {
                return None; // CPU no-op rule (bit-identical buffer)
            }
            one(OP_WARMIFY, a)
        }
        "saturation" => {
            let a = fnum(op, "amount", 0.0) as f32;
            if a == 0.0 {
                return None;
            }
            one(OP_SATURATION, a)
        }
        "vibrance" => one(OP_VIBRANCE, fnum(op, "amount", 0.0) as f32),
        "sepia" => one(OP_SEPIA, fnum(op, "amount", 1.0) as f32),
        "grayscale" => {
            let enabled = op
                .params
                .as_ref()
                .and_then(|p| p.get("enabled"))
                .and_then(Value::as_bool)
                .unwrap_or(true);
            if !enabled {
                return None;
            }
            one(OP_GRAYSCALE, 0.0)
        }
        "bw" => one(OP_BW, fnum(op, "amount", 1.0) as f32),
        "highlight_recovery" => one(OP_HIGHLIGHT_RECOVERY, fnum(op, "amount", 0.0) as f32),
        "shadow_recovery" => one(OP_SHADOW_RECOVERY, fnum(op, "amount", 0.0) as f32),
        "levels" => scalars(OP_LEVELS, levels_params(op)),
        "hsl" => scalars(
            OP_HSL,
            [
                fnum(op, "hue", 0.0) as f32,
                fnum(op, "saturation", 0.0) as f32,
                fnum(op, "luminance", 0.0) as f32,
                0.0,
                0.0,
                0.0,
                0.0,
                0.0,
                0.0,
                0.0,
            ],
        ),
        "vignette" => {
            let a = fnum(op, "amount", 0.5) as f32;
            if a <= 0.0 {
                return None;
            }
            one(OP_VIGNETTE, a)
        }
        "curves" => {
            let mut points: Vec<(f32, f32)> = op
                .params
                .as_ref()
                .and_then(|p| p.get("points"))
                .and_then(Value::as_array)
                .map(|items| {
                    items
                        .iter()
                        .filter_map(|item| {
                            let p = item.as_array()?;
                            Some((p.first()?.as_f64()? as f32, p.get(1)?.as_f64()? as f32))
                        })
                        .collect()
                })
                .unwrap_or_default();
            points.sort_by(|a, b| a.0.total_cmp(&b.0));
            if points.len() < 2 || points.len() > MAX_POINTS {
                return None; // degenerate → let the CPU path normalize it
            }
            Some(Params {
                op: OP_CURVES,
                scalars: [0.0; 10],
                points: points.iter().flat_map(|(x, y)| [*x, *y]).collect(),
            })
        }
        _ => None,
    }
}

fn levels_params(op: &Op) -> [f32; 10] {
    let black = fnum(op, "black", 0.0).clamp(0.0, 1.0) as f32;
    let white = fnum(op, "white", 1.0).clamp(0.0, 1.0) as f32;
    let gamma = fnum(op, "gamma", 1.0).clamp(0.1, 10.0) as f32;
    let span = (white - black).max(1e-4);
    [black, span, 1.0 / gamma, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]
}
