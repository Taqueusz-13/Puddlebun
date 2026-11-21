// ============================================================================
// rain_sim.wgsl — GPU raindrop physics.
//
// * Drop sizes follow the Marshall–Palmer distribution N(D) = N0·exp(-Λ·D).
//   Respawned drops are sampled flux-weighted (N(D)·v(D)) so that the
//   population *in the air* keeps the right size distribution.
// * Fall speed is the terminal velocity for that size (Atlas et al. 1973).
// * Horizontal velocity relaxes towards the local (gusting) wind with a
//   size-dependent drag time τ = v_t / g — big drops lag gusts, mist follows.
// * Impacts emit splashes, ripple impulses (puddles) and audio events.
// ============================================================================

struct Drop {
    pos: vec3f,
    size: f32,      // diameter (mm); 0 = needs spawning
    vel: vec3f,
    seed: u32,
    light: vec4f,   // rgb radiance from lamps this frame
}

struct SimParams {
    box_center: vec4f,  // xyz centre of the rain volume, w = half extent (m)
    box_y: vec4f,       // x bottom, y top, z shutter time (s), w dt
    dsd: vec4f,         // x Λ (1/mm), y Dmin, z Dmax, w max x (rain never goes past this)
    counts: vec4u,      // x active drops, y frame seed, z splash capacity, w audio capacity
    audio: vec4f,       // x audible radius, y min audible size, z splash radius, w ripple impulse scale
}

struct Splash {
    pos: vec3f,
    t0: f32,
    size: f32,
    kind: u32,
    seed: u32,
    speed: f32,
}

struct AudioEvent {
    pos: vec3f,
    size: f32,
    kind: u32,
    speed: f32,
    pad0: f32,
    pad1: f32,
}

struct Counters {
    splash_head: atomic<u32>,
    audio_count: atomic<u32>,
    impacts: atomic<u32>,
    puddle_impacts: atomic<u32>,
}

@group(1) @binding(0) var<uniform> sim: SimParams;
@group(1) @binding(1) var<storage, read_write> drops: array<Drop>;
@group(1) @binding(2) var<storage, read_write> splashes: array<Splash>;
@group(1) @binding(3) var<storage, read_write> counters: Counters;
@group(1) @binding(4) var<storage, read_write> audio_events: array<AudioEvent>;
@group(1) @binding(5) var<storage, read_write> ripple_imp: array<atomic<i32>>;

fn terminal_velocity(d_mm: f32) -> f32 {
    return max(9.65 - 10.3 * exp(-0.6 * d_mm), 0.3);
}

// Size sampled from the number distribution (used for the initial fill).
fn sample_size_volume(rng: ptr<function, u32>) -> f32 {
    let lam = sim.dsd.x;
    let dmin = sim.dsd.y;
    let dmax = sim.dsd.z;
    let span = 1.0 - exp(-lam * (dmax - dmin));
    return dmin - log(1.0 - rnd(rng) * span) / lam;
}

// Size sampled from the flux distribution N(D)·v(D) (rejection sampling).
fn sample_size_flux(rng: ptr<function, u32>) -> f32 {
    let vmax = terminal_velocity(sim.dsd.z);
    var d = sample_size_volume(rng);
    for (var k = 0; k < 8; k++) {
        if (rnd(rng) * vmax <= terminal_velocity(d)) {
            break;
        }
        d = sample_size_volume(rng);
    }
    return d;
}

fn ripple_stamp(p: vec2f, amp_um: f32) {
    let ts = frame.ripple.x;
    let res = i32(frame.ripple.y);
    let c = vec2i(floor(p / ts));
    for (var dy = -1; dy <= 1; dy++) {
        for (var dx = -1; dx <= 1; dx++) {
            let w = select(select(1.0, 2.0, dx == 0 || dy == 0), 4.0, dx == 0 && dy == 0) / 16.0;
            let x = (((c.x + dx) % res) + res) % res;
            let y = (((c.y + dy) % res) + res) % res;
            atomicAdd(&ripple_imp[u32(y * res + x)], i32(amp_um * w));
        }
    }
}

