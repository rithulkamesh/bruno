// Bruno Orb — mood-driven motion: idle, loading, happy, rage

struct Uniforms {
    time: f32, width: f32, height: f32, y_position: f32,
    speed: f32, intensity: f32, motion: f32, pulse: f32,
    core_color: vec4<f32>,
    glow_color: vec4<f32>,
    pointer: vec2<f32>,
    presence: f32,
    anim_style: f32, // 0 idle, 1 loading, 2 happy, 3 rage
}
@group(0) @binding(0) var<uniform> u: Uniforms;

struct VOut { @builtin(position) pos: vec4<f32> }

@vertex
fn vs_main(@builtin(vertex_index) vi: u32) -> VOut {
    var pts = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -1.0), vec2<f32>(3.0, -1.0), vec2<f32>(-1.0, 3.0),
    );
    var o: VOut;
    o.pos = vec4<f32>(pts[vi], 0.0, 1.0);
    return o;
}

fn hash3(p: vec3<f32>) -> f32 {
    return fract(sin(dot(p, vec3<f32>(127.1, 311.7, 74.7))) * 43758.5453);
}
fn vnoise3(p: vec3<f32>) -> f32 {
    let i = floor(p); let f = fract(p);
    let u3 = f * f * (3.0 - 2.0 * f);
    return mix(
        mix(mix(hash3(i), hash3(i + vec3<f32>(1.0, 0.0, 0.0)), u3.x),
            mix(hash3(i + vec3<f32>(0.0, 1.0, 0.0)), hash3(i + vec3<f32>(1.0, 1.0, 0.0)), u3.x), u3.y),
        mix(mix(hash3(i + vec3<f32>(0.0, 0.0, 1.0)), hash3(i + vec3<f32>(1.0, 0.0, 1.0)), u3.x),
            mix(hash3(i + vec3<f32>(0.0, 1.0, 1.0)), hash3(i + vec3<f32>(1.0, 1.0, 1.0)), u3.x), u3.y),
        u3.z);
}
fn fbm3(p_in: vec3<f32>, oct: i32) -> f32 {
    var v = 0.0; var a = 0.5; var p = p_in;
    for (var i: i32 = 0; i < oct; i = i + 1) {
        v += a * vnoise3(p);
        p = p * 2.02 + vec3<f32>(1.7, 9.2, 3.4);
        a *= 0.5;
    }
    return v;
}

fn rot2(v: vec2<f32>, a: f32) -> vec2<f32> {
    let c = cos(a);
    let s = sin(a);
    return vec2<f32>(v.x * c - v.y * s, v.x * s + v.y * c);
}

fn style_w(style: f32, which: f32) -> f32 {
    return 1.0 - min(abs(style - which), 1.0);
}

