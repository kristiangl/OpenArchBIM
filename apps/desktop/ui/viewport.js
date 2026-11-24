// The 3D viewport: orbit camera, ground grid, the model's meshes, picking. Z-up world, metres.
// It draws with WebGPU where the webview has it (Windows, macOS) and with WebGL2 where it does not
// (Linux, whose webview is WebKitGTK, and older machines). Both backends draw the same thing from the
// same buffers; everything else here (camera, picking, edges, fitting) is shared.
// The stress mode draws 1200 apartment-sized boxes with one instanced draw call to check frame rate.
// Force a backend with localStorage "openarch.renderer" = "webgl2" or "webgpu", or ?renderer=webgl2.

const Viewport = (() => {
  const mat = {
    perspective(fovy, aspect, near, far) {
      const f = 1 / Math.tan(fovy / 2), m = new Float32Array(16);
      m[0] = f / aspect; m[5] = f; m[10] = far / (near - far); m[11] = -1; m[14] = (near * far) / (near - far);
      return m;
    },
    lookAt(e, t, up) {
      const sub = (a, b) => [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
      const norm = (a) => { const l = Math.hypot(...a) || 1; return [a[0] / l, a[1] / l, a[2] / l]; };
      const cross = (a, b) => [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]];
      const dot = (a, b) => a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
      const z = norm(sub(e, t)), x = norm(cross(up, z)), y = cross(z, x);
      return new Float32Array([x[0], y[0], z[0], 0, x[1], y[1], z[1], 0, x[2], y[2], z[2], 0, -dot(x, e), -dot(y, e), -dot(z, e), 1]);
    },
    mul(a, b) {
      const o = new Float32Array(16);
      for (let c = 0; c < 4; c++) for (let r = 0; r < 4; r++) {
        let s = 0; for (let k = 0; k < 4; k++) s += a[k * 4 + r] * b[c * 4 + k];
        o[c * 4 + r] = s;
      }
      return o;
    },
  };

  const SHADER = /* wgsl */ `
struct U { vp: mat4x4<f32>, light: vec4<f32>, bg: vec4<f32>, edge: vec4<f32>, lvp: mat4x4<f32>, sun: vec4<f32>, ao: vec4<f32>, proj: vec4<f32>, screen: vec4<f32>, eye: vec4<f32> };
@group(0) @binding(0) var<uniform> u: U;
// the sun's depth map (cast shadows): u.lvp takes a world point into it; u.sun is (ambient, sun, shadow darkness, on);
// u.bg.w on: glass casts a tinted shadow, from its own depth map
@group(1) @binding(0) var shadowTex: texture_depth_2d;
@group(1) @binding(1) var shadowSamp: sampler_comparison;
@group(1) @binding(2) var glassTex: texture_depth_2d;

struct VOut { @builtin(position) pos: vec4<f32>, @location(0) n: vec3<f32>, @location(1) tint: f32, @location(2) col: vec4<f32>, @location(3) a: f32, @location(4) wp: vec3<f32> };

@vertex fn vs_box(@location(0) p: vec3<f32>, @location(1) n: vec3<f32>,
                  @location(2) off: vec4<f32>, @location(3) sc: vec4<f32>, @location(4) col: vec4<f32>) -> VOut {
  var o: VOut;
  o.pos = u.vp * vec4<f32>(p * sc.xyz + off.xyz, 1.0);
  o.n = n; o.tint = off.w; o.col = col; o.a = 1.0 - sc.w; o.wp = p * sc.xyz + off.xyz;
  return o;
}
// how much of the sun reaches a point: 1 lit, 0 in shadow, softened over the 3 x 3 texels round it (glass: the
// glass depth map instead)
fn shadowIn(wp: vec3<f32>, n: vec3<f32>, glass: bool) -> f32 {
  let c = u.lvp * vec4<f32>(wp + n * 0.03, 1.0);
  let q = c.xyz / c.w;
  let uv = vec2<f32>(q.x * 0.5 + 0.5, 0.5 - q.y * 0.5);
  if (uv.x < 0.0 || uv.x > 1.0 || uv.y < 0.0 || uv.y > 1.0 || q.z > 1.0) { return 1.0; }
  let t = 1.0 / 2048.0;
  var s = 0.0;
  for (var x = -1; x <= 1; x++) {
    for (var y = -1; y <= 1; y++) {
      let at = uv + vec2<f32>(f32(x), f32(y)) * t;
      if (glass) { s += textureSampleCompareLevel(glassTex, shadowSamp, at, q.z - 0.0015); }
      else { s += textureSampleCompareLevel(shadowTex, shadowSamp, at, q.z - 0.0015); }
    }
  }
  return s / 9.0;
}
fn shadowAt(wp: vec3<f32>, n: vec3<f32>) -> f32 { return shadowIn(wp, n, false); }
@vertex fn vs_depth(@location(0) p: vec3<f32>, @location(2) off: vec4<f32>, @location(3) sc: vec4<f32>) -> @builtin(position) vec4<f32> {
  return u.vp * vec4<f32>(p * sc.xyz + off.xyz, 1.0);
}
@vertex fn vs_shadow(@location(0) p: vec3<f32>, @location(2) off: vec4<f32>, @location(3) sc: vec4<f32>) -> @builtin(position) vec4<f32> {
  return u.lvp * vec4<f32>(p * sc.xyz + off.xyz, 1.0);
}
// a material's maps (Realistic): albedo (alpha the cutout), surface (normal x, normal y, height), roughness.
// mu.a: 1 / tile width and height (per metre), cos and sin of the tile's rotation; mu.b: tile offset (m), parallax
// depth (m), on; mu.c: roughness, metalness, reflectivity; mu.d: tint
struct MU { a: vec4<f32>, b: vec4<f32>, c: vec4<f32>, d: vec4<f32> };
@group(2) @binding(0) var albTex: texture_2d<f32>;
@group(2) @binding(1) var surfTex: texture_2d<f32>;
@group(2) @binding(2) var roughTex: texture_2d<f32>;
@group(2) @binding(3) var matSamp: sampler;
@group(2) @binding(4) var<uniform> mu: MU;

// a surface pattern on the face (Hidden Line, Shaded, Consistent Colours): pu.h is (line families, solid, drafting,
// paper mm a pixel), pu.fg the line colour, pu.bg the background colour (w on); each family is three vec4: (cos,
// sin of its angle, spacing, shift), (origin along, origin across, dash period, -), (dash, space, dash, space), in
// model mm on the face, or for a drafting pattern paper mm square to the screen
struct PU { h: vec4<f32>, fg: vec4<f32>, bg: vec4<f32>, f: array<vec4<f32>, 96> };
@group(3) @binding(0) var<uniform> pu: PU;
// pmm0 is the point in the face plane (mm), hx0 and hy0 how it changes from pixel to pixel; fc the pixel
fn hatch(c: vec3<f32>, pmm0: vec2<f32>, hx0: vec2<f32>, hy0: vec2<f32>, fc: vec2<f32>) -> vec3<f32> {
  var pmm = pmm0;
  var hx = hx0;
  var hy = hy0;
  if (pu.h.z > 0.5) {
    pmm = vec2<f32>(fc.x, -fc.y) * pu.h.w;
    hx = vec2<f32>(pu.h.w, 0.0);
    hy = vec2<f32>(0.0, -pu.h.w);
  }
  var out = c;
  if (pu.bg.w > 0.5) { out = pu.bg.rgb; }
  if (pu.h.y > 0.5) { return pu.fg.rgb; }
  let n = i32(pu.h.x);
  for (var k = 0; k < 32; k++) {
    if (k >= n) { break; }
    let a = pu.f[k * 3];
    let b = pu.f[k * 3 + 1];
    let ds = pu.f[k * 3 + 2];
    let dd = a.xy;
    let nn = vec2<f32>(-a.y, a.x);
    let sp = a.z;
    let v = dot(pmm, nn) - b.y;
    let j = round(v / sp);
    // mm a pixel across and along the lines
    let fw = abs(dot(hx, nn)) + abs(dot(hy, nn)) + 1e-6;
    let fa = abs(dot(hx, dd)) + abs(dot(hy, dd)) + 1e-6;
    var cov = 1.0 - smoothstep(0.4, 1.2, abs(v - j * sp) / fw);
    if (b.z > 0.0) {
      let along = dot(pmm, dd) - b.x - j * a.w;
      let t = along - b.z * floor(along / b.z);
      let s1 = ds.x + ds.y;
      let on = t < max(ds.x, fa) || (ds.z + ds.w > 0.0 && t >= s1 && t < s1 + max(ds.z, fa));
      if (!on) { cov = 0.0; }
    }
    // lines closer than a few pixels read as a tone, as on paper, instead of shimmering
    cov = mix(min(0.3, 1.5 * fw / sp), cov, smoothstep(2.5, 5.0, sp / fw));
    out = mix(out, pu.fg.rgb, cov);
  }
  return out;
}

// a point in the face's plane (m, before the tile's rotation) to texture coordinates (rows run down)
fn toTex(pm: vec2<f32>) -> vec2<f32> {
  let r = vec2<f32>(pm.x * mu.a.z + pm.y * mu.a.w, -pm.x * mu.a.w + pm.y * mu.a.z) * mu.a.xy;
  return vec2<f32>(r.x, -r.y);
}
fn depthAt(t: vec2<f32>, gx: vec2<f32>, gy: vec2<f32>) -> f32 { return 1.0 - textureSampleGrad(surfTex, matSamp, t, gx, gy).b; }

@fragment fn fs_box(i: VOut) -> @location(0) vec4<f32> {
  // the face's frame (walls: along and up; floors and roofs: x and y, or across and up the slope) and the material's
  // texture coordinates, worked out first so their derivatives are taken everywhere
  let gn = normalize(i.n);
  var T = vec3<f32>(1.0, 0.0, 0.0);
  if (abs(gn.z) < 0.999) { T = normalize(cross(vec3<f32>(0.0, 0.0, 1.0), gn)); }
  let B = cross(gn, T);
  let pm = vec2<f32>(dot(i.wp, T), dot(i.wp, B)) - mu.b.xy;
  let tex0 = toTex(pm);
  let gx = dpdx(tex0);
  let gy = dpdy(tex0);
  let pmm = vec2<f32>(dot(i.wp, T), dot(i.wp, B)) * 1000.0;
  let hx = dpdx(pmm);
  let hy = dpdy(pmm);
  if (u.light.w > 0.5) {
    // Hidden Line: every surface in the page colour, so the edges drawn over it are the only linework
    var c = u.bg.rgb;
    if (i.tint >= 0.99) { c = mix(c, vec3<f32>(0.85, 0.62, 0.45), 0.45); }
    if (i.tint < 0.0) { c = mix(c, vec3<f32>(0.55, 0.72, 0.86), 0.25); }
    // cast shadows in Hidden Line: grey where the sun does not reach
    if (u.sun.w > 0.5) {
      let hv = select(shadowAt(i.wp, gn), 1.0, dot(gn, normalize(u.light.xyz)) <= 0.0);
      c = c * mix(1.0 - 0.45 * u.sun.z, 1.0, hv);
    }
    return vec4<f32>(hatch(c, pmm, hx, hy, i.pos.xy), i.a);
  }
  // u.eye.w: 0 Shaded (grey by the light), 1 Consistent Colours (flat material colour), 2 Realistic
  let mode = u.eye.w;
  let selected = i.tint >= 0.99;
  let glass = i.tint < 0.0;
  var base = mix(vec3<f32>(0.78, 0.80, 0.83), vec3<f32>(0.85, 0.62, 0.45), clamp(i.tint, 0.0, 1.0));
  if (i.col.w > 0.5 && !selected) { base = i.col.rgb; } // material colour unless selected
  if (glass) { base = vec3<f32>(0.55, 0.72, 0.86); }
  if (mode < 0.5 && !selected && !glass) { base = vec3<f32>(0.9, 0.9, 0.9); }
  if (mode > 0.5 && mode < 1.5) { return vec4<f32>(hatch(base, pmm, hx, hy, i.pos.xy), i.a); }
  let L = normalize(u.light.xyz);
  var nn = gn;
  var tex = tex0;
  var lightOcc = 1.0;
  var rough = 1.0;
  var alpha = i.a;
  let textured = mode > 1.5 && mu.b.w > 0.5 && !selected && !glass;
  // the tile's own axes in the world (it may be turned)
  let Td = T * mu.a.z + B * mu.a.w;
  let Bd = -T * mu.a.w + B * mu.a.z;
  if (textured) {
    let V = normalize(u.eye.xyz - i.wp);
    let vt = vec3<f32>(dot(V, T), dot(V, B), dot(V, gn));
    // parallax occlusion while the relief spans a pixel or more on screen, plain normal mapping beyond
    let reliefPx = mu.b.z / max(length(u.eye.xyz - i.wp), 0.01) * u.screen.y * 0.5 / u.proj.w;
    if (mu.b.z > 0.0 && reliefPx > 0.6 && vt.z > 0.03) {
      let dtex = toTex(-vt.xy / vt.z * mu.b.z);
      let n = mix(40.0, 12.0, vt.z);
      let st = 1.0 / n;
      var cur = tex0;
      var d = 0.0;
      var hd = depthAt(cur, gx, gy);
      for (var k = 0; k < 48; k++) {
        if (d >= hd) { break; }
        cur += dtex * st;
        d += st;
        hd = depthAt(cur, gx, gy);
      }
      let prev = cur - dtex * st;
      let after = hd - d;
      let before = depthAt(prev, gx, gy) - (d - st);
      let w = clamp(after / (after - before + 1e-5), 0.0, 1.0);
      let hit = mix(cur, prev, w);
      let fade = smoothstep(0.6, 2.5, reliefPx);
      tex = mix(tex0, hit, fade);
      // self-shadowing: from the hit back up toward the sun, does the relief stand in the way?
      let lt = vec3<f32>(dot(L, T), dot(L, B), dot(L, gn));
      if (lt.z > 0.03) {
        let hitD = depthAt(hit, gx, gy);
        let ltex = toTex(lt.xy / lt.z * mu.b.z);
        var occ = 0.0;
        for (var j = 1; j <= 12; j++) {
          let dd = hitD * (1.0 - f32(j) / 12.0);
          let q = hit + ltex * (hitD - dd);
          occ = max(occ, clamp((dd - depthAt(q, gx, gy)) * 12.0, 0.0, 1.0));
        }
        lightOcc = 1.0 - occ * fade * 0.8;
      }
    }
    let a = textureSampleGrad(albTex, matSamp, tex, gx, gy);
    if (a.a < 0.5) { discard; }
    base = a.rgb * mu.d.rgb;
    let s = textureSampleGrad(surfTex, matSamp, tex, gx, gy);
    let nx = s.r * 2.0 - 1.0;
    let ny = s.g * 2.0 - 1.0;
    nn = normalize(Td * nx + Bd * ny + gn * sqrt(max(0.0, 1.0 - nx * nx - ny * ny)));
    rough = clamp(textureSampleGrad(roughTex, matSamp, tex, gx, gy).r, 0.03, 1.0);
  }
  let l = max(dot(nn, L), 0.0) * lightOcc;
  let lg = dot(gn, L);
  var k = vec3<f32>(u.sun.x + u.sun.y * l);
  // Shaded: faces toward the light near white, turned away dark grey
  if (mode < 0.5) { k = vec3<f32>(u.sun.x * 0.45 + u.sun.y * 1.25 * l); }
  var vis = 1.0;
  if (u.sun.w > 0.5) {
    // a face turned from the sun has only the ambient light already: a cast shadow darkens what the sun reaches
    vis = select(shadowAt(i.wp, gn), 1.0, lg <= 0.0);
    k = mix(k * (1.0 - u.sun.z), k, vis);
    // through glass (Realistic): the light that passes is tinted and weaker
    if (u.bg.w > 0.5 && lg > 0.0) {
      let gv = shadowIn(i.wp, gn, true);
      k = k * mix(mix(vec3<f32>(1.0), vec3<f32>(0.62, 0.75, 0.86), u.sun.z), vec3<f32>(1.0), max(gv, 1.0 - vis));
    }
  }
  if (mode < 0.5) { return vec4<f32>(hatch(base * k, pmm, hx, hy, i.pos.xy), alpha); }
  if (!textured) { return vec4<f32>(base * k, alpha); }
  // Realistic with maps: metallic and roughness shading (GGX)
  let metal = mu.c.y;
  let V = normalize(u.eye.xyz - i.wp);
  let H = normalize(L + V);
  let ndv = max(dot(nn, V), 0.001);
  let ndh = max(dot(nn, H), 0.0);
  let a2 = rough * rough * rough * rough;
  let dn = ndh * ndh * (a2 - 1.0) + 1.0;
  let D = a2 / (3.14159 * dn * dn);
  let F0 = mix(vec3<f32>(mu.c.z), base, metal);
  let F = F0 + (vec3<f32>(1.0) - F0) * pow(1.0 - max(dot(H, V), 0.0), 5.0);
  let kk = (rough + 1.0) * (rough + 1.0) / 8.0;
  let G = ndv / (ndv * (1.0 - kk) + kk) * l / (l * (1.0 - kk) + kk + 1e-4);
  let spec = D * F * G / max(4.0 * ndv * l, 0.001) * u.sun.y * l * vis;
  let env = F0 * u.sun.x * 0.6 * (1.0 - rough * 0.5);
  return vec4<f32>(base * (1.0 - metal) * k + spec + env, alpha);
}

struct LOut { @builtin(position) pos: vec4<f32>, @location(0) c: vec3<f32> };
@vertex fn vs_line(@location(0) p: vec3<f32>, @location(1) c: vec3<f32>) -> LOut {
  var o: LOut; o.pos = u.vp * vec4<f32>(p, 1.0); o.c = c; return o;
}
@fragment fn fs_line(i: LOut) -> @location(0) vec4<f32> { return vec4<f32>(i.c, 1.0); }

// edges of the model: the instance buffer carries the selection tint in its w
struct EOut { @builtin(position) pos: vec4<f32>, @location(0) tint: f32 };
// u.edge.w: how far (m) edges are brought towards the eye, so sketchy edges moved off their faces stay in front
// (the near plane is 0.1 m, so a depth of z changes by 0.1 k / z² for k metres)
@vertex fn vs_edge(@location(0) p: vec3<f32>, @location(2) off: vec4<f32>) -> EOut {
  var o: EOut; o.pos = u.vp * vec4<f32>(p, 1.0); o.pos.z -= 0.1 * u.edge.w / max(o.pos.w, 0.1); o.tint = off.w; return o;
}
@fragment fn fs_edge(i: EOut) -> @location(0) vec4<f32> {
  if (i.tint >= 0.99) { return vec4<f32>(0.85, 0.35, 0.08, 1.0); }
  return vec4<f32>(u.edge.rgb, 1.0);
}
`;

  // Ambient shadows (screen-space ambient occlusion): from the depth of the solid surfaces, how much of the sky round a
  // point nearby surfaces hide; drawn over the frame, multiplying. u.ao is (on, strength, radius m, -), u.proj turns a
  // depth back into a distance (the projection's z terms, tan of half the view across and up), u.screen is the size.
  const AO_SHADER = /* wgsl */ `
struct U { vp: mat4x4<f32>, light: vec4<f32>, bg: vec4<f32>, edge: vec4<f32>, lvp: mat4x4<f32>, sun: vec4<f32>, ao: vec4<f32>, proj: vec4<f32>, screen: vec4<f32> };
@group(0) @binding(0) var<uniform> u: U;
@group(1) @binding(0) var depthTex: texture_depth_2d;
@vertex fn vs_full(@builtin(vertex_index) k: u32) -> @builtin(position) vec4<f32> {
  var p = array<vec2<f32>, 3>(vec2<f32>(-1.0, -1.0), vec2<f32>(3.0, -1.0), vec2<f32>(-1.0, 3.0));
  return vec4<f32>(p[k], 0.0, 1.0);
}
fn viewAt(px: vec2<i32>) -> vec4<f32> {
  let size = vec2<i32>(u.screen.xy);
  let q = clamp(px, vec2<i32>(0), size - vec2<i32>(1));
  let d = textureLoad(depthTex, q, 0);
  if (d >= 0.99999) { return vec4<f32>(0.0, 0.0, 0.0, 0.0); }
  let dist = u.proj.y / (d + u.proj.x);
  let ndc = vec2<f32>((f32(q.x) + 0.5) / u.screen.x * 2.0 - 1.0, 1.0 - (f32(q.y) + 0.5) / u.screen.y * 2.0);
  return vec4<f32>(ndc.x * u.proj.z * dist, ndc.y * u.proj.w * dist, -dist, 1.0);
}
// Silhouettes (u.ao.w): a jump in depth between this pixel and one beside it is the outline of a surface against
// what lies behind it; both sides darken, so the line is a few pixels wide
fn silAt(px: vec2<i32>, p: vec4<f32>) -> f32 {
  let d0 = select(1e9, -p.z, p.w > 0.5);
  for (var dy = -1; dy <= 1; dy++) {
    for (var dx = -1; dx <= 1; dx++) {
      let q = viewAt(px + vec2<i32>(dx, dy));
      let dq = select(1e9, -q.z, q.w > 0.5);
      let near = min(d0, dq);
      if (max(d0, dq) - near > max(0.25, near * 0.04)) { return 0.12; }
    }
  }
  return 1.0;
}
@fragment fn fs_ao(@builtin(position) fc: vec4<f32>) -> @location(0) vec4<f32> {
  let px = vec2<i32>(fc.xy);
  let p = viewAt(px);
  var sil = 1.0;
  if (u.ao.w > 0.5) { sil = silAt(px, p); }
  if (p.w < 0.5 || u.ao.x < 0.5) { return vec4<f32>(sil, sil, sil, 1.0); }
  let px1 = viewAt(px + vec2<i32>(1, 0));
  let py1 = viewAt(px + vec2<i32>(0, 1));
  var n = normalize(cross(px1.xyz - p.xyz, py1.xyz - p.xyz));
  if (dot(n, -p.xyz) < 0.0) { n = -n; }
  let dist = -p.z;
  let R = u.ao.z;
  let rp = clamp(R * u.screen.y * 0.5 / (u.proj.w * dist), 2.0, 80.0);
  let spin = fract(sin(dot(fc.xy, vec2<f32>(12.9898, 78.233))) * 43758.5453) * 6.2832;
  var occ = 0.0;
  for (var k = 0; k < 16; k++) {
    let a = f32(k) * 2.39996 + spin;
    let r = rp * sqrt((f32(k) + 0.5) / 16.0);
    let s = viewAt(px + vec2<i32>(i32(cos(a) * r), i32(sin(a) * r)));
    if (s.w < 0.5) { continue; }
    let v = s.xyz - p.xyz; let vv = dot(v, v);
    occ += max(0.0, dot(v, n) - 0.01 * dist) / (vv + 0.02) * (1.0 - smoothstep(R * R, 4.0 * R * R, vv));
  }
  let ao = pow(clamp(1.0 - u.ao.y * R * occ / 16.0, 0.0, 1.0), 1.5) * sil;
  return vec4<f32>(ao, ao, ao, 1.0);
}
// the view's Background (Graphic Display Options): u.screen.z is where the horizon is (NDC y), u.screen.w 1 Sky, 2 Gradient
@fragment fn fs_bg(@builtin(position) fc: vec4<f32>) -> @location(0) vec4<f32> {
  let d = 1.0 - fc.y / u.screen.y * 2.0 - u.screen.z;
  if (u.screen.w > 1.5) {
    if (d >= 0.0) { return vec4<f32>(mix(vec3<f32>(0.97, 0.97, 0.97), vec3<f32>(0.55, 0.70, 0.88), clamp(d * 0.8, 0.0, 1.0)), 1.0); }
    return vec4<f32>(mix(vec3<f32>(0.97, 0.97, 0.97), vec3<f32>(0.60, 0.60, 0.60), clamp(-d * 1.5, 0.0, 1.0)), 1.0);
  }
  if (d >= 0.0) { return vec4<f32>(mix(vec3<f32>(0.86, 0.91, 0.96), vec3<f32>(0.33, 0.56, 0.85), pow(clamp(d * 0.7, 0.0, 1.0), 0.7)), 1.0); }
  return vec4<f32>(mix(vec3<f32>(0.80, 0.82, 0.80), vec3<f32>(0.55, 0.57, 0.50), clamp(-d * 2.0, 0.0, 1.0)), 1.0);
}
`;

  // The same shading in GLSL ES 3.00, for WebGL2
  const GL_UNIFORMS = "uniform mat4 vp; uniform vec4 light; uniform vec4 bg; uniform vec4 edge; uniform mat4 lvp; uniform vec4 sun; uniform vec4 ao; uniform vec4 proj; uniform vec4 screen; uniform vec4 eye;";
  const GL_BOX_VS = `#version 300 es
${GL_UNIFORMS}
layout(location = 0) in vec3 p; layout(location = 1) in vec3 n;
layout(location = 2) in vec4 off; layout(location = 3) in vec4 sc; layout(location = 4) in vec4 col;
out vec3 vn; out float vtint; out vec4 vcol; out float va; out vec3 vwp;
void main() { vwp = p * sc.xyz + off.xyz; gl_Position = vp * vec4(vwp, 1.0); vn = n; vtint = off.w; vcol = col; va = 1.0 - sc.w; }`;
  const GL_BOX_FS = `#version 300 es
precision highp float;
${GL_UNIFORMS}
uniform highp sampler2DShadow shadowMap; uniform highp sampler2DShadow glassMap;
uniform sampler2D albTex; uniform sampler2D surfTex; uniform sampler2D roughTex;
uniform vec4 mA; uniform vec4 mB; uniform vec4 mC; uniform vec4 mD;
uniform vec4 pat[99];
in vec3 vn; in float vtint; in vec4 vcol; in float va; in vec3 vwp; out vec4 o;
float shadowIn(vec3 wp, vec3 n, bool glass) {
  vec4 c = lvp * vec4(wp + n * 0.03, 1.0);
  vec3 q = c.xyz / c.w;
  vec2 uv = q.xy * 0.5 + 0.5;
  if (uv.x < 0.0 || uv.x > 1.0 || uv.y < 0.0 || uv.y > 1.0 || q.z > 1.0) return 1.0;
  float t = 1.0 / 2048.0, s = 0.0;
  for (int x = -1; x <= 1; x++) for (int y = -1; y <= 1; y++) {
    vec3 at = vec3(uv + vec2(float(x), float(y)) * t, q.z - 0.0015);
    s += glass ? texture(glassMap, at) : texture(shadowMap, at);
  }
  return s / 9.0;
}
float shadowAt(vec3 wp, vec3 n) { return shadowIn(wp, n, false); }
vec2 toTex(vec2 pm) { vec2 r = vec2(pm.x * mA.z + pm.y * mA.w, -pm.x * mA.w + pm.y * mA.z) * mA.xy; return vec2(r.x, -r.y); }
float depthAt(vec2 t, vec2 gx, vec2 gy) { return 1.0 - textureGrad(surfTex, t, gx, gy).b; }
// a surface pattern on the face (see PU in the WGSL)
vec3 hatch(vec3 c, vec2 pmm, vec2 hx, vec2 hy) {
  if (pat[0].z > 0.5) { pmm = gl_FragCoord.xy * pat[0].w; hx = vec2(pat[0].w, 0.0); hy = vec2(0.0, pat[0].w); }
  vec3 o2 = c;
  if (pat[2].w > 0.5) o2 = pat[2].rgb;
  if (pat[0].y > 0.5) return pat[1].rgb;
  int n = int(pat[0].x);
  for (int k = 0; k < 32; k++) {
    if (k >= n) break;
    vec4 a = pat[3 + k * 3], b = pat[4 + k * 3], ds = pat[5 + k * 3];
    vec2 dd = a.xy, nn = vec2(-a.y, a.x);
    float sp = a.z, v = dot(pmm, nn) - b.y, j = floor(v / sp + 0.5);
    float fw = abs(dot(hx, nn)) + abs(dot(hy, nn)) + 1e-6, fa = abs(dot(hx, dd)) + abs(dot(hy, dd)) + 1e-6;
    float cov = 1.0 - smoothstep(0.4, 1.2, abs(v - j * sp) / fw);
    if (b.z > 0.0) {
      float along = dot(pmm, dd) - b.x - j * a.w, t = along - b.z * floor(along / b.z), s1 = ds.x + ds.y;
      bool on = t < max(ds.x, fa) || (ds.z + ds.w > 0.0 && t >= s1 && t < s1 + max(ds.z, fa));
      if (!on) cov = 0.0;
    }
    cov = mix(min(0.3, 1.5 * fw / sp), cov, smoothstep(2.5, 5.0, sp / fw));
    o2 = mix(o2, pat[1].rgb, cov);
  }
  return o2;
}
void main() {
  vec3 gn = normalize(vn);
  vec3 T = abs(gn.z) < 0.999 ? normalize(cross(vec3(0.0, 0.0, 1.0), gn)) : vec3(1.0, 0.0, 0.0);
  vec3 B = cross(gn, T);
  vec2 tex0 = toTex(vec2(dot(vwp, T), dot(vwp, B)) - mB.xy);
  vec2 gx = dFdx(tex0), gy = dFdy(tex0);
  vec2 pmm = vec2(dot(vwp, T), dot(vwp, B)) * 1000.0, hx = dFdx(pmm), hy = dFdy(pmm);
  if (light.w > 0.5) {
    vec3 c = bg.rgb;
    if (vtint >= 0.99) c = mix(c, vec3(0.85, 0.62, 0.45), 0.45);
    if (vtint < 0.0) c = mix(c, vec3(0.55, 0.72, 0.86), 0.25);
    if (sun.w > 0.5) { float hv = dot(gn, normalize(light.xyz)) <= 0.0 ? 1.0 : shadowAt(vwp, gn); c *= mix(1.0 - 0.45 * sun.z, 1.0, hv); }
    o = vec4(hatch(c, pmm, hx, hy), va); return;
  }
  float mode = eye.w;
  bool selected = vtint >= 0.99, glass = vtint < 0.0;
  vec3 base = mix(vec3(0.78, 0.80, 0.83), vec3(0.85, 0.62, 0.45), clamp(vtint, 0.0, 1.0));
  if (vcol.w > 0.5 && !selected) base = vcol.rgb;
  if (glass) base = vec3(0.55, 0.72, 0.86);
  if (mode < 0.5 && !selected && !glass) base = vec3(0.9);
  if (mode > 0.5 && mode < 1.5) { o = vec4(hatch(base, pmm, hx, hy), va); return; }
  vec3 L = normalize(light.xyz);
  vec3 nn = gn; vec2 tx = tex0; float lightOcc = 1.0, rough = 1.0;
  bool textured = mode > 1.5 && mB.w > 0.5 && !selected && !glass;
  vec3 Td = T * mA.z + B * mA.w, Bd = -T * mA.w + B * mA.z;
  if (textured) {
    vec3 V = normalize(eye.xyz - vwp);
    vec3 vt = vec3(dot(V, T), dot(V, B), dot(V, gn));
    float reliefPx = mB.z / max(length(eye.xyz - vwp), 0.01) * screen.y * 0.5 / proj.w;
    if (mB.z > 0.0 && reliefPx > 0.6 && vt.z > 0.03) {
      vec2 dtex = toTex(-vt.xy / vt.z * mB.z);
      float st = 1.0 / mix(40.0, 12.0, vt.z), d = 0.0;
      vec2 cur = tex0; float hd = depthAt(cur, gx, gy);
      for (int k = 0; k < 48; k++) { if (d >= hd) break; cur += dtex * st; d += st; hd = depthAt(cur, gx, gy); }
      vec2 prev = cur - dtex * st;
      float after = hd - d, before = depthAt(prev, gx, gy) - (d - st);
      vec2 hit = mix(cur, prev, clamp(after / (after - before + 1e-5), 0.0, 1.0));
      float fade = smoothstep(0.6, 2.5, reliefPx);
      tx = mix(tex0, hit, fade);
      vec3 lt = vec3(dot(L, T), dot(L, B), dot(L, gn));
      if (lt.z > 0.03) {
        float hitD = depthAt(hit, gx, gy), occ = 0.0;
        vec2 ltex = toTex(lt.xy / lt.z * mB.z);
        for (int j = 1; j <= 12; j++) { float dd = hitD * (1.0 - float(j) / 12.0); occ = max(occ, clamp((dd - depthAt(hit + ltex * (hitD - dd), gx, gy)) * 12.0, 0.0, 1.0)); }
        lightOcc = 1.0 - occ * fade * 0.8;
      }
    }
    vec4 a = textureGrad(albTex, tx, gx, gy);
    if (a.a < 0.5) discard;
    base = a.rgb * mD.rgb;
    vec4 s = textureGrad(surfTex, tx, gx, gy);
    float nx = s.r * 2.0 - 1.0, ny = s.g * 2.0 - 1.0;
    nn = normalize(Td * nx + Bd * ny + gn * sqrt(max(0.0, 1.0 - nx * nx - ny * ny)));
    rough = clamp(textureGrad(roughTex, tx, gx, gy).r, 0.03, 1.0);
  }
  float l = max(dot(nn, L), 0.0) * lightOcc, lg = dot(gn, L);
  vec3 k = vec3(sun.x + sun.y * l);
  if (mode < 0.5) k = vec3(sun.x * 0.45 + sun.y * 1.25 * l);
  float vis = 1.0;
  if (sun.w > 0.5) {
    vis = lg <= 0.0 ? 1.0 : shadowAt(vwp, gn);
    k = mix(k * (1.0 - sun.z), k, vis);
    if (bg.w > 0.5 && lg > 0.0) { float gv = shadowIn(vwp, gn, true); k *= mix(mix(vec3(1.0), vec3(0.62, 0.75, 0.86), sun.z), vec3(1.0), max(gv, 1.0 - vis)); }
  }
  if (mode < 0.5) { o = vec4(hatch(base * k, pmm, hx, hy), va); return; }
  if (!textured) { o = vec4(base * k, va); return; }
  float metal = mC.y;
  vec3 V = normalize(eye.xyz - vwp), H = normalize(L + V);
  float ndv = max(dot(nn, V), 0.001), ndh = max(dot(nn, H), 0.0), a2 = rough * rough * rough * rough;
  float dn = ndh * ndh * (a2 - 1.0) + 1.0, D = a2 / (3.14159 * dn * dn);
  vec3 F0 = mix(vec3(mC.z), base, metal), F = F0 + (1.0 - F0) * pow(1.0 - max(dot(H, V), 0.0), 5.0);
  float kk = (rough + 1.0) * (rough + 1.0) / 8.0, G = ndv / (ndv * (1.0 - kk) + kk) * l / (l * (1.0 - kk) + kk + 1e-4);
  vec3 spec = D * F * G / max(4.0 * ndv * l, 0.001) * sun.y * l * vis, env = F0 * sun.x * 0.6 * (1.0 - rough * 0.5);
  o = vec4(base * (1.0 - metal) * k + spec + env, va);
}`;
  // the sun's depth map: depth only
  const GL_SHADOW_VS = `#version 300 es
uniform mat4 vp;
layout(location = 0) in vec3 p; layout(location = 2) in vec4 off; layout(location = 3) in vec4 sc;
void main() { gl_Position = vp * vec4(p * sc.xyz + off.xyz, 1.0); }`;
  const GL_SHADOW_FS = `#version 300 es
precision highp float;
out vec4 o;
void main() { o = vec4(1.0); }`;
  // ambient shadows over the frame (see AO_SHADER)
  const GL_BG_FS = `#version 300 es
precision highp float;
${GL_UNIFORMS}
out vec4 o;
void main() {
  float d = 1.0 - gl_FragCoord.y / screen.y * 2.0;
  d = -d - screen.z;
  if (screen.w > 1.5) {
    o = d >= 0.0 ? vec4(mix(vec3(0.97), vec3(0.55, 0.70, 0.88), clamp(d * 0.8, 0.0, 1.0)), 1.0) : vec4(mix(vec3(0.97), vec3(0.60), clamp(-d * 1.5, 0.0, 1.0)), 1.0);
  } else {
    o = d >= 0.0 ? vec4(mix(vec3(0.86, 0.91, 0.96), vec3(0.33, 0.56, 0.85), pow(clamp(d * 0.7, 0.0, 1.0), 0.7)), 1.0) : vec4(mix(vec3(0.80, 0.82, 0.80), vec3(0.55, 0.57, 0.50), clamp(-d * 2.0, 0.0, 1.0)), 1.0);
  }
}`;
  const GL_AO_VS = `#version 300 es
void main() { vec2 p = vec2(gl_VertexID == 1 ? 3.0 : -1.0, gl_VertexID == 2 ? 3.0 : -1.0); gl_Position = vec4(p, 0.0, 1.0); }`;
  const GL_AO_FS = `#version 300 es
precision highp float;
${GL_UNIFORMS}
uniform highp sampler2D depthMap;
out vec4 o;
vec4 viewAt(ivec2 px) {
  ivec2 q = clamp(px, ivec2(0), ivec2(screen.xy) - ivec2(1));
  float d = texelFetch(depthMap, q, 0).r;
  if (d >= 0.99999) return vec4(0.0);
  float dist = proj.y / (d + proj.x);
  vec2 ndc = (vec2(q) + 0.5) / screen.xy * 2.0 - 1.0;
  return vec4(ndc.x * proj.z * dist, ndc.y * proj.w * dist, -dist, 1.0);
}
float silAt(ivec2 px, vec4 p) {
  float d0 = p.w > 0.5 ? -p.z : 1e9;
  for (int dy = -1; dy <= 1; dy++) for (int dx = -1; dx <= 1; dx++) {
    vec4 q = viewAt(px + ivec2(dx, dy));
    float dq = q.w > 0.5 ? -q.z : 1e9, near = min(d0, dq);
    if (max(d0, dq) - near > max(0.25, near * 0.04)) return 0.12;
  }
  return 1.0;
}
void main() {
  ivec2 px = ivec2(gl_FragCoord.xy);
  vec4 p = viewAt(px);
  float sil = ao.w > 0.5 ? silAt(px, p) : 1.0;
  if (p.w < 0.5 || ao.x < 0.5) { o = vec4(sil, sil, sil, 1.0); return; }
  vec3 n = normalize(cross(viewAt(px + ivec2(1, 0)).xyz - p.xyz, viewAt(px + ivec2(0, 1)).xyz - p.xyz));
  if (dot(n, -p.xyz) < 0.0) n = -n;
  float dist = -p.z, R = ao.z;
  float rp = clamp(R * screen.y * 0.5 / (proj.w * dist), 2.0, 80.0);
  float spin = fract(sin(dot(gl_FragCoord.xy, vec2(12.9898, 78.233))) * 43758.5453) * 6.2832, occ = 0.0;
  for (int k = 0; k < 16; k++) {
    float a = float(k) * 2.39996 + spin, r = rp * sqrt((float(k) + 0.5) / 16.0);
    vec4 s = viewAt(px + ivec2(int(cos(a) * r), int(sin(a) * r)));
    if (s.w < 0.5) continue;
    vec3 v = s.xyz - p.xyz; float vv = dot(v, v);
    occ += max(0.0, dot(v, n) - 0.01 * dist) / (vv + 0.02) * (1.0 - smoothstep(R * R, 4.0 * R * R, vv));
  }
  float a = pow(clamp(1.0 - ao.y * R * occ / 16.0, 0.0, 1.0), 1.5) * sil;
  o = vec4(a, a, a, 1.0);
}`;
  const GL_LINE_VS = `#version 300 es
${GL_UNIFORMS}
layout(location = 0) in vec3 p; layout(location = 1) in vec3 c; out vec3 vc;
void main() { gl_Position = vp * vec4(p, 1.0); vc = c; }`;
  const GL_LINE_FS = `#version 300 es
precision highp float;
in vec3 vc; out vec4 o;
void main() { o = vec4(vc, 1.0); }`;
  const GL_EDGE_VS = `#version 300 es
${GL_UNIFORMS}
layout(location = 0) in vec3 p; layout(location = 2) in vec4 off; out float vtint;
void main() { gl_Position = vp * vec4(p, 1.0); gl_Position.z -= 0.2 * edge.w / max(gl_Position.w, 0.1); vtint = off.w; }`;
  const GL_EDGE_FS = `#version 300 es
precision highp float;
${GL_UNIFORMS}
in float vtint; out vec4 o;
void main() { o = vtint >= 0.99 ? vec4(0.85, 0.35, 0.08, 1.0) : vec4(edge.rgb, 1.0); }`;

  function boxVerts() {
    // unit cube [0,1]^3, 36 vertices of (pos, normal)
    const faces = [
      [[0, 0, 1], [[0, 0, 1], [1, 0, 1], [1, 1, 1], [0, 1, 1]]],
      [[0, 0, -1], [[0, 1, 0], [1, 1, 0], [1, 0, 0], [0, 0, 0]]],
      [[1, 0, 0], [[1, 0, 0], [1, 1, 0], [1, 1, 1], [1, 0, 1]]],
      [[-1, 0, 0], [[0, 1, 0], [0, 0, 0], [0, 0, 1], [0, 1, 1]]],
      [[0, 1, 0], [[1, 1, 0], [0, 1, 0], [0, 1, 1], [1, 1, 1]]],
      [[0, -1, 0], [[0, 0, 0], [1, 0, 0], [1, 0, 1], [0, 0, 1]]],
    ];
    const out = [];
    for (const [n, q] of faces) for (const i of [0, 1, 2, 0, 2, 3]) out.push(...q[i], ...n);
    return new Float32Array(out);
  }

  function gridVerts(R, step, majorEvery) {
    const out = [];
    for (let i = -R; i <= R; i += step) {
      const major = (i / step) % majorEvery === 0, c = major ? 0.62 : 0.84;
      out.push(i, -R, 0, c, c, c + 0.02, i, R, 0, c, c, c + 0.02);
      out.push(-R, i, 0, c, c, c + 0.02, R, i, 0, c, c, c + 0.02);
    }
    const L = Math.max(10, R / 10);
    out.push(0, 0, 0, 0.8, 0.2, 0.2, L, 0, 0, 0.8, 0.2, 0.2); // x axis
    out.push(0, 0, 0, 0.2, 0.65, 0.25, 0, L, 0, 0.2, 0.65, 0.25); // y axis
    out.push(0, 0, 0, 0.2, 0.35, 0.85, 0, 0, L, 0.2, 0.35, 0.85); // z axis
    return new Float32Array(out);
  }

  // the stress boxes: offset.xyz + tint, scale.xyz + pad, colour (12 floats each, colour unused)
  function stressInstances() {
    const I = [];
    for (let t = 0; t < 15; t++) {
      const tx = (t % 5) * 90 - 180, ty = Math.floor(t / 5) * 90 - 90;
      for (let l = 0; l < 10; l++) for (let s = 0; s < 8; s++) {
        const kind = (t + l + s) % 4;
        I.push(tx + (s % 4) * 11, ty + Math.floor(s / 4) * 9, l * 3.2, kind / 3, 10, 8, 3, 0, 0, 0, 0, 0);
      }
    }
    return new Float32Array(I);
  }

  // The edges Hidden Line and Wireframe draw: those on one triangle only (the boundary of a surface) and those where
  // two triangles meet at more than 30 degrees. Flat faces and the gentle facets of a curved one draw no line.
  // `list` is every mesh as { data, group }; meshes in one group (the layers of joined walls in one material) are
  // taken together, so where two of them meet flush there is no line. A seam where one triangle edge meets several
  // along the same line (a T-junction, as where a wall face is cut into bands round an opening) is matched piece by
  // piece, so only the parts with a surface on one side draw. Returns the edge lines of each mesh.
  function featureEdges(list) {
    const q = (x) => Math.round(x * 1000);
    const dot = (a, b) => a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    const out = list.map(() => []);
    const groups = new Map();
    list.forEach((m, mi) => { if (!groups.has(m.group)) groups.set(m.group, []); groups.get(m.group).push(mi); });
    for (const members of groups.values()) {
      const map = new Map();
      for (const mi of members) {
        const v = list[mi].data;
        for (let i = 0; i + 17 < v.length; i += 18) {
          const P = [[v[i], v[i + 1], v[i + 2]], [v[i + 6], v[i + 7], v[i + 8]], [v[i + 12], v[i + 13], v[i + 14]]];
          const e1 = [P[1][0] - P[0][0], P[1][1] - P[0][1], P[1][2] - P[0][2]], e2 = [P[2][0] - P[0][0], P[2][1] - P[0][1], P[2][2] - P[0][2]];
          let n = [e1[1] * e2[2] - e1[2] * e2[1], e1[2] * e2[0] - e1[0] * e2[2], e1[0] * e2[1] - e1[1] * e2[0]];
          const l = Math.hypot(...n);
          if (l < 1e-9) continue;
          n = [n[0] / l, n[1] / l, n[2] / l];
          for (let k = 0; k < 3; k++) {
            const a = P[k], b = P[(k + 1) % 3], ka = `${q(a[0])},${q(a[1])},${q(a[2])}`, kb = `${q(b[0])},${q(b[1])},${q(b[2])}`;
            const key = ka < kb ? `${ka}|${kb}` : `${kb}|${ka}`;
            let e = map.get(key);
            if (!e) { e = { a, b, ns: [], mesh: mi }; map.set(key, e); }
            e.ns.push(n);
          }
        }
      }
      const loose = [];
      for (const e of map.values()) {
        // two faces back to back on the edge (where joined layers of two walls meet inside the solid) cancel: the edge
        // is judged by what is left, so a flush join draws no line and a real corner still does
        const ns = e.ns.slice();
        for (let i = 0; i < ns.length; i++) {
          const j = ns.findIndex((m, k) => k > i && dot(ns[i], m) < -0.999);
          if (j > 0) { ns.splice(j, 1); ns.splice(i, 1); i--; }
        }
        if (!ns.length) continue;
        if (ns.length === 1) { loose.push({ ...e, ns }); continue; }
        if (ns.length !== 2 || dot(ns[0], ns[1]) < 0.866) out[e.mesh].push(...e.a, ...e.b);
      }
      // edges on one triangle only, by the line they lie on and the plane of their triangle: split at one another's
      // ends, a piece with a triangle either side is a seam inside a flat face
      const lines = new Map();
      for (const e of loose) {
        let d = [e.b[0] - e.a[0], e.b[1] - e.a[1], e.b[2] - e.a[2]];
        const l = Math.hypot(...d);
        if (l < 1e-9) continue;
        d = d.map((x) => x / l);
        const flip = d[0] < -1e-9 || (Math.abs(d[0]) <= 1e-9 && (d[1] < -1e-9 || (Math.abs(d[1]) <= 1e-9 && d[2] < 0)));
        if (flip) d = d.map((x) => -x);
        const s = dot(e.a, d), foot = [e.a[0] - d[0] * s, e.a[1] - d[1] * s, e.a[2] - d[2] * s];
        const key = [...d.map((x) => Math.round(x * 1e4)), ...foot.map((x) => Math.round(x * 10)), ...e.ns[0].map((x) => Math.round(x * 1e3))].join(",");
        const t0 = dot(e.a, d), t1 = dot(e.b, d);
        if (!lines.has(key)) lines.set(key, []);
        lines.get(key).push({ lo: Math.min(t0, t1), hi: Math.max(t0, t1), d, foot, mesh: e.mesh });
      }
      for (const segs of lines.values()) {
        if (segs.length === 1) {
          const s = segs[0];
          out[s.mesh].push(...s.foot.map((f, k) => f + s.d[k] * s.lo), ...s.foot.map((f, k) => f + s.d[k] * s.hi));
          continue;
        }
        const ts = segs.flatMap((s) => [s.lo, s.hi]).sort((a, b) => a - b).filter((t, i, a) => i === 0 || t - a[i - 1] > 1e-4);
        const pieces = new Map(); // index of the piece between ts[i] and ts[i + 1] -> the segments covering it
        for (const s of segs) {
          for (let i = 0; i + 1 < ts.length; i++) {
            if (ts[i] >= s.lo - 1e-4 && ts[i + 1] <= s.hi + 1e-4) { if (!pieces.has(i)) pieces.set(i, []); pieces.get(i).push(s); }
          }
        }
        for (const [i, by] of pieces) {
          if (by.length !== 1) continue;
          const s = by[0];
          out[s.mesh].push(...s.foot.map((f, k) => f + s.d[k] * ts[i]), ...s.foot.map((f, k) => f + s.d[k] * ts[i + 1]));
        }
      }
    }
    return out.map((a) => new Float32Array(a));
  }

  // Copy the lines in `src` (x y z r g b pairs) into `out`, cut to what the camera sees (a little beyond the sides of
  // the view). Lines that pass behind the camera or reach far off screen are drawn as stray marks, or dropped, by some
  // drivers (software renderers among them), so the grid goes through this every frame. `m` is the view-projection
  // matrix; `depth01` is WebGPU's 0..w depth range, otherwise GL's -w..w. Returns the number of vertices written.
  function clipLines(src, m, out, depth01) {
    let k = 0;
    const cl = (i) => {
      const x = src[i], y = src[i + 1], z = src[i + 2];
      return [m[0] * x + m[4] * y + m[8] * z + m[12], m[1] * x + m[5] * y + m[9] * z + m[13], m[2] * x + m[6] * y + m[10] * z + m[14], m[3] * x + m[7] * y + m[11] * z + m[15]];
    };
    // each boundary as a function of the clip-space point that is >= 0 inside
    const sides = [(c) => 1.05 * c[3] - c[0], (c) => 1.05 * c[3] + c[0], (c) => 1.05 * c[3] - c[1], (c) => 1.05 * c[3] + c[1], (c) => c[3] - c[2], depth01 ? (c) => c[2] : (c) => c[3] + c[2]];
    for (let i = 0; i + 11 < src.length; i += 12) {
      const p = cl(i), q = cl(i + 6);
      let t0 = 0, t1 = 1, gone = false;
      for (const f of sides) {
        const f0 = f(p), f1 = f(q);
        if (f0 < 0 && f1 < 0) { gone = true; break; }
        if (f0 < 0) t0 = Math.max(t0, f0 / (f0 - f1));
        else if (f1 < 0) t1 = Math.min(t1, f0 / (f0 - f1));
      }
      if (gone || t0 >= t1) continue;
      for (const t of [t0, t1]) {
        for (let c = 0; c < 3; c++) out[k + c] = src[i + c] + (src[i + 6 + c] - src[i + c]) * t;
        out.set(src.subarray(i + 3, i + 6), k + 3);
        k += 6;
      }
    }
    return k / 6;
  }

  // ---------------------------------------------------------------- materials
  // What Realistic draws a material with: three images the same size, each as a chain of mipmaps (canvases halving
  // down to 1 x 1), and 16 values (see MU in the shader).
  const canvasOf = (w, h) => { const c = document.createElement("canvas"); c.width = w; c.height = h; return c; };
  function mipChain(c) {
    const out = [c];
    while (out[out.length - 1].width > 1 || out[out.length - 1].height > 1) {
      const p = out[out.length - 1], n = canvasOf(Math.max(1, p.width >> 1), Math.max(1, p.height >> 1));
      const x = n.getContext("2d"); x.imageSmoothingEnabled = true; x.imageSmoothingQuality = "high";
      x.drawImage(p, 0, 0, n.width, n.height);
      out.push(n);
    }
    return out;
  }
  const solid = (r, g, b, a = 255) => { const c = canvasOf(1, 1), x = c.getContext("2d"); x.fillStyle = `rgba(${r},${g},${b},${a / 255})`; x.fillRect(0, 0, 1, 1); return [c]; };
  // the default: no maps, off
  function defaultMaterial() {
    return { albedo: solid(255, 255, 255), surface: solid(128, 128, 255), rough: solid(255, 255, 255), params: new Float32Array([1, 1, 1, 0, 0, 0, 0, 0, 1, 0, 0.04, 0, 1, 1, 1, 0]) };
  }
  globalThis.Viewport_defaultMaterial = defaultMaterial;
  // A surface pattern as 99 vec4s (see PU in the shader): `surf` is a mesh's { pattern, color, background }, `def`
  // the pattern's { solid, lines } in model mm. At most 32 line families and two dashes a family are drawn.
  function patternData(surf, def) {
    const out = new Float32Array(99 * 4);
    if (!surf) return out;
    const c = (surf.color || [0, 0, 0]).map((x) => x / 255);
    out.set([c[0], c[1], c[2], 1], 4);
    if (surf.background) out.set([...surf.background.map((x) => x / 255), 1], 8);
    if (def && def.solid) { out[1] = 1; return out; }
    // a drafting pattern keeps its paper size on screen (96 pixels an inch at 100 %)
    if (def && def.drafting) { out[2] = 1; out[3] = 25.4 / 96 / (globalThis.devicePixelRatio || 1); }
    const lines = (def && def.lines || []).filter((l) => l.spacing > 0).slice(0, 32);
    out[0] = lines.length;
    lines.forEach((l, k) => {
      const a = (l.angle * Math.PI) / 180, ca = Math.cos(a), sa = Math.sin(a);
      const dash = (l.dashes || []).slice(0, 4), period = (l.dashes || []).reduce((t, d) => t + d, 0);
      const o = 12 + k * 12;
      out.set([ca, sa, l.spacing, l.shift || 0], o);
      out.set([l.origin[0] * ca + l.origin[1] * sa, -l.origin[0] * sa + l.origin[1] * ca, dash.length ? period : 0, 0], o + 4);
      out.set([dash[0] || 0, dash[1] || 0, dash[2] || 0, dash[3] || 0], o + 8);
    });
    return out;
  }
  const loadImage = (url) => new Promise((ok, bad) => { const i = new Image(); i.onload = () => ok(i); i.onerror = () => bad(new Error("could not read a material map")); i.src = url; });
  /**
   * Turn material_textures' answer into what setMaterial takes. A standard appearance's slots are drawn to one size
   * and packed: normal (or normals worked out from a bump map) and height into `surface`, the cutout into albedo's alpha.
   */
  async function prepareMaterial(t) {
    const imgs = {};
    for (const k of ["albedo", "surface", "roughness_map", "normal", "height", "cutout"]) if (t[k]) imgs[k] = await loadImage(t[k]);
    const first = imgs.albedo || imgs.surface || imgs.normal || imgs.height || imgs.roughness_map || imgs.cutout;
    if (!first) return null;
    const cap = 2048, sc = Math.min(1, cap / Math.max(first.naturalWidth, first.naturalHeight));
    const w = Math.max(1, Math.round(first.naturalWidth * sc)), h = Math.max(1, Math.round(first.naturalHeight * sc));
    const draw = (img) => { const c = canvasOf(w, h), x = c.getContext("2d"); x.drawImage(img, 0, 0, w, h); return c; };
    const pixels = (img) => draw(img).getContext("2d").getImageData(0, 0, w, h);
    const col = t.color || [190, 190, 190];
    let albedo;
    if (imgs.albedo) albedo = draw(imgs.albedo); else { albedo = canvasOf(w, h); const x = albedo.getContext("2d"); x.fillStyle = `rgb(${col.join(",")})`; x.fillRect(0, 0, w, h); }
    if (imgs.cutout) {
      const x = albedo.getContext("2d"), a = x.getImageData(0, 0, w, h), c = pixels(imgs.cutout);
      for (let i = 0; i < a.data.length; i += 4) a.data[i + 3] = c.data[i] >= 128 ? 255 : 0;
      x.putImageData(a, 0, 0);
    }
    let surface;
    if (imgs.surface) surface = draw(imgs.surface);
    else {
      surface = canvasOf(w, h);
      const x = surface.getContext("2d"), out = x.createImageData(w, h);
      const nrm = imgs.normal && !t.bump_is_height ? pixels(imgs.normal) : null;
      const bump = imgs.normal && t.bump_is_height ? pixels(imgs.normal) : null;
      const hgt = imgs.height ? pixels(imgs.height) : null;
      // a bump map's slope: its full range is the parallax depth (or 2 mm) over the sample size
      const mmx = (t.uv_size_mm?.[0] || 1000) / w, mmy = (t.uv_size_mm?.[1] || 1000) / h, depth = t.pom_depth_mm > 0 ? t.pom_depth_mm : 2;
      const at = (px, i, j) => px.data[((((j % h) + h) % h) * w + (((i % w) + w) % w)) * 4] / 255;
      for (let j = 0; j < h; j++) for (let i = 0; i < w; i++) {
        const k = (j * w + i) * 4;
        let nx = 0, ny = 0;
        if (nrm) { nx = nrm.data[k] / 127.5 - 1; ny = nrm.data[k + 1] / 127.5 - 1; }
        else if (bump) {
          nx = -(at(bump, i + 1, j) - at(bump, i - 1, j)) * depth / (2 * mmx);
          ny = (at(bump, i, j - 1) - at(bump, i, j + 1)) * depth / (2 * mmy);
          const l = Math.hypot(nx, ny, 1); nx /= l; ny /= l;
        }
        out.data[k] = Math.round((nx * 0.5 + 0.5) * 255); out.data[k + 1] = Math.round((ny * 0.5 + 0.5) * 255);
        out.data[k + 2] = hgt ? hgt.data[k] : 255; out.data[k + 3] = 255;
      }
      x.putImageData(out, 0, 0);
    }
    let rough;
    if (imgs.roughness_map) rough = draw(imgs.roughness_map);
    else { rough = canvasOf(1, 1); const x = rough.getContext("2d"), v = Math.round((t.roughness ?? 0.6) * 255); x.fillStyle = `rgb(${v},${v},${v})`; x.fillRect(0, 0, 1, 1); }
    if (rough.width !== w || rough.height !== h) { const r = canvasOf(w, h); r.getContext("2d").drawImage(rough, 0, 0, w, h); rough = r; }
    const uv = t.uv_size_mm || [1000, 1000], rot = ((t.uv_rotation_deg || 0) * Math.PI) / 180, off = t.uv_offset_mm || [0, 0];
    // the albedo already carries the colour (a standard appearance with no image is filled with it)
    const tint = [1, 1, 1];
    const params = new Float32Array([1000 / Math.max(1, uv[0]), 1000 / Math.max(1, uv[1]), Math.cos(rot), Math.sin(rot), off[0] / 1000, off[1] / 1000, (t.pom_depth_mm || 0) / 1000, 1,
      t.roughness ?? 0.6, t.metalness ?? 0, t.reflectivity ?? 0.04, 0, ...tint, 0]);
    return { key: t.key, albedo: mipChain(albedo), surface: mipChain(surface), rough: mipChain(rough), params };
  }

  // ---------------------------------------------------------------- backends
  // A backend owns the GPU objects. mesh() uploads a model mesh (pos+normal triangles and its edge lines),
  // instance() writes the 12 per-mesh values (offset+tint, scale, colour), render() draws a frame, and
  // read() gives the frame just drawn back as RGBA rows, top row first.
  const STATIC = () => ({ box: boxVerts(), grid: gridVerts(60, 1, 10), bigGrid: gridVerts(400, 10, 5), stress: stressInstances() });

  async function webgpuBackend(canvas) {
    if (!navigator.gpu) throw new Error("navigator.gpu is not available in this webview");
    const adapter = await navigator.gpu.requestAdapter();
    if (!adapter) throw new Error("no GPU adapter found");
    const device = await adapter.requestDevice();
    // a lost device stops every 3D view drawing: the desktop log keeps why, for a frozen window's report
    device.lost.then((info) => Backend.log("gpu", `WebGPU device lost (${info.reason || "unknown"}): ${info.message || ""}`));
    device.addEventListener("uncapturederror", (e) => Backend.log("gpu", `WebGPU error: ${e.error && e.error.message}`));
    // only now take the canvas, so a failed WebGPU start leaves it free for WebGL2
    const ctx = canvas.getContext("webgpu");
    const format = navigator.gpu.getPreferredCanvasFormat();
    ctx.configure({ device, format, alphaMode: "opaque", usage: GPUTextureUsage.RENDER_ATTACHMENT | GPUTextureUsage.COPY_SRC });
    const info = adapter.info || {};

    const module = device.createShaderModule({ code: SHADER });
    const ubuf = device.createBuffer({ size: 256, usage: GPUBufferUsage.UNIFORM | GPUBufferUsage.COPY_DST });
    const bgl = device.createBindGroupLayout({ entries: [{ binding: 0, visibility: GPUShaderStage.VERTEX | GPUShaderStage.FRAGMENT, buffer: {} }] });
    const bind = device.createBindGroup({ layout: bgl, entries: [{ binding: 0, resource: { buffer: ubuf } }] });
    // the sun's depth map, drawn first when shadows are on and read by the shaded surfaces
    const SHADOW = 2048;
    const shadowTex = device.createTexture({ size: [SHADOW, SHADOW], format: "depth32float", usage: GPUTextureUsage.RENDER_ATTACHMENT | GPUTextureUsage.TEXTURE_BINDING });
    const shadowView = shadowTex.createView();
    // glass as the sun sees it, for tinted shadows in Realistic
    const glassTex = device.createTexture({ size: [SHADOW, SHADOW], format: "depth32float", usage: GPUTextureUsage.RENDER_ATTACHMENT | GPUTextureUsage.TEXTURE_BINDING });
    const glassView = glassTex.createView();
    const bgl1 = device.createBindGroupLayout({ entries: [
      { binding: 0, visibility: GPUShaderStage.FRAGMENT, texture: { sampleType: "depth" } },
      { binding: 1, visibility: GPUShaderStage.FRAGMENT, sampler: { type: "comparison" } },
      { binding: 2, visibility: GPUShaderStage.FRAGMENT, texture: { sampleType: "depth" } },
    ] });
    const bind1 = device.createBindGroup({ layout: bgl1, entries: [{ binding: 0, resource: shadowView }, { binding: 1, resource: device.createSampler({ compare: "less-equal" }) }, { binding: 2, resource: glassView }] });
    // ambient shadows: the solid surfaces' depth from the camera, then a pass over the frame that multiplies
    const aoModule = device.createShaderModule({ code: AO_SHADER });
    const bglAo = device.createBindGroupLayout({ entries: [{ binding: 0, visibility: GPUShaderStage.FRAGMENT, texture: { sampleType: "depth" } }] });
    let aoTex = null, aoView = null, aoBind = null;
    // a material's maps and values (Realistic), one bind group per material; meshes without one take the default
    const bgl2 = device.createBindGroupLayout({ entries: [
      { binding: 0, visibility: GPUShaderStage.FRAGMENT, texture: {} },
      { binding: 1, visibility: GPUShaderStage.FRAGMENT, texture: {} },
      { binding: 2, visibility: GPUShaderStage.FRAGMENT, texture: {} },
      { binding: 3, visibility: GPUShaderStage.FRAGMENT, sampler: {} },
      { binding: 4, visibility: GPUShaderStage.FRAGMENT, buffer: {} },
    ] });
    const matSampler = device.createSampler({ addressModeU: "repeat", addressModeV: "repeat", magFilter: "linear", minFilter: "linear", mipmapFilter: "linear", maxAnisotropy: 8 });
    const texFrom = (levels) => {
      const t = device.createTexture({ size: [levels[0].width, levels[0].height], format: "rgba8unorm", mipLevelCount: levels.length, usage: GPUTextureUsage.TEXTURE_BINDING | GPUTextureUsage.COPY_DST | GPUTextureUsage.RENDER_ATTACHMENT });
      levels.forEach((c, i) => device.queue.copyExternalImageToTexture({ source: c }, { texture: t, mipLevel: i }, [c.width, c.height]));
      return t;
    };
    const makeMaterial = (m) => {
      const ub = device.createBuffer({ size: 64, usage: GPUBufferUsage.UNIFORM | GPUBufferUsage.COPY_DST });
      device.queue.writeBuffer(ub, 0, m.params);
      const ts = [m.albedo, m.surface, m.rough].map(texFrom);
      const bind = device.createBindGroup({ layout: bgl2, entries: [...ts.map((t, i) => ({ binding: i, resource: t.createView() })), { binding: 3, resource: matSampler }, { binding: 4, resource: { buffer: ub } }] });
      return { bind, free() { ub.destroy(); ts.forEach((t) => t.destroy()); } };
    };
    const defaultMat = makeMaterial(Viewport_defaultMaterial());
    // a surface pattern, one bind group per pattern and colour in use; meshes without one take the empty one
    const bgl3 = device.createBindGroupLayout({ entries: [{ binding: 0, visibility: GPUShaderStage.FRAGMENT, buffer: {} }] });
    const makePattern = (data) => {
      const ub = device.createBuffer({ size: data.byteLength, usage: GPUBufferUsage.UNIFORM | GPUBufferUsage.COPY_DST });
      device.queue.writeBuffer(ub, 0, data);
      return { bind: device.createBindGroup({ layout: bgl3, entries: [{ binding: 0, resource: { buffer: ub } }] }), free() { ub.destroy(); } };
    };
    const defaultPat = makePattern(new Float32Array(99 * 4));
    const layout = device.createPipelineLayout({ bindGroupLayouts: [bgl, bgl1, bgl2, bgl3] });
    const shadowLayout = device.createPipelineLayout({ bindGroupLayouts: [bgl] });
    const depthFormat = "depth24plus";
    const boxBuffers = [
      { arrayStride: 24, attributes: [{ shaderLocation: 0, offset: 0, format: "float32x3" }, { shaderLocation: 1, offset: 12, format: "float32x3" }] },
      { arrayStride: 48, stepMode: "instance", attributes: [{ shaderLocation: 2, offset: 0, format: "float32x4" }, { shaderLocation: 3, offset: 16, format: "float32x4" }, { shaderLocation: 4, offset: 32, format: "float32x4" }] },
    ];
    const boxPipe = device.createRenderPipeline({
      layout, vertex: { module, entryPoint: "vs_box", buffers: boxBuffers },
      fragment: { module, entryPoint: "fs_box", targets: [{ format }] },
      primitive: { topology: "triangle-list", cullMode: "back" },
      depthStencil: { format: depthFormat, depthWriteEnabled: true, depthCompare: "less" },
    });
    // Hidden Line draws faces a little behind where they are, so the edges over them are not lost to depth fighting
    const boxBiasPipe = device.createRenderPipeline({
      layout, vertex: { module, entryPoint: "vs_box", buffers: boxBuffers },
      fragment: { module, entryPoint: "fs_box", targets: [{ format }] },
      primitive: { topology: "triangle-list", cullMode: "back" },
      depthStencil: { format: depthFormat, depthWriteEnabled: true, depthCompare: "less", depthBias: 4, depthBiasSlopeScale: 2 },
    });
    // see-through surfaces (a selection, glass, a material with transparency): drawn after the rest, blended
    const blend = { color: { srcFactor: "src-alpha", dstFactor: "one-minus-src-alpha" }, alpha: { srcFactor: "one", dstFactor: "one-minus-src-alpha" } };
    const boxSeePipe = device.createRenderPipeline({
      layout, vertex: { module, entryPoint: "vs_box", buffers: boxBuffers },
      fragment: { module, entryPoint: "fs_box", targets: [{ format, blend }] },
      primitive: { topology: "triangle-list", cullMode: "back" },
      depthStencil: { format: depthFormat, depthWriteEnabled: false, depthCompare: "less", depthBias: 4, depthBiasSlopeScale: 2 },
    });
    const shadowPipe = device.createRenderPipeline({
      layout: shadowLayout,
      vertex: { module, entryPoint: "vs_shadow", buffers: [
        { arrayStride: 24, attributes: [{ shaderLocation: 0, offset: 0, format: "float32x3" }] },
        { arrayStride: 48, stepMode: "instance", attributes: [{ shaderLocation: 2, offset: 0, format: "float32x4" }, { shaderLocation: 3, offset: 16, format: "float32x4" }] },
      ] },
      primitive: { topology: "triangle-list", cullMode: "none" },
      depthStencil: { format: "depth32float", depthWriteEnabled: true, depthCompare: "less", depthBias: 2, depthBiasSlopeScale: 2 },
    });
    const depthPipe = device.createRenderPipeline({
      layout: shadowLayout,
      vertex: { module, entryPoint: "vs_depth", buffers: [
        { arrayStride: 24, attributes: [{ shaderLocation: 0, offset: 0, format: "float32x3" }] },
        { arrayStride: 48, stepMode: "instance", attributes: [{ shaderLocation: 2, offset: 0, format: "float32x4" }, { shaderLocation: 3, offset: 16, format: "float32x4" }] },
      ] },
      primitive: { topology: "triangle-list", cullMode: "back" },
      depthStencil: { format: "depth32float", depthWriteEnabled: true, depthCompare: "less" },
    });
    const aoPipe = device.createRenderPipeline({
      layout: device.createPipelineLayout({ bindGroupLayouts: [bgl, bglAo] }),
      vertex: { module: aoModule, entryPoint: "vs_full" },
      fragment: { module: aoModule, entryPoint: "fs_ao", targets: [{ format, blend: { color: { srcFactor: "zero", dstFactor: "src" }, alpha: { srcFactor: "zero", dstFactor: "one" } } }] },
      primitive: { topology: "triangle-list" },
    });
    // the background: drawn first, behind everything, leaving depth alone
    const bgPipe = device.createRenderPipeline({
      layout: device.createPipelineLayout({ bindGroupLayouts: [bgl] }),
      vertex: { module: aoModule, entryPoint: "vs_full" },
      fragment: { module: aoModule, entryPoint: "fs_bg", targets: [{ format }] },
      primitive: { topology: "triangle-list" },
      depthStencil: { format: depthFormat, depthWriteEnabled: false, depthCompare: "always" },
    });
    const edgeBuffers = [
      { arrayStride: 12, attributes: [{ shaderLocation: 0, offset: 0, format: "float32x3" }] },
      { arrayStride: 48, stepMode: "instance", attributes: [{ shaderLocation: 2, offset: 0, format: "float32x4" }] },
    ];
    const edgePipe = device.createRenderPipeline({
      layout, vertex: { module, entryPoint: "vs_edge", buffers: edgeBuffers },
      fragment: { module, entryPoint: "fs_edge", targets: [{ format }] },
      primitive: { topology: "line-list" },
      depthStencil: { format: depthFormat, depthWriteEnabled: true, depthCompare: "less-equal" },
    });
    const wirePipe = device.createRenderPipeline({
      layout, vertex: { module, entryPoint: "vs_edge", buffers: edgeBuffers },
      fragment: { module, entryPoint: "fs_edge", targets: [{ format }] },
      primitive: { topology: "line-list" },
      depthStencil: { format: depthFormat, depthWriteEnabled: false, depthCompare: "always" },
    });
    const linePipe = device.createRenderPipeline({
      layout,
      vertex: { module, entryPoint: "vs_line", buffers: [
        { arrayStride: 24, attributes: [{ shaderLocation: 0, offset: 0, format: "float32x3" }, { shaderLocation: 1, offset: 12, format: "float32x3" }] },
      ] },
      fragment: { module, entryPoint: "fs_line", targets: [{ format }] },
      primitive: { topology: "line-list" },
      depthStencil: { format: depthFormat, depthWriteEnabled: true, depthCompare: "less" },
    });
    const mk = (data, usage) => {
      const b = device.createBuffer({ size: Math.max(16, data.byteLength), usage: usage | GPUBufferUsage.COPY_DST });
      device.queue.writeBuffer(b, 0, data);
      return b;
    };
    const st = STATIC();
    const boxBuf = mk(st.box, GPUBufferUsage.VERTEX), stressBuf = mk(st.stress, GPUBufferUsage.VERTEX);
    const clipped = new Float32Array(Math.max(st.grid.length, st.bigGrid.length));
    const gridBuf = device.createBuffer({ size: clipped.byteLength, usage: GPUBufferUsage.VERTEX | GPUBufferUsage.COPY_DST });
    let extraBuf = null, extraClip = null;
    let depth = null;
    const be = {
      name: "WebGPU",
      info: [info.vendor, info.architecture, info.description].filter(Boolean).join(" ") || "adapter ok",
      gpuMs: 0,
      resize(w, h) {
        depth?.destroy(); depth = device.createTexture({ size: [w, h], format: depthFormat, usage: GPUTextureUsage.RENDER_ATTACHMENT });
        aoTex?.destroy(); aoTex = device.createTexture({ size: [w, h], format: "depth32float", usage: GPUTextureUsage.RENDER_ATTACHMENT | GPUTextureUsage.TEXTURE_BINDING });
        aoView = aoTex.createView(); aoBind = device.createBindGroup({ layout: bglAo, entries: [{ binding: 0, resource: aoView }] });
      },
      mesh(data, edges) {
        return { vbuf: mk(data, GPUBufferUsage.VERTEX), ebuf: edges.length ? mk(edges, GPUBufferUsage.VERTEX) : null, ibuf: device.createBuffer({ size: 48, usage: GPUBufferUsage.VERTEX | GPUBufferUsage.COPY_DST }), count: data.length / 6, ecount: edges.length / 3 };
      },
      free(g) { g.vbuf.destroy(); g.ibuf.destroy(); g.ebuf?.destroy(); },
      instance(g, inst) { device.queue.writeBuffer(g.ibuf, 0, inst); },
      material: makeMaterial,
      pattern: makePattern,
      render(f) {
        device.queue.writeBuffer(ubuf, 0, f.uniforms);
        const enc = device.createCommandEncoder();
        // cast shadows: the opaque surfaces as the sun sees them
        if (f.shadow) {
          const sp = enc.beginRenderPass({ colorAttachments: [], depthStencilAttachment: { view: shadowView, depthClearValue: 1, depthLoadOp: "clear", depthStoreOp: "store" } });
          sp.setPipeline(shadowPipe); sp.setBindGroup(0, bind);
          for (const g of f.meshes) { if (!g.solid) continue; sp.setVertexBuffer(0, g.vbuf); sp.setVertexBuffer(1, g.ibuf); sp.draw(g.count, 1); }
          sp.end();
          // and the glass, for tinted shadows
          if (f.glassShadow) {
            const gp = enc.beginRenderPass({ colorAttachments: [], depthStencilAttachment: { view: glassView, depthClearValue: 1, depthLoadOp: "clear", depthStoreOp: "store" } });
            gp.setPipeline(shadowPipe); gp.setBindGroup(0, bind);
            for (const g of f.meshes) { if (g.solid) continue; gp.setVertexBuffer(0, g.vbuf); gp.setVertexBuffer(1, g.ibuf); gp.draw(g.count, 1); }
            gp.end();
          }
        }
        // ambient shadows: the solid surfaces' depth from the camera
        if (f.ao && aoView) {
          const dp = enc.beginRenderPass({ colorAttachments: [], depthStencilAttachment: { view: aoView, depthClearValue: 1, depthLoadOp: "clear", depthStoreOp: "store" } });
          dp.setPipeline(depthPipe); dp.setBindGroup(0, bind);
          for (const g of [...f.meshes, ...(f.ground && f.aoGround ? [f.ground] : [])]) { if (g.alpha < 0.999) continue; dp.setVertexBuffer(0, g.vbuf); dp.setVertexBuffer(1, g.ibuf); dp.draw(g.count, 1); }
          dp.end();
        }
        const tex = ctx.getCurrentTexture();
        const pass = enc.beginRenderPass({
          colorAttachments: [{ view: tex.createView(), loadOp: "clear", storeOp: "store", clearValue: { r: f.clear[0], g: f.clear[1], b: f.clear[2], a: 1 } }],
          depthStencilAttachment: { view: depth.createView(), depthClearValue: 1, depthLoadOp: "clear", depthStoreOp: "store" },
        });
        if (f.background && !f.stress) { pass.setPipeline(bgPipe); pass.setBindGroup(0, bind); pass.draw(3); }
        pass.setBindGroup(0, bind); pass.setBindGroup(1, bind1); pass.setBindGroup(2, defaultMat.bind); pass.setBindGroup(3, defaultPat.bind);
        const n = clipLines(f.stress ? st.bigGrid : st.grid, f.uniforms.subarray(0, 16), clipped, true);
        if (n) { device.queue.writeBuffer(gridBuf, 0, clipped, 0, n * 6); pass.setPipeline(linePipe); pass.setVertexBuffer(0, gridBuf); pass.draw(n); }
        // lines given with setLines (imported CAD drawings), cut to the view like the grid
        if (f.lines && f.lines.length && !f.stress) {
            if (!extraClip || extraClip.length < f.lines.length) {
              extraClip = new Float32Array(f.lines.length);
              extraBuf?.destroy();
              extraBuf = device.createBuffer({ size: extraClip.byteLength, usage: GPUBufferUsage.VERTEX | GPUBufferUsage.COPY_DST });
            }
            const k = clipLines(f.lines, f.uniforms.subarray(0, 16), extraClip, true);
            if (k) { device.queue.writeBuffer(extraBuf, 0, extraClip, 0, k * 6); pass.setPipeline(linePipe); pass.setVertexBuffer(0, extraBuf); pass.draw(k); }
        }
        if (f.stress) {
          pass.setPipeline(boxPipe);
          pass.setVertexBuffer(0, boxBuf); pass.setVertexBuffer(1, stressBuf); pass.draw(36, st.stress.length / 12);
        } else {
          if (f.lineMode !== 2) {
            pass.setPipeline(f.lineMode === 1 || f.edges ? boxBiasPipe : boxPipe);
            for (const g of f.meshes) { if (g.alpha < 0.999) continue; pass.setBindGroup(2, (g.mat || defaultMat).bind); pass.setBindGroup(3, (g.pat || defaultPat).bind); pass.setVertexBuffer(0, g.vbuf); pass.setVertexBuffer(1, g.ibuf); pass.draw(g.count, 1); }
            pass.setBindGroup(2, defaultMat.bind); pass.setBindGroup(3, defaultPat.bind);
            // the ground plane that catches shadows
            if (f.ground) { pass.setVertexBuffer(0, f.ground.vbuf); pass.setVertexBuffer(1, f.ground.ibuf); pass.draw(f.ground.count, 1); }
            pass.setPipeline(boxSeePipe);
            for (const g of f.meshes) { if (g.alpha >= 0.999) continue; pass.setBindGroup(2, (g.mat || defaultMat).bind); pass.setBindGroup(3, (g.pat || defaultPat).bind); pass.setVertexBuffer(0, g.vbuf); pass.setVertexBuffer(1, g.ibuf); pass.draw(g.count, 1); }
            pass.setBindGroup(2, defaultMat.bind); pass.setBindGroup(3, defaultPat.bind);
          }
          if (f.lineMode || f.edges) {
            pass.setPipeline(f.lineMode === 2 ? wirePipe : edgePipe);
            for (const g of f.meshes) { if (!g.ecount) continue; pass.setVertexBuffer(0, g.ebuf); pass.setVertexBuffer(1, g.ibuf); pass.draw(g.ecount, 1); }
          }
        }
        pass.end();
        // ambient shadows over the frame
        if (f.ao && aoBind) {
          const ap = enc.beginRenderPass({ colorAttachments: [{ view: tex.createView(), loadOp: "load", storeOp: "store" }] });
          ap.setPipeline(aoPipe); ap.setBindGroup(0, bind); ap.setBindGroup(1, aoBind); ap.draw(3); ap.end();
        }
        const t0 = performance.now();
        // the read-back copy (if asked for) goes in the same submission
        if (be.pending) {
          const req = be.pending; be.pending = null;
          const w = canvas.width, h = canvas.height, bpr = Math.ceil((w * 4) / 256) * 256, bgra = format.startsWith("bgra");
          const buf = device.createBuffer({ size: bpr * h, usage: GPUBufferUsage.COPY_DST | GPUBufferUsage.MAP_READ });
          enc.copyTextureToBuffer({ texture: tex }, { buffer: buf, bytesPerRow: bpr }, [w, h]);
          device.queue.submit([enc.finish()]);
          buf.mapAsync(GPUMapMode.READ).then(() => {
            const src = new Uint8Array(buf.getMappedRange()), out = new Uint8ClampedArray(w * h * 4);
            for (let y = 0; y < h; y++) for (let x = 0; x < w; x++) {
              const si = y * bpr + x * 4, di = (y * w + x) * 4;
              out[di] = src[si + (bgra ? 2 : 0)]; out[di + 1] = src[si + 1]; out[di + 2] = src[si + (bgra ? 0 : 2)]; out[di + 3] = 255;
            }
            buf.unmap(); buf.destroy();
            req({ w, h, data: out });
          });
        } else device.queue.submit([enc.finish()]);
        device.queue.onSubmittedWorkDone().then(() => { be.gpuMs = performance.now() - t0; }, () => {});
      },
      pending: null,
    };
    return be;
  }

  function webgl2Backend(canvas) {
    const gl = canvas.getContext("webgl2", { antialias: true, depth: true, alpha: false, preserveDrawingBuffer: false });
    if (!gl) throw new Error("neither WebGPU nor WebGL2 is available in this webview");
    canvas.addEventListener("webglcontextlost", () => Backend.log("gpu", "WebGL2 context lost"));
    const compile = (type, src) => {
      const s = gl.createShader(type);
      gl.shaderSource(s, src); gl.compileShader(s);
      if (!gl.getShaderParameter(s, gl.COMPILE_STATUS)) throw new Error("WebGL2 shader: " + gl.getShaderInfoLog(s));
      return s;
    };
    const program = (vs, fs) => {
      const p = gl.createProgram();
      gl.attachShader(p, compile(gl.VERTEX_SHADER, vs)); gl.attachShader(p, compile(gl.FRAGMENT_SHADER, fs));
      gl.linkProgram(p);
      if (!gl.getProgramParameter(p, gl.LINK_STATUS)) throw new Error("WebGL2 program: " + gl.getProgramInfoLog(p));
      const at = (n) => gl.getUniformLocation(p, n);
      return { p, vp: at("vp"), light: at("light"), bg: at("bg"), edge: at("edge"), lvp: at("lvp"), sun: at("sun"), shadowMap: at("shadowMap"), glassMap: at("glassMap"), ao: at("ao"), proj: at("proj"), screen: at("screen"), depthMap: at("depthMap"), eye: at("eye"),
        albTex: at("albTex"), surfTex: at("surfTex"), roughTex: at("roughTex"), mA: at("mA"), mB: at("mB"), mC: at("mC"), mD: at("mD"), pat: at("pat") };
    };
    const boxProg = program(GL_BOX_VS, GL_BOX_FS), lineProg = program(GL_LINE_VS, GL_LINE_FS), edgeProg = program(GL_EDGE_VS, GL_EDGE_FS), shadowProg = program(GL_SHADOW_VS, GL_SHADOW_FS), aoProg = program(GL_AO_VS, GL_AO_FS), bgProg = program(GL_AO_VS, GL_BG_FS);
    // the sun's depth map
    const SHADOW = 2048;
    const shadowTex = gl.createTexture();
    gl.bindTexture(gl.TEXTURE_2D, shadowTex);
    gl.texStorage2D(gl.TEXTURE_2D, 1, gl.DEPTH_COMPONENT24, SHADOW, SHADOW);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.NEAREST); gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.NEAREST);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE); gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_COMPARE_MODE, gl.COMPARE_REF_TO_TEXTURE); gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_COMPARE_FUNC, gl.LEQUAL);
    // a depth texture and its framebuffer (`compare`: read as a shadow map)
    const depthTarget = (w, h, compare) => {
      const tex = gl.createTexture();
      gl.bindTexture(gl.TEXTURE_2D, tex);
      gl.texStorage2D(gl.TEXTURE_2D, 1, gl.DEPTH_COMPONENT24, w, h);
      gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.NEAREST); gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.NEAREST);
      gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE); gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);
      if (compare) { gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_COMPARE_MODE, gl.COMPARE_REF_TO_TEXTURE); gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_COMPARE_FUNC, gl.LEQUAL); }
      const fbo = gl.createFramebuffer();
      gl.bindFramebuffer(gl.FRAMEBUFFER, fbo);
      gl.framebufferTexture2D(gl.FRAMEBUFFER, gl.DEPTH_ATTACHMENT, gl.TEXTURE_2D, tex, 0);
      gl.drawBuffers([gl.NONE]); gl.readBuffer(gl.NONE);
      gl.bindFramebuffer(gl.FRAMEBUFFER, null);
      return { tex, fbo, w, h };
    };
    const shadowFbo = gl.createFramebuffer();
    gl.bindFramebuffer(gl.FRAMEBUFFER, shadowFbo);
    gl.framebufferTexture2D(gl.FRAMEBUFFER, gl.DEPTH_ATTACHMENT, gl.TEXTURE_2D, shadowTex, 0);
    gl.drawBuffers([gl.NONE]); gl.readBuffer(gl.NONE);
    gl.bindFramebuffer(gl.FRAMEBUFFER, null);
    const glassT = depthTarget(SHADOW, SHADOW, true);
    let aoT = null;
    const buf = (data) => { const b = gl.createBuffer(); gl.bindBuffer(gl.ARRAY_BUFFER, b); gl.bufferData(gl.ARRAY_BUFFER, data, gl.STATIC_DRAW); return b; };
    const st = STATIC();
    // a vertex array per shape of input: positions with normals (or colours), positions only, plus instance data
    const vao = (vbuf, stride, attrs, ibuf) => {
      const a = gl.createVertexArray();
      gl.bindVertexArray(a);
      gl.bindBuffer(gl.ARRAY_BUFFER, vbuf);
      for (const [loc, size, off] of attrs) { gl.enableVertexAttribArray(loc); gl.vertexAttribPointer(loc, size, gl.FLOAT, false, stride, off); gl.vertexAttribDivisor(loc, 0); }
      if (ibuf) {
        gl.bindBuffer(gl.ARRAY_BUFFER, ibuf);
        for (const [loc, off] of [[2, 0], [3, 16], [4, 32]]) { gl.enableVertexAttribArray(loc); gl.vertexAttribPointer(loc, 4, gl.FLOAT, false, 48, off); gl.vertexAttribDivisor(loc, 1); }
      }
      gl.bindVertexArray(null);
      return a;
    };
    const gridDyn = buf(new Float32Array(6)), gridDynVao = vao(gridDyn, 24, [[0, 3, 0], [1, 3, 12]]);
    const clipped = new Float32Array(Math.max(st.grid.length, st.bigGrid.length));
    let extraClip = null;
    const stressVao = vao(buf(st.box), 24, [[0, 3, 0], [1, 3, 12]], buf(st.stress));
    const dbg = gl.getExtension("WEBGL_debug_renderer_info");
    const aniso = gl.getExtension("EXT_texture_filter_anisotropic");
    // a material's maps: three textures with their mipmaps, and its values
    const makeMaterial = (m) => {
      const tex = [m.albedo[0], m.surface[0], m.rough[0]].map((c) => {
        const t = gl.createTexture();
        gl.bindTexture(gl.TEXTURE_2D, t);
        gl.pixelStorei(gl.UNPACK_PREMULTIPLY_ALPHA_WEBGL, false);
        gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA, gl.RGBA, gl.UNSIGNED_BYTE, c);
        gl.generateMipmap(gl.TEXTURE_2D);
        gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.REPEAT); gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.REPEAT);
        gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.LINEAR_MIPMAP_LINEAR); gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.LINEAR);
        if (aniso) gl.texParameterf(gl.TEXTURE_2D, aniso.TEXTURE_MAX_ANISOTROPY_EXT, 8);
        return t;
      });
      return { tex, params: m.params, free() { tex.forEach((t) => gl.deleteTexture(t)); } };
    };
    const defaultMat = makeMaterial(Viewport_defaultMaterial());
    const emptyPat = new Float32Array(99 * 4);
    let shownPat = null;
    const bindPat = (pt) => { const d = pt ? pt.data : emptyPat; if (d !== shownPat) { gl.uniform4fv(boxProg.pat, d); shownPat = d; } };
    const bindMat = (m) => {
      const mm = m || defaultMat;
      mm.tex.forEach((t, i) => { gl.activeTexture(gl.TEXTURE3 + i); gl.bindTexture(gl.TEXTURE_2D, t); });
      gl.activeTexture(gl.TEXTURE0);
      gl.uniform1i(boxProg.albTex, 3); gl.uniform1i(boxProg.surfTex, 4); gl.uniform1i(boxProg.roughTex, 5);
      gl.uniform4fv(boxProg.mA, mm.params.subarray(0, 4)); gl.uniform4fv(boxProg.mB, mm.params.subarray(4, 8)); gl.uniform4fv(boxProg.mC, mm.params.subarray(8, 12)); gl.uniform4fv(boxProg.mD, mm.params.subarray(12, 16));
    };
    let lastW = 0, lastH = 0;
    const be = {
      name: "WebGL2",
      // WebKit reports every GPU as "Apple GPU" for privacy, so on Linux that says nothing
      info: (() => { const r = dbg ? gl.getParameter(dbg.UNMASKED_RENDERER_WEBGL) : gl.getParameter(gl.RENDERER); return /^Apple GPU$/.test(r) && !/Mac/.test(navigator.platform) ? "WebGL2" : `WebGL2 ${r}`; })(),
      gpuMs: 0,
      resize(w, h) { lastW = w; lastH = h; },
      mesh(data, edges) {
        const vbuf = buf(data), ebuf = edges.length ? buf(edges) : null;
        // the per-mesh values are one instance, read with a divisor so both shaders see them as in WebGPU
        const ibuf = buf(new Float32Array(12));
        return { vbuf, ebuf, ibuf, tri: vao(vbuf, 24, [[0, 3, 0], [1, 3, 12]], ibuf), lines: ebuf ? vao(ebuf, 12, [[0, 3, 0]], ibuf) : null, count: data.length / 6, ecount: edges.length / 3 };
      },
      free(g) { gl.deleteBuffer(g.vbuf); gl.deleteBuffer(g.ibuf); if (g.ebuf) gl.deleteBuffer(g.ebuf); gl.deleteVertexArray(g.tri); if (g.lines) gl.deleteVertexArray(g.lines); },
      instance(g, inst) { gl.bindBuffer(gl.ARRAY_BUFFER, g.ibuf); gl.bufferSubData(gl.ARRAY_BUFFER, 0, inst); },
      material: makeMaterial,
      pattern: (data) => ({ data, free() {} }),
      render(f) {
        const t0 = performance.now();
        shownPat = null;
        const toGL = (m) => { const o = new Float32Array(m); for (let c = 0; c < 4; c++) o[c * 4 + 2] = 2 * o[c * 4 + 2] - o[c * 4 + 3]; return o; };
        // cast shadows: the opaque surfaces as the sun sees them
        if (f.shadow) {
          gl.bindFramebuffer(gl.FRAMEBUFFER, shadowFbo);
          gl.viewport(0, 0, SHADOW, SHADOW);
          gl.enable(gl.DEPTH_TEST); gl.depthFunc(gl.LESS); gl.depthMask(true);
          gl.clearDepth(1); gl.clear(gl.DEPTH_BUFFER_BIT);
          gl.disable(gl.CULL_FACE); gl.enable(gl.POLYGON_OFFSET_FILL); gl.polygonOffset(2, 2);
          gl.useProgram(shadowProg.p); gl.uniformMatrix4fv(shadowProg.vp, false, toGL(f.uniforms.subarray(28, 44)));
          for (const g of f.meshes) { if (!g.solid) continue; gl.bindVertexArray(g.tri); gl.drawArraysInstanced(gl.TRIANGLES, 0, g.count, 1); }
          // and the glass, for tinted shadows
          if (f.glassShadow) {
            gl.bindFramebuffer(gl.FRAMEBUFFER, glassT.fbo); gl.clear(gl.DEPTH_BUFFER_BIT);
            for (const g of f.meshes) { if (g.solid) continue; gl.bindVertexArray(g.tri); gl.drawArraysInstanced(gl.TRIANGLES, 0, g.count, 1); }
          }
          gl.disable(gl.POLYGON_OFFSET_FILL);
          gl.bindFramebuffer(gl.FRAMEBUFFER, null);
        }
        // ambient shadows: the solid surfaces' depth from the camera
        const W = lastW || canvas.width, H = lastH || canvas.height;
        if (f.ao) {
          if (!aoT || aoT.w !== W || aoT.h !== H) { if (aoT) { gl.deleteTexture(aoT.tex); gl.deleteFramebuffer(aoT.fbo); } aoT = depthTarget(W, H, false); }
          gl.bindFramebuffer(gl.FRAMEBUFFER, aoT.fbo); gl.viewport(0, 0, W, H);
          gl.enable(gl.DEPTH_TEST); gl.depthFunc(gl.LESS); gl.depthMask(true); gl.clearDepth(1); gl.clear(gl.DEPTH_BUFFER_BIT);
          gl.enable(gl.CULL_FACE); gl.cullFace(gl.BACK);
          gl.useProgram(shadowProg.p); gl.uniformMatrix4fv(shadowProg.vp, false, toGL(f.uniforms.subarray(0, 16)));
          for (const g of [...f.meshes, ...(f.ground && f.aoGround ? [f.ground] : [])]) { if (g.alpha < 0.999) continue; gl.bindVertexArray(g.tri); gl.drawArraysInstanced(gl.TRIANGLES, 0, g.count, 1); }
          gl.bindFramebuffer(gl.FRAMEBUFFER, null);
        }
        gl.viewport(0, 0, lastW || canvas.width, lastH || canvas.height);
        gl.clearColor(f.clear[0], f.clear[1], f.clear[2], 1); gl.clearDepth(1);
        gl.depthMask(true);
        gl.clear(gl.COLOR_BUFFER_BIT | gl.DEPTH_BUFFER_BIT);
        // WebGPU keeps depth in 0..1, GL in -1..1: z' = 2z - w
        const vp = new Float32Array(f.uniforms.subarray(0, 16));
        for (let c = 0; c < 4; c++) vp[c * 4 + 2] = 2 * vp[c * 4 + 2] - vp[c * 4 + 3];
        const use = (pr) => {
          gl.useProgram(pr.p);
          gl.uniformMatrix4fv(pr.vp, false, vp);
          if (pr.light) gl.uniform4fv(pr.light, f.uniforms.subarray(16, 20));
          if (pr.bg) gl.uniform4fv(pr.bg, f.uniforms.subarray(20, 24));
          if (pr.edge) gl.uniform4fv(pr.edge, f.uniforms.subarray(24, 28));
          if (pr.lvp) gl.uniformMatrix4fv(pr.lvp, false, f.uniforms.subarray(28, 44));
          if (pr.sun) gl.uniform4fv(pr.sun, f.uniforms.subarray(44, 48));
          if (pr.shadowMap) { gl.activeTexture(gl.TEXTURE0); gl.bindTexture(gl.TEXTURE_2D, shadowTex); gl.uniform1i(pr.shadowMap, 0); }
          if (pr.glassMap) { gl.activeTexture(gl.TEXTURE1); gl.bindTexture(gl.TEXTURE_2D, glassT.tex); gl.uniform1i(pr.glassMap, 1); gl.activeTexture(gl.TEXTURE0); }
          if (pr.ao) gl.uniform4fv(pr.ao, f.uniforms.subarray(48, 52));
          if (pr.proj) gl.uniform4fv(pr.proj, f.uniforms.subarray(52, 56));
          if (pr.screen) gl.uniform4fv(pr.screen, f.uniforms.subarray(56, 60));
          if (pr.eye) gl.uniform4fv(pr.eye, f.uniforms.subarray(60, 64));
        };
        if (f.background && !f.stress) { use(bgProg); gl.disable(gl.DEPTH_TEST); gl.bindVertexArray(null); gl.drawArrays(gl.TRIANGLES, 0, 3); }
        gl.enable(gl.DEPTH_TEST); gl.depthFunc(gl.LESS);
        gl.disable(gl.CULL_FACE); gl.disable(gl.POLYGON_OFFSET_FILL);
        use(lineProg);
        const n = clipLines(f.stress ? st.bigGrid : st.grid, vp, clipped, false);
        gl.bindBuffer(gl.ARRAY_BUFFER, gridDyn);
        gl.bufferData(gl.ARRAY_BUFFER, clipped.subarray(0, n * 6), gl.DYNAMIC_DRAW);
        gl.bindVertexArray(gridDynVao);
        gl.drawArrays(gl.LINES, 0, n);
        // lines given with setLines (imported CAD drawings), cut to the view like the grid
        if (f.lines && f.lines.length && !f.stress) {
          if (!extraClip || extraClip.length < f.lines.length) extraClip = new Float32Array(f.lines.length);
          const k = clipLines(f.lines, vp, extraClip, false);
          gl.bufferData(gl.ARRAY_BUFFER, extraClip.subarray(0, k * 6), gl.DYNAMIC_DRAW);
          gl.drawArrays(gl.LINES, 0, k);
        }
        gl.enable(gl.CULL_FACE); gl.cullFace(gl.BACK); gl.frontFace(gl.CCW);
        if (f.stress) {
          use(boxProg); bindMat(null); bindPat(null);
          gl.bindVertexArray(stressVao);
          gl.drawArraysInstanced(gl.TRIANGLES, 0, 36, st.stress.length / 12);
        } else {
          if (f.lineMode !== 2) {
            use(boxProg);
            if (f.lineMode === 1 || f.edges) { gl.enable(gl.POLYGON_OFFSET_FILL); gl.polygonOffset(2, 4); }
            for (const g of f.meshes) { if (g.alpha < 0.999) continue; bindMat(g.mat); bindPat(g.pat); gl.bindVertexArray(g.tri); gl.drawArraysInstanced(gl.TRIANGLES, 0, g.count, 1); }
            bindMat(null); bindPat(null);
            // the ground plane that catches shadows
            if (f.ground) { gl.bindVertexArray(f.ground.tri); gl.drawArraysInstanced(gl.TRIANGLES, 0, f.ground.count, 1); }
            // see-through surfaces after the rest, blended, leaving depth alone
            gl.enable(gl.POLYGON_OFFSET_FILL); gl.polygonOffset(2, 4);
            gl.enable(gl.BLEND); gl.blendFuncSeparate(gl.SRC_ALPHA, gl.ONE_MINUS_SRC_ALPHA, gl.ONE, gl.ONE_MINUS_SRC_ALPHA); gl.depthMask(false);
            for (const g of f.meshes) { if (g.alpha >= 0.999) continue; bindMat(g.mat); bindPat(g.pat); gl.bindVertexArray(g.tri); gl.drawArraysInstanced(gl.TRIANGLES, 0, g.count, 1); }
            bindMat(null); bindPat(null);
            gl.disable(gl.BLEND); gl.depthMask(true);
            gl.disable(gl.POLYGON_OFFSET_FILL);
          }
          if (f.lineMode || f.edges) {
            use(edgeProg);
            if (f.lineMode === 2) { gl.depthMask(false); gl.depthFunc(gl.ALWAYS); } else gl.depthFunc(gl.LEQUAL);
            for (const g of f.meshes) { if (!g.lines) continue; gl.bindVertexArray(g.lines); gl.drawArraysInstanced(gl.LINES, 0, g.ecount, 1); }
            gl.depthMask(true); gl.depthFunc(gl.LESS);
          }
        }
        // ambient shadows over the frame, multiplying
        if (f.ao && aoT) {
          use(aoProg);
          gl.activeTexture(gl.TEXTURE2); gl.bindTexture(gl.TEXTURE_2D, aoT.tex); gl.uniform1i(aoProg.depthMap, 2); gl.activeTexture(gl.TEXTURE0);
          gl.disable(gl.DEPTH_TEST); gl.disable(gl.CULL_FACE);
          gl.enable(gl.BLEND); gl.blendFuncSeparate(gl.ZERO, gl.SRC_COLOR, gl.ZERO, gl.ONE);
          gl.bindVertexArray(null); gl.drawArrays(gl.TRIANGLES, 0, 3);
          gl.disable(gl.BLEND); gl.enable(gl.DEPTH_TEST);
        }
        gl.bindVertexArray(null);
        // the drawing buffer is cleared once the frame is shown, so read it back now
        if (be.pending) {
          const req = be.pending; be.pending = null;
          const w = gl.drawingBufferWidth, h = gl.drawingBufferHeight, src = new Uint8Array(w * h * 4);
          gl.readPixels(0, 0, w, h, gl.RGBA, gl.UNSIGNED_BYTE, src);
          const out = new Uint8ClampedArray(w * h * 4);
          for (let y = 0; y < h; y++) out.set(src.subarray((h - 1 - y) * w * 4, (h - y) * w * 4), y * w * 4);
          req({ w, h, data: out });
        }
        be.gpuMs = performance.now() - t0;
      },
      pending: null,
    };
    return be;
  }

  // ---------------------------------------------------------------- the viewport
  // opts: { minDist (how close the camera may come, metres), minFit (the least distance Fit leaves) }: a family is
  // looked at closer than a building
  async function init(canvas, onFrame, opts = {}) {
    const minDist = opts.minDist ?? 2, minFit = opts.minFit ?? 6;
    let afterRender = null;
    let want = null;
    try { want = new URLSearchParams(location.search).get("renderer") || localStorage.getItem("openarch.renderer"); } catch { /* storage can be unavailable */ }
    let be = null, why = "";
    if (want !== "webgl2") {
      try { be = await webgpuBackend(canvas); } catch (e) { why = String(e.message || e); }
    }
    if (!be) be = webgl2Backend(canvas);
    if (why) console.info(`3D view: WebGPU not used (${why}); drawing with WebGL2.`);

    let meshes = []; // { g (backend handles), id, kind, data, bounds, color, material }
    const materials = new Map(); // material name -> { key, gpu }: the maps Realistic draws it with
    let patternGpu = new Map(); // a mesh's surface (pattern and colours) as JSON -> its backend pattern
    let lastVp = null; // the view and projection last drawn with, for box selection
    const cam = { az: -0.9, el: 0.5, dist: 38, target: [0, 0, 1.5] };
    let style = "shaded", stress = false, dirty = true, sized = false, last = performance.now(), frames = 0, acc = 0, fps = 0;

    function resize() {
      const dpr = window.devicePixelRatio || 1;
      const w = Math.max(1, Math.floor(canvas.clientWidth * dpr)), h = Math.max(1, Math.floor(canvas.clientHeight * dpr));
      if (canvas.width !== w || canvas.height !== h || !sized) {
        canvas.width = w; canvas.height = h;
        be.resize(w, h);
        sized = true;
        dirty = true;
      }
    }

    let extraLines = null, sunLines = null, allLines = null;
    // the view's sun (setSun): { dir (toward the sun, or null), relative: { az, alt } (lighting over the viewer's
    // shoulder), intensity, ambient (0..100), shadows, shadowStrength (0..1), groundZ (metres) }
    let sun = null, ground = null, groundKey = "";
    let sceneLo = [0, 0, 0], sceneHi = [0, 0, 0];
    const groundMesh = (z, c, R, colour) => {
      const key = `${z}|${c[0]}|${c[1]}|${R}|${colour}`;
      if (key === groundKey && ground) return ground;
      if (ground) be.free(ground);
      const [x0, x1, y0, y1] = [c[0] - R, c[0] + R, c[1] - R, c[1] + R];
      const v = new Float32Array([x0, y0, z, 0, 0, 1, x1, y0, z, 0, 0, 1, x1, y1, z, 0, 0, 1, x0, y0, z, 0, 0, 1, x1, y1, z, 0, 0, 1, x0, y1, z, 0, 0, 1]);
      ground = be.mesh(v, new Float32Array(0));
      be.instance(ground, new Float32Array([0, 0, 0, 0, 1, 1, 1, 0, ...colour, 1]));
      ground.alpha = 1;
      groundKey = key;
      return ground;
    };
    // an orthographic box along the sun round the model, WebGPU depth 0..1
    function sunMatrix(L) {
      const c = [0, 1, 2].map((k) => (sceneLo[k] + sceneHi[k]) / 2);
      const R = Math.max(1, Math.hypot(sceneHi[0] - sceneLo[0], sceneHi[1] - sceneLo[1], sceneHi[2] - sceneLo[2]) / 2 * 1.05);
      const eye = [c[0] + L[0] * R * 2, c[1] + L[1] * R * 2, c[2] + L[2] * R * 2];
      const up = Math.abs(L[2]) > 0.99 ? [0, 1, 0] : [0, 0, 1];
      // shadows reach the ground far off when the sun is low: the box runs on past the model to there
      const near = R * 0.5, far = R * 3 + (sceneHi[2] - Math.min(sceneLo[2], sun.groundZ)) / Math.max(L[2], 0.05) + R;
      const o = new Float32Array(16);
      o[0] = 1 / R; o[5] = 1 / R; o[10] = -1 / (far - near); o[14] = -near / (far - near); o[15] = 1;
      return mat.mul(o, mat.lookAt(eye, c, up));
    }
    function frame(now) {
      requestAnimationFrame(frame);
      if (stress) { cam.az += 0.0025; dirty = true; }
      if (!dirty) return;
      dirty = false;
      resize();
      const dt = now - last; last = now; frames++; acc += dt;
      if (acc >= 500) { fps = (frames * 1000) / acc; frames = 0; acc = 0; if (stress) onFrame?.(fps, be.gpuMs); }
      const eye = eyePos();
      const aspect = canvas.width / canvas.height;
      const vp = mat.mul(mat.perspective(0.9, aspect, 0.1, 4000), mat.lookAt(eye, cam.target, [0, 0, 1]));
      lastVp = vp;
      const dark = matchMedia("(prefers-color-scheme: dark)").matches;
      const page = dark ? [0.12, 0.14, 0.16] : [0.98, 0.98, 0.99];
      const lineMode = !stress && style === "hidden_line" ? 1 : !stress && style === "wireframe" ? 2 : 0;
      // the light: the view's sun, over the viewer's shoulder, or (no sun given) a fixed one
      let L = [0.4, -0.5, 0.8], amb = 0.38, dif = 0.62, shadow = false;
      if (sun && !stress) {
        if (sun.dir) L = sun.dir;
        else {
          const a = sun.relative || { az: 135, alt: 35 }, ang = cam.az - ((a.az - 90) * Math.PI) / 180, al = (a.alt * Math.PI) / 180;
          L = [Math.cos(al) * Math.cos(ang), Math.cos(al) * Math.sin(ang), Math.sin(al)];
        }
        amb = 0.18 + (sun.ambient / 100) * 0.8; dif = (sun.intensity / 100) * 1.24;
        // the sun below the horizon: no sunlight and no shadows, an even light to work by
        if (L[2] <= 0) { dif = 0; amb = Math.max(amb, 0.6); }
        shadow = !!sun.shadows && lineMode !== 2 && L[2] > 0.02 && meshes.length > 0;
      }
      // glass casts a tinted shadow in Realistic, passes the light otherwise
      const glassShadow = shadow && lineMode === 0 && style === "realistic" && meshes.some((m) => !m.g.solid);
      // Show Edges (Graphic Display Options): Shaded, Consistent Colours and Realistic draw the model's edges over the faces
      const edges = lineMode === 0 && !stress && graphics.showEdges !== false;
      const ao = !!sun?.ambientShadows && !stress && lineMode !== 2 && meshes.length > 0;
      // Show Silhouettes (Graphic Display Options): outlines drawn in the same pass over the frame
      const sil = !!graphics.silhouettes && !stress && lineMode !== 2 && meshes.length > 0;
      const u = new Float32Array(64); u.set(vp, 0); u.set([L[0], L[1], L[2], lineMode === 1 ? 1 : 0], 16); u.set([...page, glassShadow ? 1 : 0], 20);
      // edges' colour, and (w) how far sketchy edges are brought forward: a little more than their jitter
      const pull = graphics.sketchy ? graphics.sketchy.jitter * 0.004 + 0.02 : 0;
      u.set(dark ? [0.88, 0.9, 0.92, pull] : [0.08, 0.09, 0.1, pull], 24);
      if (shadow) u.set(sunMatrix(L), 28);
      u.set([amb, dif, sun ? sun.shadowStrength ?? 0.5 : 0.5, shadow ? 1 : 0], 44);
      // ambient shadows: strength and reach, and what turns a depth back into a distance
      const near = 0.1, far = 4000, th = Math.tan(0.45);
      u.set([ao ? 1 : 0, 1.3, 0.6, sil ? 1 : 0], 48); u.set([far / (near - far), (near * far) / (near - far), th * aspect, th], 52); 
      // the background, and where the horizon falls on screen (a point far off level with the eye)
      const background = graphics.background === "sky" ? 1 : graphics.background === "gradient" ? 2 : 0;
      let horizon = 0;
      if (background) {
        const fx = cam.target[0] - eye[0], fy = cam.target[1] - eye[1], fl = Math.hypot(fx, fy) || 1;
        const q = [eye[0] + (fx / fl) * 3000, eye[1] + (fy / fl) * 3000, eye[2]];
        const cy = vp[1] * q[0] + vp[5] * q[1] + vp[9] * q[2] + vp[13], cw = vp[3] * q[0] + vp[7] * q[1] + vp[11] * q[2] + vp[15];
        horizon = cw > 1e-6 ? Math.max(-4, Math.min(4, cy / cw)) : cam.el > 0 ? 4 : -4;
      }
      u.set([canvas.width, canvas.height, horizon, background], 56);
      // the eye (for specular light and parallax) and how surfaces are coloured: 0 Shaded, 1 Consistent Colours, 2 Realistic
      u.set([eye[0], eye[1], eye[2], style === "consistent_colors" ? 1 : style === "realistic" ? 2 : 0], 60);
      for (const m of meshes) { m.g.mat = m.material ? materials.get(m.material)?.gpu || null : null; m.g.pat = m.surfKey ? patternGpu.get(m.surfKey) || null : null; }
      let gnd = null;
      if (shadow) {
        const c = [(sceneLo[0] + sceneHi[0]) / 2, (sceneLo[1] + sceneHi[1]) / 2];
        const R = Math.max(30, Math.hypot(sceneHi[0] - sceneLo[0], sceneHi[1] - sceneLo[1]) * 3);
        gnd = groundMesh(sun.groundZ - 0.003, c, Math.round(R), dark ? [0.2, 0.22, 0.24] : [0.93, 0.93, 0.91]);
      }
      if (!allLines) allLines = extraLines && sunLines ? (() => { const a = new Float32Array(extraLines.length + sunLines.length); a.set(extraLines); a.set(sunLines, extraLines.length); return a; })() : extraLines || sunLines;
      be.render({ uniforms: u, clear: page, stress, lineMode, edges, background, meshes: meshes.map((m) => m.g), lines: allLines, shadow, glassShadow, ao: ao || sil, aoGround: ao, ground: gnd });
      if (afterRender) afterRender();
    }

    // ---- picking: cast a ray from the camera through the cursor and take the nearest triangle ----
    function eyePos() {
      return [
        cam.target[0] + cam.dist * Math.cos(cam.el) * Math.cos(cam.az),
        cam.target[1] + cam.dist * Math.cos(cam.el) * Math.sin(cam.az),
        cam.target[2] + cam.dist * Math.sin(cam.el),
      ];
    }
    function rayAt(sx, sy) {
      const eye = eyePos(), w = canvas.clientWidth || 1, h = canvas.clientHeight || 1;
      const norm = (a) => { const l = Math.hypot(...a) || 1; return [a[0] / l, a[1] / l, a[2] / l]; };
      const cross = (a, b) => [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]];
      const f = norm([cam.target[0] - eye[0], cam.target[1] - eye[1], cam.target[2] - eye[2]]);
      const right = norm(cross(f, [0, 0, 1])), up = cross(right, f);
      const t = Math.tan(0.45), x = (2 * sx / w - 1) * (w / h) * t, y = (1 - 2 * sy / h) * t;
      return { o: eye, d: norm([f[0] + right[0] * x + up[0] * y, f[1] + right[1] * x + up[1] * y, f[2] + right[2] * x + up[2] * y]) };
    }
    /** Möller-Trumbore over a triangle list of (pos, normal) vertices; returns the nearest hit distance or Infinity. */
    function hitMesh(m, o, d) {
      const b = m.bounds; // slab test on the bounding box first
      let t0 = 0, t1 = Infinity;
      for (let k = 0; k < 3; k++) {
        if (Math.abs(d[k]) < 1e-12) { if (o[k] < b[0][k] || o[k] > b[1][k]) return Infinity; continue; }
        let a = (b[0][k] - o[k]) / d[k], c = (b[1][k] - o[k]) / d[k];
        if (a > c) [a, c] = [c, a];
        t0 = Math.max(t0, a); t1 = Math.min(t1, c);
        if (t0 > t1) return Infinity;
      }
      const v = m.data; let best = Infinity;
      for (let i = 0; i + 17 < v.length; i += 18) {
        const e1 = [v[i + 6] - v[i], v[i + 7] - v[i + 1], v[i + 8] - v[i + 2]];
        const e2 = [v[i + 12] - v[i], v[i + 13] - v[i + 1], v[i + 14] - v[i + 2]];
        const p = [d[1] * e2[2] - d[2] * e2[1], d[2] * e2[0] - d[0] * e2[2], d[0] * e2[1] - d[1] * e2[0]];
        const det = e1[0] * p[0] + e1[1] * p[1] + e1[2] * p[2];
        if (Math.abs(det) < 1e-12) continue;
        const s = [o[0] - v[i], o[1] - v[i + 1], o[2] - v[i + 2]];
        const u = (s[0] * p[0] + s[1] * p[1] + s[2] * p[2]) / det;
        if (u < 0 || u > 1) continue;
        const q = [s[1] * e1[2] - s[2] * e1[1], s[2] * e1[0] - s[0] * e1[2], s[0] * e1[1] - s[1] * e1[0]];
        const w2 = (d[0] * q[0] + d[1] * q[1] + d[2] * q[2]) / det;
        if (w2 < 0 || u + w2 > 1) continue;
        const t = (e2[0] * q[0] + e2[1] * q[1] + e2[2] * q[2]) / det;
        if (t > 1e-6 && t < best) best = t;
      }
      return best;
    }
    /** Element id under a canvas pixel (CSS px), or null. With Select Elements by Face off, only an edge picks. */
    let byFace = true;
    function pick(sx, sy) {
      if (!byFace) return edgePick(sx, sy);
      const { o, d } = rayAt(sx, sy);
      let bestT = Infinity, bestId = null;
      for (const m of meshes) { const t = hitMesh(m, o, d); if (t < bestT) { bestT = t; bestId = m.id; } }
      return bestId;
    }
    function edgePick(sx, sy) {
      let best = 6, id = null;
      const seg = (a, b) => { const dx = b.x - a.x, dy = b.y - a.y, L = dx * dx + dy * dy, t = L ? Math.max(0, Math.min(1, ((sx - a.x) * dx + (sy - a.y) * dy) / L)) : 0; return Math.hypot(sx - a.x - t * dx, sy - a.y - t * dy); };
      for (const m of meshes) {
        const e = m.edges || [];
        for (let i = 0; i + 5 < e.length; i += 6) {
          const a = project([e[i], e[i + 1], e[i + 2]]), b = project([e[i + 3], e[i + 4], e[i + 5]]);
          if (!a || !b) continue;
          const dd = seg(a, b);
          if (dd < best) { best = dd; id = m.id; }
        }
      }
      return id;
    }

    // a screen point (CSS px) of a model point, or null behind the camera
    function project(p) {
      const m = lastVp;
      if (!m) return null;
      const x = m[0] * p[0] + m[4] * p[1] + m[8] * p[2] + m[12], y = m[1] * p[0] + m[5] * p[1] + m[9] * p[2] + m[13], w = m[3] * p[0] + m[7] * p[1] + m[11] * p[2] + m[15];
      if (w <= 1e-6) return null;
      return { x: (x / w * 0.5 + 0.5) * canvas.clientWidth, y: (0.5 - y / w * 0.5) * canvas.clientHeight };
    }
    // The elements a box takes: dragged left to right, those wholly inside; right to left, those it touches.
    function boxPick(x0, y0, x1, y1) {
      const [l, r, t, b] = [Math.min(x0, x1), Math.max(x0, x1), Math.min(y0, y1), Math.max(y0, y1)], window = x1 >= x0;
      const inside = new Map();
      for (const m of meshes) {
        const [lo, hi] = m.bounds, pts = [];
        for (const cx of [lo[0], hi[0]]) for (const cy of [lo[1], hi[1]]) for (const cz of [lo[2], hi[2]]) pts.push(project([cx, cy, cz]));
        if (pts.some((p) => !p)) { if (window) inside.set(m.id, false); continue; }
        const bx0 = Math.min(...pts.map((p) => p.x)), bx1 = Math.max(...pts.map((p) => p.x)), by0 = Math.min(...pts.map((p) => p.y)), by1 = Math.max(...pts.map((p) => p.y));
        const ok = window ? bx0 >= l && bx1 <= r && by0 >= t && by1 <= b : bx1 >= l && bx0 <= r && by1 >= t && by0 <= b;
        if (window) inside.set(m.id, (inside.get(m.id) ?? true) && ok); else if (ok) inside.set(m.id, true);
      }
      return [...inside].filter(([, ok]) => ok).map(([id]) => id);
    }

    // The mouse: by default the left button selects (a click, or a box when dragged), the right button orbits and
    // the wheel button pans; Shift with the wheel button orbits. The buttons can be swapped (setButtons).
    let buttons = { select: 0, orbit: 2, pan: 1 };
    try { const b = JSON.parse(localStorage.getItem("openarch.mouse3d") || "null"); if (b) buttons = { ...buttons, ...b }; } catch { /* storage can be unavailable */ }
    let drag = null, band = null;
    canvas.addEventListener("contextmenu", (e) => e.preventDefault());
    let pickHandler = null, clickHook = null, bandHandler = null, menuHandler = null;
    const bandBox = document.createElement("div");
    bandBox.className = "vp-band";
    bandBox.hidden = true;
    canvas.after(bandBox);
    canvas.addEventListener("pointerdown", (e) => {
      canvas.setPointerCapture(e.pointerId);
      const what = e.button === buttons.select && !e.shiftKey ? "select" : e.button === buttons.orbit || (e.button === buttons.pan && e.shiftKey) ? "orbit" : e.button === buttons.pan ? "pan" : "select";
      drag = { x: e.clientX, y: e.clientY, x0: e.clientX, y0: e.clientY, button: e.button, what, moved: false };
    });
    canvas.addEventListener("pointerup", (e) => {
      const r = canvas.getBoundingClientRect(), x = e.clientX - r.left, y = e.clientY - r.top;
      const still = drag && Math.hypot(e.clientX - drag.x0, e.clientY - drag.y0) < 4;
      const additive = e.shiftKey || e.ctrlKey || e.metaKey;
      if (drag && drag.what === "select" && still) {
        // a click that did not move is a pick; clicking empty space clears the selection.
        // A tool that wants the click (picking a point) takes it instead of the selection
        if (!(clickHook && clickHook(x, y)) && pickHandler) pickHandler(pick(x, y), additive);
      } else if (drag && drag.what === "select" && band) {
        if (bandHandler) bandHandler(boxPick(band.x0, band.y0, x, y), additive);
      } else if (drag && still && drag.button === 2 && menuHandler) {
        // a right click that did not move: the view's menu
        menuHandler(e.clientX, e.clientY);
      }
      band = null; bandBox.hidden = true;
      drag = null;
    });
    canvas.addEventListener("pointermove", (e) => {
      if (!drag) return;
      const dx = e.clientX - drag.x, dy = e.clientY - drag.y; drag.x = e.clientX; drag.y = e.clientY;
      if (drag.what === "select") {
        if (Math.hypot(e.clientX - drag.x0, e.clientY - drag.y0) < 4) return;
        const r = canvas.getBoundingClientRect();
        band = { x0: drag.x0 - r.left, y0: drag.y0 - r.top };
        const x = e.clientX - r.left, y = e.clientY - r.top;
        Object.assign(bandBox.style, { left: `${canvas.offsetLeft + Math.min(band.x0, x)}px`, top: `${canvas.offsetTop + Math.min(band.y0, y)}px`, width: `${Math.abs(x - band.x0)}px`, height: `${Math.abs(y - band.y0)}px` });
        bandBox.classList.toggle("crossing", x < band.x0);
        bandBox.hidden = false;
        return;
      }
      if (drag.what === "pan") {
        const s = cam.dist * 0.0016, ca = Math.cos(cam.az), sa = Math.sin(cam.az);
        cam.target[0] += (sa * dx - ca * dy * Math.sin(cam.el)) * s;
        cam.target[1] += (-ca * dx - sa * dy * Math.sin(cam.el)) * s;
        cam.target[2] += dy * Math.cos(cam.el) * s;
      } else {
        cam.az -= dx * 0.006;
        cam.el = Math.min(1.5, Math.max(-0.05, cam.el + dy * 0.006));
      }
      dirty = true;
    });
    canvas.addEventListener("wheel", (e) => { e.preventDefault(); cam.dist = Math.min(3000, Math.max(minDist, cam.dist * Math.exp(e.deltaY * 0.001))); dirty = true; }, { passive: false });
    new ResizeObserver(() => (dirty = true)).observe(canvas);
    requestAnimationFrame(frame);

    const TINT = { Wall: 0.0, Floor: 0.3, Roof: 0.15, Ceiling: 0.35, Stair: 0.25, Railing: 0.1, Ramp: 0.25, Door: 0.45, Window: 0.0, Glass: -1.0 };
    // how see-through a mesh is: its material's transparency (glass half), a selection at least 30%, and then the
    // view's Transparency (Graphic Display Options) over the whole model
    let graphics = {};
    // Sketchy Lines: each edge carried on past its ends by up to `extension` (0 to 10) and its ends moved off the
    // line by up to `jitter` (0 to 10), the same way every time for the same edge (seeded by where it is)
    const sketchKey = () => (graphics.sketchy ? `${graphics.sketchy.jitter}|${graphics.sketchy.extension}` : "");
    let shownSketch = "";
    function sketchify(e, jitter, extension) {
      const out = new Float32Array(e.length);
      const rnd = (x, y, z, k) => { const s = Math.sin(x * 12.9898 + y * 78.233 + z * 37.719 + k * 4.581) * 43758.5453; return (s - Math.floor(s)) * 2 - 1; };
      for (let i = 0; i + 5 < e.length; i += 6) {
        const a = [e[i], e[i + 1], e[i + 2]], b = [e[i + 3], e[i + 4], e[i + 5]];
        const d = [b[0] - a[0], b[1] - a[1], b[2] - a[2]], L = Math.hypot(...d) || 1, u = d.map((v) => v / L);
        const ext = Math.min(L * 0.25, extension * 0.025), amp = Math.min(L * 0.1, jitter * 0.004);
        const seed = a[0] + b[0] * 1.3, s2 = a[1] + b[1] * 1.7, s3 = a[2] + b[2] * 1.1;
        for (let k = 0; k < 3; k++) {
          out[i + k] = a[k] - u[k] * ext * (0.5 + 0.5 * Math.abs(rnd(seed, s2, s3, 1))) + amp * rnd(seed, s2, s3, 2 + k);
          out[i + 3 + k] = b[k] + u[k] * ext * (0.5 + 0.5 * Math.abs(rnd(seed, s2, s3, 5))) + amp * rnd(seed, s2, s3, 6 + k);
        }
      }
      return out;
    }
    // the meshes' edge buffers again, sketchy or plain, when Sketchy Lines changes
    function resketch() {
      const key = sketchKey();
      if (key === shownSketch) return;
      shownSketch = key;
      for (const m of meshes) {
        be.free(m.g);
        m.g = be.mesh(m.data, key ? sketchify(m.edges, graphics.sketchy.jitter, graphics.sketchy.extension) : m.edges);
        writeTint(m, !!m.selected);
      }
    }
    const writeTint = (m, selected) => {
      m.selected = selected;
      const t = 1 - (1 - Math.max(m.transparency, selected ? 0.3 : 0)) * (1 - (graphics.transparency || 0) / 100);
      m.g.alpha = 1 - t;
      m.g.solid = m.transparency < 0.001;
      be.instance(m.g, new Float32Array([0, 0, 0, selected ? 1.0 : m.color ? 0 : TINT[m.kind] ?? 0, 1, 1, 1, t, ...(m.color || [0, 0, 0]), m.color ? 1 : 0]));
    };

    return {
      info: be.info,
      /** "WebGPU" or "WebGL2" */
      backend: be.name,
      setStress(on) {
        stress = on;
        if (on) { cam.dist = 450; cam.el = 0.5; cam.target = [0, 0, 20]; } else { this.fit(); }
        dirty = true;
      },
      /** Lines drawn with the model, hidden where it covers them: a Float32Array of x y z r g b for each end (metres, colours 0 to 1), or null. */
      setLines(a) { extraLines = a && a.length ? a : null; allLines = null; dirty = true; },
      /** The sun path diagram's lines (x y z r g b for each end, metres), or null. */
      setSunLines(a) { sunLines = a && a.length ? a : null; allLines = null; dirty = true; },
      /** The view's sun, or null for the fixed light: see `sun` above. */
      setSun(s) { sun = s; dirty = true; },
      /** The view's Graphic Display Options the renderer follows: { showEdges, silhouettes, transparency (0-100), background ("none", "sky", "gradient"), sketchy ({ jitter, extension } or null) }. */
      setGraphics(g) { graphics = g || {}; resketch(); for (const m of meshes) writeTint(m, !!m.selected); dirty = true; },
      /** The model's bounding box, metres: [lo, hi]. */
      bounds() { return [sceneLo.slice(), sceneHi.slice()]; },
      /**
       * Replace the displayed model meshes: [{ id, kind, verts: [x y z nx ny nz ...], material, surface }]. `patterns`
       * holds the surface patterns' definitions by name (model mm), drawn on the faces in Hidden Line, Shaded and
       * Consistent Colours.
       */
      setMeshes(list, selected = [], patterns = {}) {
        for (const m of meshes) be.free(m.g);
        // one pattern buffer for each pattern and colour in use (a definition may have changed under its name)
        for (const g of patternGpu.values()) g.free();
        patternGpu = new Map();
        for (const m of list) {
          if (!m.surface) continue;
          const key = JSON.stringify(m.surface);
          if (!patternGpu.has(key)) patternGpu.set(key, be.pattern(patternData(m.surface, patterns[m.surface.pattern])));
        }
        const sel = new Set(selected);
        const datas = list.map((m) => new Float32Array(m.verts));
        // wall layers of one material join up, so a flush joint between them draws no line
        const edges = featureEdges(list.map((m, i) => ({ data: datas[i], group: m.material ? `material:${m.material}` : `mesh:${i}` })));
        meshes = list.map((m, i) => {
          const data = datas[i];
          const bounds = [[Infinity, Infinity, Infinity], [-Infinity, -Infinity, -Infinity]];
          for (let i = 0; i < data.length; i += 6) for (let k = 0; k < 3; k++) { bounds[0][k] = Math.min(bounds[0][k], data[i + k]); bounds[1][k] = Math.max(bounds[1][k], data[i + k]); }
          const out = { g: be.mesh(data, graphics.sketchy ? sketchify(edges[i], graphics.sketchy.jitter, graphics.sketchy.extension) : edges[i]), edges: edges[i], id: m.id, kind: m.kind, material: m.material || null, surfKey: m.surface ? JSON.stringify(m.surface) : null, data, bounds, color: m.color ? m.color.map((c) => c / 255) : null, transparency: m.transparency != null ? m.transparency / 100 : m.kind === "Glass" ? 0.5 : 0 };
          writeTint(out, sel.has(m.id));
          return out;
        });
        shownSketch = sketchKey();
        sceneLo = [Infinity, Infinity, Infinity]; sceneHi = [-Infinity, -Infinity, -Infinity];
        for (const m of meshes) for (let k = 0; k < 3; k++) { sceneLo[k] = Math.min(sceneLo[k], m.bounds[0][k]); sceneHi[k] = Math.max(sceneHi[k], m.bounds[1][k]); }
        if (!isFinite(sceneLo[0])) { sceneLo = [0, 0, 0]; sceneHi = [0, 0, 0]; }
        dirty = true;
      },
      /** The key of the maps held for a material (see setMaterial), or null. */
      materialKey(name) { return materials.get(name)?.key ?? null; },
      /** Hold a material's maps for Realistic: { key, albedo, surface, rough (mip chains of canvases), params } from Viewport.prepareMaterial, or null to drop them. */
      setMaterial(name, m) {
        const old = materials.get(name);
        if (old) { old.gpu.free(); materials.delete(name); }
        if (m) materials.set(name, { key: m.key, gpu: be.material(m) });
        dirty = true;
      },
      /** Drop the maps of materials not in `names`. */
      keepMaterials(names) { const keep = new Set(names); for (const n of [...materials.keys()]) if (!keep.has(n)) this.setMaterial(n, null); },
      /** How the surfaces are drawn: shaded (grey by the light), consistent_colors (flat material colours), realistic (material appearances), hidden_line or wireframe. */
      setStyle(st) { if (st !== style) { style = st; dirty = true; } },
      setSelection(ids) { const s = new Set(ids); for (const m of meshes) writeTint(m, s.has(m.id)); dirty = true; },
      /** Frame everything in the model (or reset to a default view when empty). */
      fit() {
        let lo = [Infinity, Infinity, Infinity], hi = [-Infinity, -Infinity, -Infinity];
        for (const m of meshes) for (let i = 0; i < m.data.length; i += 6) for (let k = 0; k < 3; k++) { lo[k] = Math.min(lo[k], m.data[i + k]); hi[k] = Math.max(hi[k], m.data[i + k]); }
        if (!isFinite(lo[0])) { cam.target = [3, 2, 1]; cam.dist = 14; }
        else {
          cam.target = [(lo[0] + hi[0]) / 2, (lo[1] + hi[1]) / 2, (lo[2] + hi[2]) / 2];
          cam.dist = Math.max(minFit, Math.hypot(hi[0] - lo[0], hi[1] - lo[1], hi[2] - lo[2]) * 1.2);
        }
        cam.az = -0.9; cam.el = 0.5;
        dirty = true;
      },
      poke() { dirty = true; },
      getCamera() { return JSON.parse(JSON.stringify(cam)); },
      setCameraState(c) { Object.assign(cam, JSON.parse(JSON.stringify(c))); dirty = true; },
      pick,
      /** Called with (elementId | null, additive) when the user clicks in the view. */
      onPick(fn) { pickHandler = fn; },
      onBand(fn) { bandHandler = fn; },
      setPickByFace(v) { byFace = !!v; },
      onMenu(fn) { menuHandler = fn; },
      get buttons() { return { ...buttons }; },
      setButtons(b) { buttons = { ...buttons, ...b }; try { localStorage.setItem("openarch.mouse3d", JSON.stringify(buttons)); } catch { /* storage can be unavailable */ } },
      /** Called with the canvas pixel of a click before the pick; returning true keeps the selection as it is. */
      onClick(fn) { clickHook = fn; },
      /** Called after each frame is drawn (to draw over the view in step with the camera). */
      onRender(fn) { afterRender = fn; },
      /** Where the line of sight through a canvas pixel meets the horizontal plane at height `z` (metres), or null. */
      groundAt(sx, sy, z) {
        const eye = eyePos(), t = cam.target, w = canvas.clientWidth, h = canvas.clientHeight;
        const sub = (a, b) => [a[0] - b[0], a[1] - b[1], a[2] - b[2]], len = (a) => Math.hypot(a[0], a[1], a[2]);
        const nrm = (a) => { const l = len(a) || 1; return [a[0] / l, a[1] / l, a[2] / l]; };
        const cross = (a, b) => [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]];
        const f = nrm(sub(t, eye)), r = nrm(cross(f, [0, 0, 1])), u = cross(r, f);
        const k = Math.tan(0.9 / 2), nx = (sx / w) * 2 - 1, ny = 1 - (sy / h) * 2;
        const d = [0, 1, 2].map((i) => f[i] + r[i] * nx * k * (w / h) + u[i] * ny * k);
        if (Math.abs(d[2]) < 1e-9) return null;
        const s = (z - eye[2]) / d[2];
        return s > 0 ? [eye[0] + d[0] * s, eye[1] + d[1] * s] : null;
      },
      /** Canvas pixel of a world point, for tests. */
      /** A function from a world point to its canvas pixel for the current camera, null behind the eye (for many points). */
      projector() {
        const eye = eyePos(), vp = mat.mul(mat.perspective(0.9, canvas.clientWidth / canvas.clientHeight, 0.1, 4000), mat.lookAt(eye, cam.target, [0, 0, 1]));
        const W = canvas.clientWidth, H = canvas.clientHeight;
        return (x0, y0, z0) => {
          const w = vp[3] * x0 + vp[7] * y0 + vp[11] * z0 + vp[15];
          if (w <= 1e-6) return null;
          const x = vp[0] * x0 + vp[4] * y0 + vp[8] * z0 + vp[12], y = vp[1] * x0 + vp[5] * y0 + vp[9] * z0 + vp[13];
          return { x: (x / w * 0.5 + 0.5) * W, y: (0.5 - y / w * 0.5) * H };
        };
      },
      project(p) {
        const eye = eyePos(), vp = mat.mul(mat.perspective(0.9, canvas.clientWidth / canvas.clientHeight, 0.1, 4000), mat.lookAt(eye, cam.target, [0, 0, 1]));
        const x = vp[0] * p[0] + vp[4] * p[1] + vp[8] * p[2] + vp[12], y = vp[1] * p[0] + vp[5] * p[1] + vp[9] * p[2] + vp[13], w = vp[3] * p[0] + vp[7] * p[1] + vp[11] * p[2] + vp[15];
        return { x: (x / w * 0.5 + 0.5) * canvas.clientWidth, y: (0.5 - y / w * 0.5) * canvas.clientHeight };
      },
      // Debug/test helper: read the next frame back as RGBA pixels, top row first.
      snapshot() { return new Promise((res) => { be.pending = res; dirty = true; }); },
      instances: () => (stress ? 1200 : meshes.length),
    };
  }

  return { init, mat, prepareMaterial };
})();
