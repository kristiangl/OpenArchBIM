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
struct U { vp: mat4x4<f32>, light: vec4<f32>, bg: vec4<f32>, edge: vec4<f32> };
@group(0) @binding(0) var<uniform> u: U;

struct VOut { @builtin(position) pos: vec4<f32>, @location(0) n: vec3<f32>, @location(1) tint: f32, @location(2) col: vec4<f32> };

@vertex fn vs_box(@location(0) p: vec3<f32>, @location(1) n: vec3<f32>,
                  @location(2) off: vec4<f32>, @location(3) sc: vec4<f32>, @location(4) col: vec4<f32>) -> VOut {
  var o: VOut;
  o.pos = u.vp * vec4<f32>(p * sc.xyz + off.xyz, 1.0);
  o.n = n; o.tint = off.w; o.col = col;
  return o;
}
@fragment fn fs_box(i: VOut) -> @location(0) vec4<f32> {
  if (u.light.w > 0.5) {
    // Hidden Line: every surface in the page colour, so the edges drawn over it are the only linework
    var c = u.bg.rgb;
    if (i.tint >= 0.99) { c = mix(c, vec3<f32>(0.85, 0.62, 0.45), 0.45); }
    if (i.tint < 0.0) { c = mix(c, vec3<f32>(0.55, 0.72, 0.86), 0.25); }
    return vec4<f32>(c, 1.0);
  }
  let l = max(dot(normalize(i.n), normalize(u.light.xyz)), 0.0);
  var base = mix(vec3<f32>(0.78, 0.80, 0.83), vec3<f32>(0.85, 0.62, 0.45), clamp(i.tint, 0.0, 1.0));
  if (i.col.w > 0.5 && i.tint < 0.99) { base = i.col.rgb; } // material colour unless selected
  if (i.tint < 0.0) { base = vec3<f32>(0.55, 0.72, 0.86); } // glass
  return vec4<f32>(base * (0.38 + 0.62 * l), 1.0);
}

struct LOut { @builtin(position) pos: vec4<f32>, @location(0) c: vec3<f32> };
@vertex fn vs_line(@location(0) p: vec3<f32>, @location(1) c: vec3<f32>) -> LOut {
  var o: LOut; o.pos = u.vp * vec4<f32>(p, 1.0); o.c = c; return o;
}
@fragment fn fs_line(i: LOut) -> @location(0) vec4<f32> { return vec4<f32>(i.c, 1.0); }

// edges of the model: the instance buffer carries the selection tint in its w
struct EOut { @builtin(position) pos: vec4<f32>, @location(0) tint: f32 };
@vertex fn vs_edge(@location(0) p: vec3<f32>, @location(2) off: vec4<f32>) -> EOut {
  var o: EOut; o.pos = u.vp * vec4<f32>(p, 1.0); o.tint = off.w; return o;
}
@fragment fn fs_edge(i: EOut) -> @location(0) vec4<f32> {
  if (i.tint >= 0.99) { return vec4<f32>(0.85, 0.35, 0.08, 1.0); }
  return vec4<f32>(u.edge.rgb, 1.0);
}
`;

  // The same shading in GLSL ES 3.00, for WebGL2
  const GL_UNIFORMS = "uniform mat4 vp; uniform vec4 light; uniform vec4 bg; uniform vec4 edge;";
  const GL_BOX_VS = `#version 300 es
${GL_UNIFORMS}
layout(location = 0) in vec3 p; layout(location = 1) in vec3 n;
layout(location = 2) in vec4 off; layout(location = 3) in vec4 sc; layout(location = 4) in vec4 col;
out vec3 vn; out float vtint; out vec4 vcol;
void main() { gl_Position = vp * vec4(p * sc.xyz + off.xyz, 1.0); vn = n; vtint = off.w; vcol = col; }`;
  const GL_BOX_FS = `#version 300 es