@fragment
fn fs_main(in: VOut) -> @location(0) vec4<f32> {
    let presence = clamp(u.presence, 0.0, 1.0);
    let style = u.anim_style;
    let w_idle = style_w(style, 0.0);
    let w_load = style_w(style, 1.0);
    let w_happy = style_w(style, 2.0);
    let w_rage = style_w(style, 3.0);

    let t = u.time * u.speed;

    // --- per-mood motion ---
    let breathe_idle = 0.5 + 0.5 * sin(t * 0.62 * u.pulse);
    let breathe_load = 0.5 + 0.5 * sin(t * 2.4 * u.pulse);
    let bounce_happy = pow(max(sin(t * 1.35 * u.pulse), 0.0), 1.65);
    let breathe_happy = 0.5 + 0.5 * sin(t * 0.95 * u.pulse);
    let rage_flicker = 0.55 + 0.45 * sin(t * 9.5) * sin(t * 14.0 + 0.7);

    let breathe = breathe_idle * w_idle
        + breathe_load * w_load
        + mix(breathe_happy, 1.0, bounce_happy * 0.55) * w_happy
        + rage_flicker * w_rage;

    let sway_idle = sin(t * 0.18) * 0.35;
    let sway_load = sin(t * 0.55) * 0.2;
    let sway_happy = sin(t * 0.42) * cos(t * 0.27 + 0.9);
    let sway_rage = sin(t * 1.1) * 0.65 + sin(t * 2.3) * 0.35;
    let sway = sway_idle * w_idle + sway_load * w_load + sway_happy * w_happy + sway_rage * w_rage;

    let scr = in.pos.xy / vec2<f32>(u.width, u.height);
    let raw = scr * 2.0 - 1.0;
    var p   = vec2<f32>(raw.x * (u.width / u.height), -raw.y);

    let shake = vec2<f32>(
        sin(t * 19.0) + sin(t * 31.0 + 1.1),
        sin(t * 23.0 + 0.4) + sin(t * 37.0),
    ) * 0.0022 * u.motion * w_rage;
    p += shake;
    p.x += sway * 0.016 * u.motion;
    p.y += u.y_position;

    let R_base = 0.228 * mix(0.78, 1.0, presence);
    let R_bounce = 0.011 * bounce_happy * u.pulse * w_happy;
    let R_load = 0.004 * sin(t * 3.2 * u.pulse) * w_load;
    let R  = R_base + 0.005 * breathe * u.pulse + R_bounce + R_load;
    let d  = length(p);

    if d > R * 1.14 {
        return vec4<f32>(0.0);
    }

    let nd = d / R;
    let nz = sqrt(max(0.0, 1.0 - min(nd * nd, 1.0)));
    let n3 = vec3<f32>(p.x / max(R, 0.001), p.y / max(R, 0.001), nz);
    let fresnel = pow(clamp(1.0 - nz, 0.0, 1.0), 2.2);

    let L = normalize(vec3<f32>(
        0.42 + u.pointer.x * 0.14,
        0.58 + u.pointer.y * 0.12,
        0.70,
    ));
    let NdotL = max(dot(n3, L), 0.0);
    let wrap  = max(dot(n3, -L), 0.0) * 0.35;

    let ptr = vec3<f32>(u.pointer.x * 0.12, u.pointer.y * 0.12, 0.0);
    let drift_idle = vec3<f32>(t * 0.04, t * 0.035, t * 0.03) * u.motion;
    let drift_load = vec3<f32>(t * 0.22, t * 0.18, t * 0.14) * u.motion;
    let drift_happy = vec3<f32>(t * 0.10, t * 0.085, t * 0.07) * u.motion;
    let drift_rage = vec3<f32>(t * 0.28, t * 0.24, t * 0.20) * u.motion;
    let drift = drift_idle * w_idle + drift_load * w_load + drift_happy * w_happy + drift_rage * w_rage + ptr;

    let swirl_idle = t * 0.04 * u.motion;
    let swirl_load = t * 0.55 * u.motion;
    let swirl_happy = t * 0.14 * u.motion;
    let swirl_rage = t * 0.42 * u.motion;
    let swirl = swirl_idle * w_idle + swirl_load * w_load + swirl_happy * w_happy + swirl_rage * w_rage;

    let swirl_xy = rot2(n3.xy, swirl);
    let n_anim = vec3<f32>(swirl_xy.x, swirl_xy.y, n3.z);
    let n_a = fbm3(n_anim * 2.2 + drift, 4);
    let n_b = fbm3(n_anim * 3.6 + drift * 1.2 + vec3<f32>(4.0, 1.5, 2.8), 3);

    // Loading: rotating segment sweep + ascending data bands
    let angle = atan2(p.y, p.x);
    let spin = t * 2.6;
    let segments = 5.0;
    let spinner = smoothstep(0.48, 0.92, sin(angle * segments - spin * segments));
    let scan_arc = smoothstep(0.15, 0.85, fract(angle / 6.2831853 + spin * 0.38));
    let stream = smoothstep(0.38, 0.62, sin(nd * 14.0 - t * 4.2 + n_a * 2.0));
    let load_fx = (spinner * 0.55 + scan_arc * 0.45) * (0.65 + stream * 0.35);

    let caustic = smoothstep(0.34, 0.66, n_a) * smoothstep(0.36, 0.64, n_b);
    let caustic_pulse_idle = 0.72 + 0.28 * sin(t * 1.1 * u.pulse + n_a * 6.28);
    let caustic_pulse_load = 0.55 + 0.45 * sin(t * 3.8 * u.pulse + n_a * 6.28);
    let caustic_pulse_happy = 0.68 + 0.32 * sin(t * 1.6 * u.pulse + n_a * 6.28);
    let caustic_pulse_rage = 0.45 + 0.55 * rage_flicker;
    let caustic_pulse = caustic_pulse_idle * w_idle
        + caustic_pulse_load * w_load
        + caustic_pulse_happy * w_happy
        + caustic_pulse_rage * w_rage;

    let depth = pow(max(1.0 - nd, 0.0), 1.3);
    let inner = pow(max(1.0 - nd, 0.0), 2.2) * (0.50 + 0.22 * breathe);

    let base = mix(u.core_color.xyz, u.glow_color.xyz, depth * 0.52 + 0.12);
    let matte = 0.84 + NdotL * 0.10 + wrap * 0.08;
    var emission = u.glow_color.xyz * (inner * 0.72 + caustic * caustic_pulse * 0.38);
    emission += u.glow_color.xyz * load_fx * 0.42 * w_load;

    let tint = (n_a - 0.5) * 0.07;
    var rgb = (base * matte + emission) * 1.18;
    rgb += vec3<f32>(tint * 0.25, tint * 0.12, tint * 0.4);
    rgb += u.glow_color.xyz * rage_flicker * 0.08 * w_rage * (1.0 - nd);

    let body_a = (1.0 - smoothstep(R * 0.70, R * 1.02, d))
               * mix(0.97, 0.38, fresnel);

    let dist_out = max(d - R, 0.0);
    let bloom = exp(-dist_out * 16.0) * smoothstep(R * 1.06, R * 0.88, d);
    let bloom_rgb = mix(u.glow_color.xyz, u.core_color.xyz, 0.28) * bloom;
    let bloom_a = bloom * u.intensity * 0.14;

    var a = (body_a + bloom_a * (1.0 - body_a)) * u.intensity * presence;
    a *= 1.0 - smoothstep(R * 1.02, R * 1.14, d);

    var rgb_out = rgb * body_a + bloom_rgb * bloom_a * (1.0 - body_a);

    let edge_lift = smoothstep(R * 0.92, R * 1.06, d);
    rgb_out = mix(rgb_out, u.glow_color.xyz * 0.7, edge_lift * (1.0 - a) * 0.35);

    if a < 0.004 { return vec4<f32>(0.0); }
    return vec4<f32>(rgb_out * a, a);
}
