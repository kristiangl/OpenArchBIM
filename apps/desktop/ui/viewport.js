// WebGPU viewport spike: orbit camera, ground grid, instanced boxes.
// Z-up world, metres. The stress mode draws 1200 apartment-sized boxes with
// one instanced draw call to check frame rate on the machine it runs on.

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

  // instance = offset.xyz + tint, scale.xyz + pad  (8 floats)
  function stressInstances() {
    const I = [];
    for (let t = 0; t < 15; t++) {
      const tx = (t % 5) * 90 - 180, ty = Math.floor(t / 5) * 90 - 90;
      for (let l = 0; l < 10; l++) for (let s = 0; s < 8; s++) {
        const kind = (t + l + s) % 4;
        I.push(tx + (s % 4) * 11, ty + Math.floor(s / 4) * 9, l * 3.2, kind / 3, 10, 8, 3, 0);
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

  async function init(canvas, onFrame) {
    if (!navigator.gpu) throw new Error("navigator.gpu is not available in this webview");
    const adapter = await navigator.gpu.requestAdapter();
    if (!adapter) throw new Error("no GPU adapter found");
    const device = await adapter.requestDevice();
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

    const boxPipe = device.createRenderPipeline({
      layout,
      vertex: { module, entryPoint: "vs_box", buffers: [
        { arrayStride: 24, attributes: [{ shaderLocation: 0, offset: 0, format: "float32x3" }, { shaderLocation: 1, offset: 12, format: "float32x3" }] },
        { arrayStride: 48, stepMode: "instance", attributes: [{ shaderLocation: 2, offset: 0, format: "float32x4" }, { shaderLocation: 3, offset: 16, format: "float32x4" }, { shaderLocation: 4, offset: 32, format: "float32x4" }] },
      ] },
      fragment: { module, entryPoint: "fs_box", targets: [{ format }] },
      primitive: { topology: "triangle-list", cullMode: "back" },
      depthStencil: { format: depthFormat, depthWriteEnabled: true, depthCompare: "less" },
    });
    // Hidden Line draws faces a little behind where they are, so the edges over them are not lost to depth fighting
    const boxBiasPipe = device.createRenderPipeline({
      layout,
      vertex: { module, entryPoint: "vs_box", buffers: [
        { arrayStride: 24, attributes: [{ shaderLocation: 0, offset: 0, format: "float32x3" }, { shaderLocation: 1, offset: 12, format: "float32x3" }] },
        { arrayStride: 48, stepMode: "instance", attributes: [{ shaderLocation: 2, offset: 0, format: "float32x4" }, { shaderLocation: 3, offset: 16, format: "float32x4" }, { shaderLocation: 4, offset: 32, format: "float32x4" }] },
      ] },
      fragment: { module, entryPoint: "fs_box", targets: [{ format }] },
      primitive: { topology: "triangle-list", cullMode: "back" },
      depthStencil: { format: depthFormat, depthWriteEnabled: true, depthCompare: "less", depthBias: 4, depthBiasSlopeScale: 2 },
    });
    const edgeBuffers = [
      { arrayStride: 12, attributes: [{ shaderLocation: 0, offset: 0, format: "float32x3" }] },
      { arrayStride: 48, stepMode: "instance", attributes: [{ shaderLocation: 2, offset: 0, format: "float32x4" }] },
    ];
    const edgePipe = device.createRenderPipeline({
      layout,
      vertex: { module, entryPoint: "vs_edge", buffers: edgeBuffers },
      fragment: { module, entryPoint: "fs_edge", targets: [{ format }] },
      primitive: { topology: "line-list" },
      depthStencil: { format: depthFormat, depthWriteEnabled: true, depthCompare: "less-equal" },
    });
    const wirePipe = device.createRenderPipeline({
      layout,
      vertex: { module, entryPoint: "vs_edge", buffers: edgeBuffers },
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
      const b = device.createBuffer({ size: data.byteLength, usage: usage | GPUBufferUsage.COPY_DST });
      device.queue.writeBuffer(b, 0, data);
      return b;
    };
    const boxBuf = mk(boxVerts(), GPUBufferUsage.VERTEX);
    const grid = gridVerts(60, 1, 10), bigGrid = gridVerts(400, 10, 5);
    const gridBuf = mk(grid, GPUBufferUsage.VERTEX), bigGridBuf = mk(bigGrid, GPUBufferUsage.VERTEX);
    const stressData = stressInstances();
    // stress instances are 8 floats each; the pipeline reads 12, so pad with an unused colour
    const stressPadded = new Float32Array((stressData.length / 8) * 12);
    for (let i = 0; i < stressData.length / 8; i++) stressPadded.set(stressData.subarray(i * 8, i * 8 + 8), i * 12);
    const stressBuf = mk(stressPadded, GPUBufferUsage.VERTEX);
    let meshes = []; // { vbuf, ibuf, count, id, kind }
    let gpuMs = 0;

    const cam = { az: -0.9, el: 0.5, dist: 38, target: [0, 0, 1.5] };
    let style = "shaded", snapReq = null, stress = false, dirty = true, depth = null, last = performance.now(), frames = 0, acc = 0, fps = 0;

    function resize() {
      const dpr = window.devicePixelRatio || 1;
      const w = Math.max(1, Math.floor(canvas.clientWidth * dpr)), h = Math.max(1, Math.floor(canvas.clientHeight * dpr));
      if (canvas.width !== w || canvas.height !== h || !depth) {
        canvas.width = w; canvas.height = h;
        depth?.destroy();
        depth = device.createTexture({ size: [w, h], format: depthFormat, usage: GPUTextureUsage.RENDER_ATTACHMENT });
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
      if (acc >= 500) { fps = (frames * 1000) / acc; frames = 0; acc = 0; if (stress) onFrame?.(fps, gpuMs); }

      const eye = [
        cam.target[0] + cam.dist * Math.cos(cam.el) * Math.cos(cam.az),
        cam.target[1] + cam.dist * Math.cos(cam.el) * Math.sin(cam.az),
        cam.target[2] + cam.dist * Math.sin(cam.el),
      ];
      const aspect = canvas.width / canvas.height;
      const vp = mat.mul(mat.perspective(0.9, aspect, 0.1, 4000), mat.lookAt(eye, cam.target, [0, 0, 1]));
      const dark = matchMedia("(prefers-color-scheme: dark)").matches;
      const page = dark ? [0.12, 0.14, 0.16] : [0.98, 0.98, 0.99];
      const lineMode = !stress && style === "hidden_line" ? 1 : !stress && style === "wireframe" ? 2 : 0;
      const u = new Float32Array(28); u.set(vp, 0); u.set([0.4, -0.5, 0.8, lineMode === 1 ? 1 : 0], 16); u.set([...page, 1], 20);
      u.set(dark ? [0.88, 0.9, 0.92, 1] : [0.08, 0.09, 0.1, 1], 24);
      device.queue.writeBuffer(ubuf, 0, u);

      const enc = device.createCommandEncoder();
      const pass = enc.beginRenderPass({
        colorAttachments: [{ view: ctx.getCurrentTexture().createView(), loadOp: "clear", storeOp: "store",
          clearValue: dark ? { r: 0.12, g: 0.14, b: 0.16, a: 1 } : { r: 0.98, g: 0.98, b: 0.99, a: 1 } }],
        depthStencilAttachment: { view: depth.createView(), depthClearValue: 1, depthLoadOp: "clear", depthStoreOp: "store" },
      });
      pass.setBindGroup(0, bind);
      pass.setPipeline(linePipe); pass.setVertexBuffer(0, stress ? bigGridBuf : gridBuf); pass.draw((stress ? bigGrid : grid).length / 6);
      if (stress) {
        pass.setPipeline(boxPipe);
        pass.setVertexBuffer(0, boxBuf); pass.setVertexBuffer(1, stressBuf); pass.draw(36, stressData.length / 8);
      } else {
        if (lineMode !== 2) {
          pass.setPipeline(lineMode === 1 ? boxBiasPipe : boxPipe);
          for (const m of meshes) { pass.setVertexBuffer(0, m.vbuf); pass.setVertexBuffer(1, m.ibuf); pass.draw(m.count, 1); }
        }
        if (lineMode) {
          pass.setPipeline(lineMode === 2 ? wirePipe : edgePipe);
          for (const m of meshes) { if (!m.ecount) continue; pass.setVertexBuffer(0, m.ebuf); pass.setVertexBuffer(1, m.ibuf); pass.draw(m.ecount, 1); }
        }
      }
      pass.end();
      let snapBuf = null, bpr = 0;
      if (snapReq) {
        bpr = Math.ceil((canvas.width * 4) / 256) * 256;
        snapBuf = device.createBuffer({ size: bpr * canvas.height, usage: GPUBufferUsage.COPY_DST | GPUBufferUsage.MAP_READ });
        enc.copyTextureToBuffer({ texture: ctx.getCurrentTexture() }, { buffer: snapBuf, bytesPerRow: bpr }, [canvas.width, canvas.height]);
      }
      const t0 = performance.now();
      device.queue.submit([enc.finish()]);
      device.queue.onSubmittedWorkDone().then(() => { gpuMs = performance.now() - t0; }, () => {});
      if (snapReq) {
        const req = snapReq, w = canvas.width, h = canvas.height, bgra = format.startsWith("bgra");
        snapReq = null;
        snapBuf.mapAsync(GPUMapMode.READ).then(() => {
          const src = new Uint8Array(snapBuf.getMappedRange()), out = new Uint8ClampedArray(w * h * 4);
          for (let y = 0; y < h; y++) for (let x = 0; x < w; x++) {
            const si = y * bpr + x * 4, di = (y * w + x) * 4;
            out[di] = src[si + (bgra ? 2 : 0)]; out[di + 1] = src[si + 1]; out[di + 2] = src[si + (bgra ? 0 : 2)]; out[di + 3] = 255;
          }
          snapBuf.unmap();
          req({ w, h, data: out });
        });
      }
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
    const writeTint = (m, selected) => device.queue.writeBuffer(m.ibuf, 0, new Float32Array([0, 0, 0, selected ? 1.0 : m.color ? 0 : TINT[m.kind] ?? 0, 1, 1, 1, 0, ...(m.color || [0, 0, 0]), m.color ? 1 : 0]));

    return {
      info: [info.vendor, info.architecture, info.description].filter(Boolean).join(" ") || "adapter ok",
      setStress(on) {
        stress = on;
        if (on) { cam.dist = 450; cam.el = 0.5; cam.target = [0, 0, 20]; } else { this.fit(); }
        dirty = true;
      },
      /** Replace the displayed model meshes: [{ id, kind, verts: [x y z nx ny nz ...] }]. */
      setMeshes(list, selected = []) {
        for (const m of meshes) { m.vbuf.destroy(); m.ibuf.destroy(); m.ebuf?.destroy(); }
        const sel = new Set(selected);
        meshes = list.map((m) => {
          const data = new Float32Array(m.verts);
          const vbuf = mk(data, GPUBufferUsage.VERTEX);
          const ibuf = device.createBuffer({ size: 48, usage: GPUBufferUsage.VERTEX | GPUBufferUsage.COPY_DST });
          const bounds = [[Infinity, Infinity, Infinity], [-Infinity, -Infinity, -Infinity]];
          for (let i = 0; i < data.length; i += 6) for (let k = 0; k < 3; k++) { bounds[0][k] = Math.min(bounds[0][k], data[i + k]); bounds[1][k] = Math.max(bounds[1][k], data[i + k]); }
          const edges = featureEdges(data);
          const ebuf = edges.length ? mk(edges, GPUBufferUsage.VERTEX) : null;
          const out = { vbuf, ibuf, ebuf, ecount: edges.length / 3, count: data.length / 6, id: m.id, kind: m.kind, data, bounds, color: m.color ? m.color.map((c) => c / 255) : null };
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
      // Debug/test helper: read the next frame back as RGBA pixels.
      snapshot() { return new Promise((res) => { snapReq = res; dirty = true; }); },
      instances: () => (stress ? stressData.length / 8 : meshes.length),
    };
  }

  return { init, mat };
})();