fn impact(hp: vec3f, kind: u32, d: Drop, speed: f32, rng: ptr<function, u32>) {
    if (kind == SURF_NONE) {
        return;
    }
    atomicAdd(&counters.impacts, 1u);
    let dist = distance(hp, frame.cam_pos.xyz);

    if (dist < sim.audio.z && d.size > 0.6) {
        let idx = atomicAdd(&counters.splash_head, 1u) % sim.counts.z;
        splashes[idx] = Splash(hp, frame.time.x - rnd(rng) * sim.box_y.w, d.size, kind, hash_u(*rng), speed);
    }
    if (dist < sim.audio.x && d.size > sim.audio.y) {
        let a = atomicAdd(&counters.audio_count, 1u);
        if (a < sim.counts.w) {
            audio_events[a] = AudioEvent(hp, d.size, kind, speed, 0.0, 0.0);
        }
    }
    if (kind == SURF_PUDDLE) {
        atomicAdd(&counters.puddle_impacts, 1u);
        // crater depth grows roughly with drop size^1.5 and impact speed
        let amp = sim.audio.w * pow(d.size, 1.5) * (speed / 7.0);
        ripple_stamp(hp.xz, amp);
    }
}

@compute @workgroup_size(256)
fn cs_update(@builtin(global_invocation_id) gid: vec3u) {
    let i = gid.x;
    if (i >= sim.counts.x) {
        return;
    }
    var d = drops[i];
    var rng = hash_uu(i, sim.counts.y);
    let dt = sim.box_y.w;
    let c = sim.box_center.xyz;
    let half = sim.box_center.w;

    if (d.size <= 0.0) {
        // first use: fill the whole volume
        d.size = sample_size_volume(&rng);
        d.pos = vec3f(
            c.x + (rnd(&rng) * 2.0 - 1.0) * half,
            mix(sim.box_y.x, sim.box_y.y, rnd(&rng)),
            c.z + (rnd(&rng) * 2.0 - 1.0) * half
        );
        let w0 = wind_at(d.pos);
        d.vel = vec3f(w0.x, -terminal_velocity(d.size), w0.z);
        d.seed = hash_u(rng);
    }

    let vt = terminal_velocity(d.size);
    let tau = max(vt / 9.81, 0.03);
    let w = wind_at(d.pos);
    let k = 1.0 - exp(-dt / tau);
    d.vel.x += (w.x - d.vel.x) * k;
    d.vel.z += (w.z - d.vel.z) * k;
    d.vel.y = -vt + w.y;

    let p1 = d.pos + d.vel * dt;
    let su = surface_under(p1, d.pos.y);
    let gh = su.x;
    var hit = p1.y <= gh;

    // drops blown into a building front stop there (no splash)
    let ax0 = abs(d.pos.x);
    let ax1 = abs(p1.x);
    let wx = wall_x();
    var wall_hit = false;
    if (ax0 < wx && ax1 >= wx && p1.y < building_top(p1)) {
        wall_hit = true;
    }
    // never rain inside the room (box limit on +x)
    if (p1.x > sim.dsd.w) {
        wall_hit = true;
    }

    if (hit || wall_hit) {
        if (hit) {
            impact(vec3f(p1.x, gh, p1.z), u32(su.y + 0.5), d, length(d.vel), &rng);
        }
        // respawn at the top of the volume
        d.size = sample_size_flux(&rng);
        d.pos = vec3f(
            min(c.x + (rnd(&rng) * 2.0 - 1.0) * half, sim.dsd.w - 0.01),
            sim.box_y.y - rnd(&rng) * 1.5,
            c.z + (rnd(&rng) * 2.0 - 1.0) * half
        );
        let w1 = wind_at(d.pos);
        d.vel = vec3f(w1.x, -terminal_velocity(d.size), w1.z);
    } else {
        d.pos = p1;
    }

    // keep the volume centred on the camera (wrap horizontally)
    let rel = d.pos.xz - c.xz;
    let span = 2.0 * half;
    let wrapped = rel - span * floor(rel / span + vec2f(0.5));
    d.pos.x = c.x + wrapped.x;
    d.pos.z = c.z + wrapped.y;
    if (d.pos.y > sim.box_y.y + 3.0) {
        d.pos.y = sim.box_y.y - rnd(&rng) * (sim.box_y.y - sim.box_y.x);
    }

    d.light = vec4f(drop_light(d.pos), 0.0);
    drops[i] = d;
}