precision highp float;
${GL_UNIFORMS}
in vec3 vn; in float vtint; in vec4 vcol; out vec4 o;
void main() {
  if (light.w > 0.5) {
    vec3 c = bg.rgb;
    if (vtint >= 0.99) c = mix(c, vec3(0.85, 0.62, 0.45), 0.45);
    if (vtint < 0.0) c = mix(c, vec3(0.55, 0.72, 0.86), 0.25);
    o = vec4(c, 1.0); return;
  }
  float l = max(dot(normalize(vn), normalize(light.xyz)), 0.0);
  vec3 base = mix(vec3(0.78, 0.80, 0.83), vec3(0.85, 0.62, 0.45), clamp(vtint, 0.0, 1.0));
  if (vcol.w > 0.5 && vtint < 0.99) base = vcol.rgb;
  if (vtint < 0.0) base = vec3(0.55, 0.72, 0.86);
  o = vec4(base * (0.38 + 0.62 * l), 1.0);
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
void main() { gl_Position = vp * vec4(p, 1.0); vtint = off.w; }`;
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
  function featureEdges(v) {
    const q = (x) => Math.round(x * 1000);
    const map = new Map();
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
        if (!e) { e = { a, b, ns: [] }; map.set(key, e); }
        e.ns.push(n);
      }
    }
    const out = [];
    for (const e of map.values()) {
      const keep = e.ns.length !== 2 || e.ns[0][0] * e.ns[1][0] + e.ns[0][1] * e.ns[1][1] + e.ns[0][2] * e.ns[1][2] < 0.866;
      if (keep) out.push(...e.a, ...e.b);
    }
    return new Float32Array(out);
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
    // only now take the canvas, so a failed WebGPU start leaves it free for WebGL2
    const ctx = canvas.getContext("webgpu");
    const format = navigator.gpu.getPreferredCanvasFormat();
    ctx.configure({ device, format, alphaMode: "opaque", usage: GPUTextureUsage.RENDER_ATTACHMENT | GPUTextureUsage.COPY_SRC });
    const info = adapter.info || {};

    const module = device.createShaderModule({ code: SHADER });
    const ubuf = device.createBuffer({ size: 112, usage: GPUBufferUsage.UNIFORM | GPUBufferUsage.COPY_DST });
    const bgl = device.createBindGroupLayout({ entries: [{ binding: 0, visibility: GPUShaderStage.VERTEX | GPUShaderStage.FRAGMENT, buffer: {} }] });
    const bind = device.createBindGroup({ layout: bgl, entries: [{ binding: 0, resource: { buffer: ubuf } }] });
    const layout = device.createPipelineLayout({ bindGroupLayouts: [bgl] });
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
    let depth = null;
    const be = {
      name: "WebGPU",
      info: [info.vendor, info.architecture, info.description].filter(Boolean).join(" ") || "adapter ok",
      gpuMs: 0,
      resize(w, h) { depth?.destroy(); depth = device.createTexture({ size: [w, h], format: depthFormat, usage: GPUTextureUsage.RENDER_ATTACHMENT }); },
      mesh(data, edges) {
        return { vbuf: mk(data, GPUBufferUsage.VERTEX), ebuf: edges.length ? mk(edges, GPUBufferUsage.VERTEX) : null, ibuf: device.createBuffer({ size: 48, usage: GPUBufferUsage.VERTEX | GPUBufferUsage.COPY_DST }), count: data.length / 6, ecount: edges.length / 3 };
      },
      free(g) { g.vbuf.destroy(); g.ibuf.destroy(); g.ebuf?.destroy(); },
      instance(g, inst) { device.queue.writeBuffer(g.ibuf, 0, inst); },
      render(f) {
        device.queue.writeBuffer(ubuf, 0, f.uniforms);
        const enc = device.createCommandEncoder();
        const tex = ctx.getCurrentTexture();
        const pass = enc.beginRenderPass({
          colorAttachments: [{ view: tex.createView(), loadOp: "clear", storeOp: "store", clearValue: { r: f.clear[0], g: f.clear[1], b: f.clear[2], a: 1 } }],
          depthStencilAttachment: { view: depth.createView(), depthClearValue: 1, depthLoadOp: "clear", depthStoreOp: "store" },
        });
        pass.setBindGroup(0, bind);
        const n = clipLines(f.stress ? st.bigGrid : st.grid, f.uniforms.subarray(0, 16), clipped, true);
        if (n) { device.queue.writeBuffer(gridBuf, 0, clipped, 0, n * 6); pass.setPipeline(linePipe); pass.setVertexBuffer(0, gridBuf); pass.draw(n); }
        if (f.stress) {
          pass.setPipeline(boxPipe);
          pass.setVertexBuffer(0, boxBuf); pass.setVertexBuffer(1, stressBuf); pass.draw(36, st.stress.length / 12);
        } else {
          if (f.lineMode !== 2) {
            pass.setPipeline(f.lineMode === 1 ? boxBiasPipe : boxPipe);
            for (const g of f.meshes) { pass.setVertexBuffer(0, g.vbuf); pass.setVertexBuffer(1, g.ibuf); pass.draw(g.count, 1); }
          }
          if (f.lineMode) {
            pass.setPipeline(f.lineMode === 2 ? wirePipe : edgePipe);
            for (const g of f.meshes) { if (!g.ecount) continue; pass.setVertexBuffer(0, g.ebuf); pass.setVertexBuffer(1, g.ibuf); pass.draw(g.ecount, 1); }
          }
        }
        pass.end();
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
      return { p, vp: gl.getUniformLocation(p, "vp"), light: gl.getUniformLocation(p, "light"), bg: gl.getUniformLocation(p, "bg"), edge: gl.getUniformLocation(p, "edge") };
    };
    const boxProg = program(GL_BOX_VS, GL_BOX_FS), lineProg = program(GL_LINE_VS, GL_LINE_FS), edgeProg = program(GL_EDGE_VS, GL_EDGE_FS);
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
    const stressVao = vao(buf(st.box), 24, [[0, 3, 0], [1, 3, 12]], buf(st.stress));
    const dbg = gl.getExtension("WEBGL_debug_renderer_info");
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
      render(f) {
        const t0 = performance.now();
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
        };
        gl.enable(gl.DEPTH_TEST); gl.depthFunc(gl.LESS);
        gl.disable(gl.CULL_FACE); gl.disable(gl.POLYGON_OFFSET_FILL);
        use(lineProg);
        const n = clipLines(f.stress ? st.bigGrid : st.grid, vp, clipped, false);
        gl.bindBuffer(gl.ARRAY_BUFFER, gridDyn);
        gl.bufferData(gl.ARRAY_BUFFER, clipped.subarray(0, n * 6), gl.DYNAMIC_DRAW);
        gl.bindVertexArray(gridDynVao);
        gl.drawArrays(gl.LINES, 0, n);
        gl.enable(gl.CULL_FACE); gl.cullFace(gl.BACK); gl.frontFace(gl.CCW);
        if (f.stress) {
          use(boxProg);
          gl.bindVertexArray(stressVao);
          gl.drawArraysInstanced(gl.TRIANGLES, 0, 36, st.stress.length / 12);
        } else {
          if (f.lineMode !== 2) {
            use(boxProg);
            if (f.lineMode === 1) { gl.enable(gl.POLYGON_OFFSET_FILL); gl.polygonOffset(2, 4); }
            for (const g of f.meshes) { gl.bindVertexArray(g.tri); gl.drawArraysInstanced(gl.TRIANGLES, 0, g.count, 1); }
            gl.disable(gl.POLYGON_OFFSET_FILL);
          }
          if (f.lineMode) {
            use(edgeProg);
            if (f.lineMode === 2) { gl.depthMask(false); gl.depthFunc(gl.ALWAYS); } else gl.depthFunc(gl.LEQUAL);
            for (const g of f.meshes) { if (!g.lines) continue; gl.bindVertexArray(g.lines); gl.drawArraysInstanced(gl.LINES, 0, g.ecount, 1); }
            gl.depthMask(true); gl.depthFunc(gl.LESS);
          }
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
  async function init(canvas, onFrame) {
    let want = null;
    try { want = new URLSearchParams(location.search).get("renderer") || localStorage.getItem("openarch.renderer"); } catch { /* storage can be unavailable */ }
    let be = null, why = "";
    if (want !== "webgl2") {
      try { be = await webgpuBackend(canvas); } catch (e) { why = String(e.message || e); }
    }
    if (!be) be = webgl2Backend(canvas);
    if (why) console.info(`3D view: WebGPU not used (${why}); drawing with WebGL2.`);

    let meshes = []; // { g (backend handles), id, kind, data, bounds, color }
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
      const dark = matchMedia("(prefers-color-scheme: dark)").matches;
      const page = dark ? [0.12, 0.14, 0.16] : [0.98, 0.98, 0.99];
      const lineMode = !stress && style === "hidden_line" ? 1 : !stress && style === "wireframe" ? 2 : 0;
      const u = new Float32Array(28); u.set(vp, 0); u.set([0.4, -0.5, 0.8, lineMode === 1 ? 1 : 0], 16); u.set([...page, 1], 20);
      u.set(dark ? [0.88, 0.9, 0.92, 1] : [0.08, 0.09, 0.1, 1], 24);
      be.render({ uniforms: u, clear: page, stress, lineMode, meshes: meshes.map((m) => m.g) });
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
    /** Element id under a canvas pixel (CSS px), or null. */
    function pick(sx, sy) {
      const { o, d } = rayAt(sx, sy);
      let bestT = Infinity, bestId = null;
      for (const m of meshes) { const t = hitMesh(m, o, d); if (t < bestT) { bestT = t; bestId = m.id; } }
      return bestId;
    }

    // orbit / pan / zoom
    let drag = null;
    canvas.addEventListener("contextmenu", (e) => e.preventDefault());
    let pickHandler = null;
    canvas.addEventListener("pointerdown", (e) => { canvas.setPointerCapture(e.pointerId); drag = { x: e.clientX, y: e.clientY, x0: e.clientX, y0: e.clientY, button: e.button, pan: e.button !== 0 || e.shiftKey }; });
    canvas.addEventListener("pointerup", (e) => {
      // a left click that did not move is a pick; clicking empty space clears the selection
      if (drag && drag.button === 0 && Math.hypot(e.clientX - drag.x0, e.clientY - drag.y0) < 4 && pickHandler) {
        const r = canvas.getBoundingClientRect();
        pickHandler(pick(e.clientX - r.left, e.clientY - r.top), e.shiftKey || e.ctrlKey || e.metaKey);
      }
      drag = null;
    });
    canvas.addEventListener("pointermove", (e) => {
      if (!drag) return;
      const dx = e.clientX - drag.x, dy = e.clientY - drag.y; drag.x = e.clientX; drag.y = e.clientY;
      if (drag.pan) {
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
    canvas.addEventListener("wheel", (e) => { e.preventDefault(); cam.dist = Math.min(3000, Math.max(2, cam.dist * Math.exp(e.deltaY * 0.001))); dirty = true; }, { passive: false });
    new ResizeObserver(() => (dirty = true)).observe(canvas);
    requestAnimationFrame(frame);

    const TINT = { Wall: 0.0, Floor: 0.3, Roof: 0.15, Door: 0.45, Window: 0.0, Glass: -1.0 };
    const writeTint = (m, selected) => be.instance(m.g, new Float32Array([0, 0, 0, selected ? 1.0 : m.color ? 0 : TINT[m.kind] ?? 0, 1, 1, 1, 0, ...(m.color || [0, 0, 0]), m.color ? 1 : 0]));

    return {
      info: be.info,
      /** "WebGPU" or "WebGL2" */
      backend: be.name,
      setStress(on) {
        stress = on;
        if (on) { cam.dist = 450; cam.el = 0.5; cam.target = [0, 0, 20]; } else { this.fit(); }
        dirty = true;
      },
      /** Replace the displayed model meshes: [{ id, kind, verts: [x y z nx ny nz ...] }]. */
      setMeshes(list, selected = []) {
        for (const m of meshes) be.free(m.g);
        const sel = new Set(selected);
        meshes = list.map((m) => {
          const data = new Float32Array(m.verts);
          const bounds = [[Infinity, Infinity, Infinity], [-Infinity, -Infinity, -Infinity]];
          for (let i = 0; i < data.length; i += 6) for (let k = 0; k < 3; k++) { bounds[0][k] = Math.min(bounds[0][k], data[i + k]); bounds[1][k] = Math.max(bounds[1][k], data[i + k]); }
          const out = { g: be.mesh(data, featureEdges(data)), id: m.id, kind: m.kind, data, bounds, color: m.color ? m.color.map((c) => c / 255) : null };
          writeTint(out, sel.has(m.id));
          return out;
        });
        dirty = true;
      },
      /** How the surfaces are drawn: shaded (also used for Consistent Colours and Realistic for now), hidden_line or wireframe. */
      setStyle(st) { if (st !== style) { style = st; dirty = true; } },
      setSelection(ids) { const s = new Set(ids); for (const m of meshes) writeTint(m, s.has(m.id)); dirty = true; },
      /** Frame everything in the model (or reset to a default view when empty). */
      fit() {
        let lo = [Infinity, Infinity, Infinity], hi = [-Infinity, -Infinity, -Infinity];
        for (const m of meshes) for (let i = 0; i < m.data.length; i += 6) for (let k = 0; k < 3; k++) { lo[k] = Math.min(lo[k], m.data[i + k]); hi[k] = Math.max(hi[k], m.data[i + k]); }
        if (!isFinite(lo[0])) { cam.target = [3, 2, 1]; cam.dist = 14; }
        else {
          cam.target = [(lo[0] + hi[0]) / 2, (lo[1] + hi[1]) / 2, (lo[2] + hi[2]) / 2];
          cam.dist = Math.max(6, Math.hypot(hi[0] - lo[0], hi[1] - lo[1], hi[2] - lo[2]) * 1.2);
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
      /** Canvas pixel of a world point, for tests. */
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

  return { init, mat };
})();
