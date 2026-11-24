// ARUS 2026 SOTA Global 3D Flood Simulation Studio Client
// Driven by real hydr 0.1.0 binary Float32 frames (/api/v1/runs/{id}/grid & frames.bin).

const PRESETS = [
  { id: 'shah-alam', name: 'Shah Alam / Taman Sri Muda (MY)', lon: 101.535, lat: 3.031, zoom: 14.4 },
  { id: 'kl', name: 'Kuala Lumpur / Sg. Klang (MY)', lon: 101.696, lat: 3.149, zoom: 14.4 },
  { id: 'kota-bharu', name: 'Kota Bharu / Sg. Kelantan (MY)', lon: 102.238, lat: 6.125, zoom: 14.0 },
  { id: 'jakarta', name: 'Jakarta / Ciliwung (ID)', lon: 106.845, lat: -6.208, zoom: 14.2 },
  { id: 'rotterdam', name: 'Rotterdam / Nieuwe Maas (NL)', lon: 4.477, lat: 51.924, zoom: 14.0 },
  { id: 'houston', name: 'Houston / Buffalo Bayou (US)', lon: -95.369, lat: 29.760, zoom: 14.0 },
];

const state = {
  center: { lon: 101.535, lat: 3.031 },
  tool: 'inspect', // inspect | pick | bund | pond | xsection
  layer: 'depth',  // depth | velocity | hazard | diff
  envelopeMode: false,
  particlesOn: true,
  is3D: true,
  publicMode: false,
  playing: true,
  speed: 1,
  frameIdx: 0,
  lastTickMs: 0,
  runId: null,
  grid: null,      // RunGridResponse JSON
  frames: [],      // [{ time_s, depth: Float32Array, vx: Float32Array, vy: Float32Array }]
  diffData: null,  // CompareSimulationsResponse
  interventions: [], // [{ col, row, delta_m, reason }]
  xsecPts: [],     // [{ col, row }]
  selectedCell: null, // { col, row }
  uploadedDem: null, // { ascii_content, projected_crs, vertical_datum, wgs84_bounds }
  cachedDem: null,   // { key, elevations }
  particles: [],
};

// Offscreen high-resolution canvas rendered smoothly with bilinear interpolation
// and pushed to MapLibre via ImageSource.updateImage (zero WebGL CanvasSource tile artifacts).
const rasterCanvas = document.createElement('canvas');
rasterCanvas.width = 256;
rasterCanvas.height = 256;
const rasterCtx = rasterCanvas.getContext('2d');
let lastRasterKey = '';

let map = null;
let layersReady = false;
let initialSimTriggered = false;

function $(id) { return document.getElementById(id); }

function buildOfflineStyle() {
  return {
    version: 8,
    projection: { type: 'globe' },
    sources: {},
    layers: [
      { id: 'bg', type: 'background', paint: { 'background-color': '#07101f' } },
    ],
  };
}

function initMap() {
  const onlineStyle = 'https://tiles.openfreemap.org/styles/dark';
  map = new maplibregl.Map({
    container: 'map',
    style: onlineStyle,
    center: [state.center.lon, state.center.lat],
    zoom: 14.1,
    pitch: 56,
    bearing: -20,
    antialias: true,
  });

  map.addControl(new maplibregl.ScaleControl({ maxWidth: 110, unit: 'metric' }), 'bottom-left');
  map.addControl(new maplibregl.NavigationControl({ visualizePitch: true }), 'top-right');

  let fellBack = false;
  map.on('error', (e) => {
    // Only fall back to offline blank style if the base style itself fails, never on a single DEM tile miss.
    if (!fellBack && !map.isStyleLoaded() && (!e || !e.sourceId)) {
      fellBack = true;
      map.setStyle(buildOfflineStyle());
    }
  });

  map.on('style.load', () => {
    try {
      if (typeof map.setProjection === 'function') {
        map.setProjection({ type: 'globe' });
      }
    } catch (_) { /* projection unsupported */ }
    ensureMapLayers();
    if (state.grid) {
      updateDomainOutline(state.grid.metadata.wgs84_bounds);
      renderFloodCanvas(true);
    }
    if (!initialSimTriggered) {
      initialSimTriggered = true;
      runSimulation(false);
    }
  });

  setTimeout(() => {
    if (!initialSimTriggered) {
      initialSimTriggered = true;
      runSimulation(false);
    }
  }, 2400);

  map.on('mousemove', (e) => handleHover(e.lngLat.lng, e.lngLat.lat));
  map.on('click', (e) => handleMapClick(e.lngLat.lng, e.lngLat.lat));
}

function ensureMapLayers() {
  if (!map || !map.isStyleLoaded() || map.getSource('arus-particles')) return;
  const empty = { type: 'FeatureCollection', features: [] };

  map.addSource('arus-particles', { type: 'geojson', data: empty });
  map.addLayer({
    id: 'arus-particles-line',
    type: 'line',
    source: 'arus-particles',
    paint: {
      'line-color': ['get', 'color'],
      'line-width': 2.2,
      'line-opacity': ['get', 'opacity'],
    },
    layout: { 'line-cap': 'round' },
  });

  map.addSource('arus-overlays', { type: 'geojson', data: empty });
  map.addLayer({
    id: 'arus-overlays-line',
    type: 'line',
    source: 'arus-overlays',
    paint: {
      'line-color': ['get', 'color'],
      'line-width': 3,
      'line-dasharray': [2, 1.5],
    },
  });

  map.addSource('arus-domain-box', { type: 'geojson', data: empty });
  map.addLayer({
    id: 'arus-domain-outline',
    type: 'line',
    source: 'arus-domain-box',
    paint: { 'line-color': '#38bdf8', 'line-width': 1.8, 'line-opacity': 0.75, 'line-dasharray': [4, 3] },
  });

  layersReady = true;
}

function updateDomainOutline(bounds) {
  if (!map || !layersReady) return;
  const [minLon, minLat, maxLon, maxLat] = bounds;
  map.getSource('arus-domain-box').setData({
    type: 'Feature',
    geometry: {
      type: 'Polygon',
      coordinates: [[[minLon, minLat], [maxLon, minLat], [maxLon, maxLat], [minLon, maxLat], [minLon, minLat]]],
    },
  });
}

function attachFloodCanvasSource(bounds) {
  ensureMapLayers();
  updateDomainOutline(bounds);
  lastRasterKey = '';
}

function decodeFramesBin(buffer) {
  const view = new DataView(buffer);
  const magic = String.fromCharCode(...new Uint8Array(buffer, 0, 8));
  if (magic !== 'ARUSFRM1') throw new Error('Invalid ARUSFRM1 binary header');
  const cols = view.getUint32(8, true);
  const rows = view.getUint32(12, true);
  const count = view.getUint32(16, true);
  const n = cols * rows;
  const frames = [];
  let offset = 20;
  for (let f = 0; f < count; f++) {
    const time_s = view.getFloat32(offset, true);
    offset += 4;
    const depth = new Float32Array(buffer, offset, n);
    offset += n * 4;
    const vx = new Float32Array(buffer, offset, n);
    offset += n * 4;
    const vy = new Float32Array(buffer, offset, n);
    offset += n * 4;
    frames.push({ time_s, depth, vx, vy });
  }
  return frames;
}

function seedParticles() {
  state.particles = [];
  for (let i = 0; i < 420; i++) {
    state.particles.push({
      x: Math.random(),
      y: Math.random(),
      age: Math.floor(Math.random() * 60),
      maxAge: 45 + Math.floor(Math.random() * 55),
    });
  }
}

function colorForDepth(h, thresh) {
  if (h < 0.005) return null;
  const t = Math.min(1, h / Math.max(thresh * 2.2, 0.22));
  if (t < 0.35) {
    const k = t / 0.35;
    return [14 + k * 42, 165 + k * 35, 233 + k * 22, 155 + k * 60];
  }
  if (t < 0.7) {
    const k = (t - 0.35) / 0.35;
    return [56 + k * 189, 200 - k * 42, 255 - k * 244, 215 + k * 25];
  }
  const k = (t - 0.7) / 0.3;
  return [245 - k * 1, 158 - k * 95, 11 + k * 83, 242];
}

function colorForVelocity(v) {
  if (v < 0.02) return null;
  const t = Math.min(1, v / 1.5);
  return [45 + t * 200, 212 - t * 140, 191 - t * 90, 185 + t * 55];
}

// DEFRA FD2320 Flood Hazard: HR = h * (v + 0.5)
function colorForHazard(hr) {
  if (hr < 0.02) return null;
  if (hr < 0.75) return [56, 189, 248, 175];   // Low
  if (hr < 1.25) return [250, 204, 21, 210];   // Moderate
  if (hr < 2.0)  return [249, 115, 22, 230];   // Significant
  return [244, 63, 94, 245];                   // Extreme
}

// Colorblind-safe diverging Teal (protected < 0) <-> Orange/Purple (worsened > 0)
function colorForDiff(d) {
  if (Math.abs(d) < 0.002) return null;
  const mag = Math.min(1, Math.abs(d) / 0.15);
  if (d < 0) return [20, 184 + mag * 40, 166 + mag * 40, 120 + mag * 120];
  return [249, 115 - mag * 55, 22 + mag * 110, 130 + mag * 115];
}

function sampleBilinear(arr, active, cols, rows, fx, fy) {
  const c0 = Math.max(0, Math.min(cols - 1, Math.floor(fx)));
  const r0 = Math.max(0, Math.min(rows - 1, Math.floor(fy)));
  const c1 = Math.min(cols - 1, c0 + 1);
  const r1 = Math.min(rows - 1, r0 + 1);
  const tx = Math.max(0, Math.min(1, fx - c0));
  const ty = Math.max(0, Math.min(1, fy - r0));
  const i00 = r0 * cols + c0;
  const i10 = r0 * cols + c1;
  const i01 = r1 * cols + c0;
  const i11 = r1 * cols + c1;
  if (!active[i00]) return null;
  const v00 = arr[i00];
  const v10 = active[i10] ? arr[i10] : v00;
  const v01 = active[i01] ? arr[i01] : v00;
  const v11 = active[i11] ? arr[i11] : v00;
  return (v00 * (1 - tx) + v10 * tx) * (1 - ty) + (v01 * (1 - tx) + v11 * tx) * ty;
}

function updateSmoothWaterImage(meta, env, frame) {
  const cols = meta.cols;
  const rows = meta.rows;
  const W = rasterCanvas.width;
  const H = rasterCanvas.height;
  const img = rasterCtx.createImageData(W, H);
  const data = img.data;

  let speedArr = null;
  if (state.layer === 'velocity' || state.layer === 'hazard') {
    speedArr = new Float32Array(cols * rows);
    for (let i = 0; i < cols * rows; i++) {
      speedArr[i] = Math.hypot(frame.vx[i], frame.vy[i]);
    }
  }

  for (let py = 0; py < H; py++) {
    const fy = (1 - (py + 0.5) / H) * (rows - 1);
    const rNearest = Math.min(rows - 1, Math.max(0, Math.round(fy)));
    for (let px = 0; px < W; px++) {
      const fx = ((px + 0.5) / W) * (cols - 1);
      const cNearest = Math.min(cols - 1, Math.max(0, Math.round(fx)));
      const out = (py * W + px) * 4;

      if (!meta.active_mask[rNearest * cols + cNearest]) {
        if (((px + py) % 10) < 2) {
          data[out] = 244; data[out + 1] = 63; data[out + 2] = 94; data[out + 3] = 125;
        }
        continue;
      }

      let rgba = null;
      if (state.layer === 'depth') {
        const src = state.envelopeMode ? env.max_depth_m : frame.depth;
        const h = sampleBilinear(src, meta.active_mask, cols, rows, fx, fy);
        if (h !== null) rgba = colorForDepth(h, env.threshold_m);
      } else if (state.layer === 'velocity') {
        const src = state.envelopeMode ? env.max_speed_m_s : speedArr;
        const sp = sampleBilinear(src, meta.active_mask, cols, rows, fx, fy);
        if (sp !== null) rgba = colorForVelocity(sp);
      } else if (state.layer === 'hazard') {
        const h = sampleBilinear(frame.depth, meta.active_mask, cols, rows, fx, fy);
        const sp = sampleBilinear(speedArr, meta.active_mask, cols, rows, fx, fy);
        const hr = state.envelopeMode
          ? sampleBilinear(env.max_hazard_m2_s, meta.active_mask, cols, rows, fx, fy)
          : (h !== null && sp !== null ? h * (sp + 0.5) : null);
        if (hr !== null) rgba = colorForHazard(hr);
      } else if (state.layer === 'diff' && state.diffData) {
        const d = sampleBilinear(state.diffData.delta_max_depth_m, meta.active_mask, cols, rows, fx, fy);
        if (d !== null) rgba = colorForDiff(d);
      }

      if (rgba) {
        data[out] = rgba[0];
        data[out + 1] = rgba[1];
        data[out + 2] = rgba[2];
        data[out + 3] = Math.round(rgba[3] * 0.86);
      }
    }
  }
  rasterCtx.putImageData(img, 0, 0);

  const [minLon, minLat, maxLon, maxLat] = meta.wgs84_bounds;
  const coords = [[minLon, maxLat], [maxLon, maxLat], [maxLon, minLat], [minLon, minLat]];
  const url = rasterCanvas.toDataURL('image/png');
  if (map.getSource('arus-water-img')) {
    map.getSource('arus-water-img').updateImage({ url, coordinates: coords });
  } else {
    map.addSource('arus-water-img', { type: 'image', url, coordinates: coords });
    map.addLayer(
      {
        id: 'arus-water-smooth',
        type: 'raster',
        source: 'arus-water-img',
        paint: { 'raster-opacity': 0.88, 'raster-resampling': 'linear', 'raster-fade-duration': 0 },
      },
      'arus-particles-line',
    );
  }
}

function renderFloodCanvas(forceRaster) {
  if (!state.grid || !state.frames.length || !layersReady) return;
  const meta = state.grid.metadata;
  const env = state.grid.envelope;
  const cols = meta.cols;
  const rows = meta.rows;
  const [minLon, minLat, maxLon, maxLat] = meta.wgs84_bounds;
  const dLon = (maxLon - minLon) / cols;
  const dLat = (maxLat - minLat) / rows;
  const frame = state.frames[Math.min(state.frameIdx, state.frames.length - 1)];

  const rasterKey = `${state.runId}:${state.frameIdx}:${state.layer}:${state.envelopeMode}`;
  if (forceRaster || rasterKey !== lastRasterKey) {
    lastRasterKey = rasterKey;
    updateSmoothWaterImage(meta, env, frame);
  }

  // 60 FPS animated velocity particle streamlines as crisp vector segments
  const particleFeatures = [];
  if (state.particlesOn && state.layer !== 'diff') {
    for (const p of state.particles) {
      const c = Math.min(cols - 1, Math.max(0, Math.floor(p.x * cols)));
      const r = Math.min(rows - 1, Math.max(0, Math.floor(p.y * rows)));
      const idx = r * cols + c;
      const h = frame.depth[idx];
      const vx = frame.vx[idx];
      const vy = frame.vy[idx];
      const sp = Math.hypot(vx, vy);
      if (!meta.active_mask[idx] || h < 0.01 || sp < 0.01 || p.age >= p.maxAge) {
        p.x = Math.random();
        p.y = Math.random();
        p.age = 0;
        continue;
      }
      const lon0 = minLon + p.x * (maxLon - minLon);
      const lat0 = minLat + p.y * (maxLat - minLat);
      const step = 0.018;
      const nx = p.x + vx * step;
      const ny = p.y + vy * step;
      const lon1 = minLon + nx * (maxLon - minLon);
      const lat1 = minLat + ny * (maxLat - minLat);
      const opacity = Math.min(0.95, 0.35 + sp * 0.8) * (1 - p.age / p.maxAge);
      particleFeatures.push({
        type: 'Feature',
        geometry: { type: 'LineString', coordinates: [[lon0, lat0], [lon1, lat1]] },
        properties: { color: sp > 0.5 ? '#fde047' : '#e0f2fe', opacity },
      });
      p.x = nx;
      p.y = ny;
      p.age += 1;
      if (p.x < 0 || p.x > 1 || p.y < 0 || p.y > 1) {
        p.x = Math.random();
        p.y = Math.random();
        p.age = 0;
      }
    }
  }

  const overlayFeatures = [];
  for (const iv of state.interventions) {
    const lon0 = minLon + iv.col * dLon;
    const lat0 = minLat + iv.row * dLat;
    const ring = [[lon0, lat0], [lon0 + dLon, lat0], [lon0 + dLon, lat0 + dLat], [lon0, lat0 + dLat], [lon0, lat0]];
    overlayFeatures.push({
      type: 'Feature',
      geometry: { type: 'LineString', coordinates: ring },
      properties: { color: iv.delta_m > 0 ? '#fbbf24' : '#2dd4bf' },
    });
  }

  if (state.xsecPts.length === 2) {
    const [a, b] = state.xsecPts;
    overlayFeatures.push({
      type: 'Feature',
      geometry: {
        type: 'LineString',
        coordinates: [
          [minLon + (a.col + 0.5) * dLon, minLat + (a.row + 0.5) * dLat],
          [minLon + (b.col + 0.5) * dLon, minLat + (b.row + 0.5) * dLat],
        ],
      },
      properties: { color: '#ffffff' },
    });
  }

  map.getSource('arus-particles').setData({ type: 'FeatureCollection', features: particleFeatures });
  map.getSource('arus-overlays').setData({ type: 'FeatureCollection', features: overlayFeatures });
}

function lngLatToCell(lon, lat) {
  if (!state.grid) return null;
  const meta = state.grid.metadata;
  const [minLon, minLat, maxLon, maxLat] = meta.wgs84_bounds;
  if (lon < minLon || lon > maxLon || lat < minLat || lat > maxLat) return null;
  const col = Math.min(meta.cols - 1, Math.max(0, Math.floor(((lon - minLon) / (maxLon - minLon)) * meta.cols)));
  const row = Math.min(meta.rows - 1, Math.max(0, Math.floor(((lat - minLat) / (maxLat - minLat)) * meta.rows)));
  return { col, row, idx: row * meta.cols + col };
}

function handleHover(lon, lat) {
  const el = $('telemetry');
  const hit = lngLatToCell(lon, lat);
  if (!hit || !state.frames.length) {
    el.textContent = `${lat.toFixed(4)}°, ${lon.toFixed(4)}° • Outside domain`;
    return;
  }
  const meta = state.grid.metadata;
  if (!meta.active_mask[hit.idx]) {
    el.textContent = `${lat.toFixed(4)}°, ${lon.toFixed(4)}° • NODATA / OUT OF COVERAGE`;
    return;
  }
  const frame = state.frames[Math.min(state.frameIdx, state.frames.length - 1)];
  const z = meta.bed_m[hit.idx];
  const h = frame.depth[hit.idx];
  const sp = Math.hypot(frame.vx[hit.idx], frame.vy[hit.idx]);
  el.textContent = `${lat.toFixed(4)}°, ${lon.toFixed(4)}° | z=${z.toFixed(2)}m | h=${h.toFixed(3)}m | WSE=${(z + h).toFixed(2)}m | |v|=${sp.toFixed(2)}m/s`;
}

function handleMapClick(lon, lat) {
  if (state.tool === 'pick') {
    state.center = { lon, lat };
    state.uploadedDem = null;
    $('coord-search').value = `${lat.toFixed(4)}, ${lon.toFixed(4)}`;
    setTool('inspect');
    runSimulation(false);
    return;
  }
  const hit = lngLatToCell(lon, lat);
  if (!hit) return;

  if (state.tool === 'bund' || state.tool === 'pond') {
    const delta = state.tool === 'bund' ? 0.8 : -0.8;
    const reason = state.tool === 'bund' ? 'raised flood bund' : 'detention pond';
    // Paint a small 2x2 footprint so earthwork & hydraulic impact are unmistakable
    for (let dr = 0; dr <= 1; dr++) {
      for (let dc = 0; dc <= 1; dc++) {
        const c = hit.col + dc;
        const r = hit.row + dr;
        if (c < state.grid.metadata.cols && r < state.grid.metadata.rows) {
          state.interventions.push({ col: c, row: r, delta_m: delta, reason });
        }
      }
    }
    updateInterventionBar();
    return;
  }

  if (state.tool === 'xsection') {
    if (state.xsecPts.length >= 2) state.xsecPts = [];
    state.xsecPts.push({ col: hit.col, row: hit.row });
    if (state.xsecPts.length === 2) {
      renderCrossSectionInspector();
    }
    return;
  }

  state.selectedCell = { col: hit.col, row: hit.row };
  renderHydrographInspector();
}

function renderHydrographInspector() {
  if (!state.selectedCell || !state.grid || !state.frames.length) return;
  const meta = state.grid.metadata;
  const env = state.grid.envelope;
  const { col, row } = state.selectedCell;
  const idx = row * meta.cols + col;
  const box = $('inspector');
  box.classList.remove('hidden');

  if (!meta.active_mask[idx]) {
    $('inspector-title').textContent = `Cell (${col}, ${row}) — OUT OF COVERAGE`;
    $('inspector-svg').innerHTML = `<text x="170" y="68" text-anchor="middle" fill="#fda4af" font-family="monospace" font-size="11">NODATA CELL — NEVER TREAT AS DRY</text>`;
    $('inspector-stats').innerHTML = '';
    return;
  }

  const times = state.frames.map(f => f.time_s);
  const depths = state.frames.map(f => f.depth[idx]);
  const tMax = Math.max(1, times[times.length - 1]);
  const dPeak = Math.max(env.max_depth_m[idx], ...depths);
  const yMax = Math.max(dPeak * 1.15, env.threshold_m * 1.3, 0.05);

  const pts = times.map((t, i) => {
    const x = 36 + (t / tMax) * 290;
    const y = 108 - (depths[i] / yMax) * 86;
    return `${x.toFixed(1)},${y.toFixed(1)}`;
  }).join(' ');
  const ty = 108 - (env.threshold_m / yMax) * 86;
  const curT = state.frames[Math.min(state.frameIdx, state.frames.length - 1)].time_s;
  const cx = 36 + (curT / tMax) * 290;

  $('inspector-title').textContent = `Probe Hydrograph (${col}, ${row})`;
  $('inspector-svg').innerHTML = `
    <line x1="36" y1="22" x2="326" y2="22" stroke="#1e293b"/>
    <line x1="36" y1="108" x2="326" y2="108" stroke="#334155"/>
    <polygon points="${pts} 326,108 36,108" fill="rgba(56,189,248,0.22)"/>
    <polyline points="${pts}" fill="none" stroke="#38bdf8" stroke-width="2.2"/>
    <line x1="36" y1="${ty.toFixed(1)}" x2="326" y2="${ty.toFixed(1)}" stroke="#f43f5e" stroke-dasharray="4 3" stroke-width="1.3"/>
    <line x1="${cx.toFixed(1)}" y1="18" x2="${cx.toFixed(1)}" y2="108" stroke="#fbbf24" stroke-width="1.2"/>
    <text x="32" y="26" text-anchor="end" fill="#64748b" font-size="8.5" font-family="monospace">${yMax.toFixed(2)}</text>
    <text x="32" y="111" text-anchor="end" fill="#64748b" font-size="8.5" font-family="monospace">0.00</text>
    <text x="324" y="${(ty - 3).toFixed(1)}" text-anchor="end" fill="#fb7185" font-size="8.5" font-family="monospace">THRESH ${env.threshold_m.toFixed(2)}m</text>
  `;
  const onset = env.onset_time_s[idx] >= 0 ? `${env.onset_time_s[idx].toFixed(0)}s` : '—';
  const rec = depths[depths.length - 1] >= env.threshold_m ? 'Right-cens.' : 'Receded';
  $('inspector-stats').innerHTML = `
    <div class="mini-stat"><div class="mini-k">Peak h</div><div class="mini-v">${dPeak.toFixed(3)}m</div></div>
    <div class="mini-stat"><div class="mini-k">Max |v|</div><div class="mini-v">${env.max_speed_m_s[idx].toFixed(2)}m/s</div></div>
    <div class="mini-stat"><div class="mini-k">Onset</div><div class="mini-v">${onset}</div></div>
    <div class="mini-stat"><div class="mini-k">Status</div><div class="mini-v">${rec}</div></div>
  `;
}

function renderCrossSectionInspector() {
  if (state.xsecPts.length !== 2 || !state.grid || !state.frames.length) return;
  const meta = state.grid.metadata;
  const frame = state.frames[Math.min(state.frameIdx, state.frames.length - 1)];
  const [a, b] = state.xsecPts;
  const steps = 32;
  const beds = [];
  const wses = [];
  for (let i = 0; i <= steps; i++) {
    const t = i / steps;
    const c = Math.min(meta.cols - 1, Math.max(0, Math.round(a.col + (b.col - a.col) * t)));
    const r = Math.min(meta.rows - 1, Math.max(0, Math.round(a.row + (b.row - a.row) * t)));
    const idx = r * meta.cols + c;
    const z = meta.bed_m[idx];
    const h = meta.active_mask[idx] ? frame.depth[idx] : 0;
    beds.push(z);
    wses.push(z + h);
  }
  const zMin = Math.min(...beds) - 0.1;
  const zMax = Math.max(...wses) + 0.15;
  const span = Math.max(0.2, zMax - zMin);
  const bedPts = beds.map((z, i) => `${(36 + (i / steps) * 290).toFixed(1)},${(108 - ((z - zMin) / span) * 86).toFixed(1)}`).join(' ');
  const wsePts = wses.map((z, i) => `${(36 + (i / steps) * 290).toFixed(1)},${(108 - ((z - zMin) / span) * 86).toFixed(1)}`).join(' ');
  const bedRev = [...beds].reverse().map((z, i) => `${(36 + ((steps - i) / steps) * 290).toFixed(1)},${(108 - ((z - zMin) / span) * 86).toFixed(1)}`).join(' ');

  const lenM = Math.hypot((b.col - a.col) * meta.dx_m, (b.row - a.row) * meta.dy_m);
  const maxH = Math.max(...wses.map((w, i) => w - beds[i]));

  $('inspector').classList.remove('hidden');
  $('inspector-title').textContent = `Cross-Section A → B (${lenM.toFixed(0)} m)`;
  $('inspector-svg').innerHTML = `
    <polygon points="${wsePts} ${bedRev}" fill="rgba(56,189,248,0.45)"/>
    <polygon points="${bedPts} 326,108 36,108" fill="#1e293b"/>
    <polyline points="${bedPts}" fill="none" stroke="#94a3b8" stroke-width="1.8"/>
    <polyline points="${wsePts}" fill="none" stroke="#38bdf8" stroke-width="2.2"/>
    <text x="32" y="26" text-anchor="end" fill="#64748b" font-size="8.5" font-family="monospace">${zMax.toFixed(1)}m</text>
    <text x="32" y="111" text-anchor="end" fill="#64748b" font-size="8.5" font-family="monospace">${zMin.toFixed(1)}m</text>
    <text x="38" y="122" fill="#cbd5e1" font-size="8.5" font-family="monospace">A</text>
    <text x="322" y="122" text-anchor="end" fill="#cbd5e1" font-size="8.5" font-family="monospace">B (${lenM.toFixed(0)}m)</text>
  `;
  $('inspector-stats').innerHTML = `
    <div class="mini-stat"><div class="mini-k">Length</div><div class="mini-v">${lenM.toFixed(0)}m</div></div>
    <div class="mini-stat"><div class="mini-k">Max h</div><div class="mini-v">${maxH.toFixed(3)}m</div></div>
    <div class="mini-stat"><div class="mini-k">Bed Min</div><div class="mini-v">${Math.min(...beds).toFixed(2)}m</div></div>
    <div class="mini-stat"><div class="mini-k">WSE Max</div><div class="mini-v">${Math.max(...wses).toFixed(2)}m</div></div>
  `;
}

function lonLatToTilePixel(lon, lat, zoom) {
  const n = Math.pow(2, zoom);
  const x = ((lon + 180) / 360) * n;
  const latRad = (lat * Math.PI) / 180;
  const y = ((1 - Math.log(Math.tan(latRad) + 1 / Math.cos(latRad)) / Math.PI) / 2) * n;
  const tx = Math.floor(x);
  const ty = Math.floor(y);
  const px = Math.min(255, Math.max(0, Math.floor((x - tx) * 256)));
  const py = Math.min(255, Math.max(0, Math.floor((y - ty) * 256)));
  return { tx, ty, px, py };
}

function loadTerrariumTile(z, tx, ty) {
  return new Promise((resolve) => {
    const img = new Image();
    img.crossOrigin = 'anonymous';
    const timer = setTimeout(() => resolve(null), 4000);
    img.onload = () => {
      clearTimeout(timer);
      const c = document.createElement('canvas');
      c.width = 256;
      c.height = 256;
      const ctx = c.getContext('2d');
      ctx.drawImage(img, 0, 0, 256, 256);
      resolve(ctx.getImageData(0, 0, 256, 256).data);
    };
    img.onerror = () => {
      clearTimeout(timer);
      resolve(null);
    };
    img.src = `https://s3.amazonaws.com/elevation-tiles-prod/terrarium/${z}/${tx}/${ty}.png`;
  });
}

async function fetchTerrariumElevations(centerLon, centerLat, cols, rows, dxM) {
  const key = `${centerLon.toFixed(4)},${centerLat.toFixed(4)},${cols},${rows},${dxM}`;
  if (state.cachedDem && state.cachedDem.key === key) {
    return state.cachedDem.elevations;
  }
  const widthM = cols * dxM;
  const heightM = rows * dxM;
  const mPerDegLat = 111320.0;
  const mPerDegLon = Math.max(10000.0, 111320.0 * Math.cos((centerLat * Math.PI) / 180));
  const minLon = centerLon - (widthM * 0.5) / mPerDegLon;
  const maxLon = centerLon + (widthM * 0.5) / mPerDegLon;
  const minLat = centerLat - (heightM * 0.5) / mPerDegLat;
  const maxLat = centerLat + (heightM * 0.5) / mPerDegLat;

  const zoom = 13;
  const needed = new Map();
  const samples = [];
  for (let r = 0; r < rows; r++) {
    const lat = minLat + ((r + 0.5) / rows) * (maxLat - minLat);
    for (let c = 0; c < cols; c++) {
      const lon = minLon + ((c + 0.5) / cols) * (maxLon - minLon);
      const tp = lonLatToTilePixel(lon, lat, zoom);
      const tkey = `${tp.tx}/${tp.ty}`;
      if (!needed.has(tkey)) needed.set(tkey, { tx: tp.tx, ty: tp.ty });
      samples.push({ tkey, px: tp.px, py: tp.py });
    }
  }

  const tileData = new Map();
  await Promise.all(
    Array.from(needed.entries()).map(async ([tkey, { tx, ty }]) => {
      const data = await loadTerrariumTile(zoom, tx, ty);
      if (data) tileData.set(tkey, data);
    }),
  );
  if (tileData.size === 0) return null;

  const elevations = [];
  let validCount = 0;
  let minZ = Infinity;
  let maxZ = -Infinity;
  for (const s of samples) {
    const data = tileData.get(s.tkey);
    if (!data) {
      elevations.push(null);
      continue;
    }
    const off = (s.py * 256 + s.px) * 4;
    const z = (data[off] * 256 + data[off + 1] + data[off + 2] / 256) - 32768;
    if (Number.isFinite(z)) {
      elevations.push(z);
      validCount++;
      if (z < minZ) minZ = z;
      if (z > maxZ) maxZ = z;
    } else {
      elevations.push(null);
    }
  }
  if (validCount < cols * rows * 0.8 || (maxZ - minZ) < 0.2) {
    return null;
  }
  state.cachedDem = { key, elevations };
  return elevations;
}

async function buildRequestPayload() {
  const landChoice = $('ctl-soil').value === 'green-ampt'
    ? { kind: 'modified-green-ampt', suction_m: 0.11, conductivity_m_per_s: 0.000015, maximum_deficit: 0.35 }
    : { kind: 'impervious' };
  const bndVal = $('ctl-boundary').value;
  const boundary = bndVal === 'closed'
    ? { kind: 'closed' }
    : { kind: 'transmissive' };
  const cols = 20;
  const rows = 18;
  const dxM = 75.0;
  const sampled = state.uploadedDem
    ? null
    : await fetchTerrariumElevations(state.center.lon, state.center.lat, cols, rows, dxM);
  return {
    center_lon: state.center.lon,
    center_lat: state.center.lat,
    cols,
    rows,
    dx_m: dxM,
    rainfall_mm_per_hr: parseFloat($('ctl-rain').value),
    hyetograph: $('ctl-hyeto').value,
    duration_s: parseFloat($('ctl-dur').value),
    manning_n: parseFloat($('ctl-manning').value),
    land: landChoice,
    boundary,
    threshold_m: 0.12,
    precision: 'f64',
    interventions: state.interventions,
    ascii_dem: state.uploadedDem,
    terrain_elevations_m: sampled,
    terrain_provenance: sampled ? 'BrowserTerrariumScreening' : null,
  };
}

async function loadRun(runId) {
  const [gridResp, binResp] = await Promise.all([
    fetch(`/api/v1/runs/${runId}/grid`),
    fetch(`/api/v1/runs/${runId}/frames.bin`),
  ]);
  if (!gridResp.ok || !binResp.ok) throw new Error('Failed to load simulation frames');
  state.grid = await gridResp.json();
  state.frames = decodeFramesBin(await binResp.arrayBuffer());
  state.runId = runId;
  state.frameIdx = Math.min(Math.floor(state.frames.length * 0.55), state.frames.length - 1);
  seedParticles();
  attachFloodCanvasSource(state.grid.metadata.wgs84_bounds);

  const scrubber = $('scrubber');
  scrubber.max = String(Math.max(0, state.frames.length - 1));
  scrubber.value = String(state.frameIdx);
  updateHonestyPill();
  updateTimeReadout();
  renderFloodCanvas();
}

async function runSimulation(compareMode) {
  const btn = compareMode ? $('btn-compare') : $('btn-run');
  const orig = btn.innerHTML;
  btn.disabled = true;
  btn.innerHTML = '<span class="spinner"></span> Solving hydr F64...';
  try {
    const payload = await buildRequestPayload();
    if (compareMode) {
      const resp = await fetch('/api/v1/compare', {
        method: 'POST',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify(payload),
      });
      if (!resp.ok) throw new Error(await resp.text());
      state.diffData = await resp.json();
      state.xsecPts = [];
      state.selectedCell = null;
      await loadRun(state.diffData.intervention_run_id);
      setLayer('diff');
      $('inspector').classList.remove('hidden');
      $('inspector-title').textContent = 'Baseline vs Intervention Δh Impact';
      $('inspector-svg').innerHTML = `
        <text x="16" y="34" fill="#2dd4bf" font-family="monospace" font-size="11" font-weight="700">PROTECTED: ${state.diffData.improved_cells} cells (max -${state.diffData.max_reduction_m.toFixed(3)} m)</text>
        <text x="16" y="60" fill="#fb923c" font-family="monospace" font-size="11" font-weight="700">WORSENED:  ${state.diffData.worsened_cells} cells (max +${state.diffData.max_worsening_m.toFixed(3)} m)</text>
        <text x="16" y="88" fill="#cbd5e1" font-family="monospace" font-size="10">Cut: ${state.diffData.earthwork.cut_volume_m3.toFixed(0)} m³  |  Fill: ${state.diffData.earthwork.fill_volume_m3.toFixed(0)} m³</text>
        <text x="16" y="110" fill="#4ade80" font-family="monospace" font-size="10">Conservation Valid: ${state.diffData.conservation_valid ? 'PASS' : 'FAIL'}</text>
      `;
      $('inspector-stats').innerHTML = '';
    } else {
      state.diffData = null;
      const resp = await fetch('/api/v1/simulations', {
        method: 'POST',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify(payload),
      });
      if (!resp.ok) throw new Error(await resp.text());
      const summary = await resp.json();
      await loadRun(summary.run_id);
      const [minLon, minLat, maxLon, maxLat] = summary.wgs84_bounds;
      map.flyTo({
        center: [(minLon + maxLon) * 0.5, (minLat + maxLat) * 0.5],
        zoom: 14.5,
        pitch: state.is3D ? 58 : 0,
        duration: 900,
      });
    }
  } catch (err) {
    $('inspector').classList.remove('hidden');
    $('inspector-title').textContent = 'Simulation Notice';
    $('inspector-svg').innerHTML = `<text x="14" y="64" fill="#fda4af" font-family="monospace" font-size="10">${String(err.message).slice(0, 56)}</text>`;
    $('inspector-stats').innerHTML = '';
  } finally {
    btn.disabled = false;
    btn.innerHTML = orig;
  }
}

function updateHonestyPill() {
  const pill = $('honesty-pill');
  if (!state.grid) return;
  const ok = state.grid.budget_assessment.passed;
  const res = Math.abs(state.grid.water_budget.residual_m3);
  const prov = state.grid.metadata.terrain_provenance;
  const demTag = prov === 'BrowserTerrariumScreening'
    ? 'DEM: Terrarium 30m'
    : prov === 'UploadedAsciiDem'
      ? 'DEM: Uploaded .ASC'
      : 'DEM: Synthetic';
  pill.classList.toggle('fail', !ok);
  pill.innerHTML = `<span class="dot"></span>${demTag} • MASS ${ok ? 'OK' : 'FAIL'} (${res.toExponential(1)} m³)`;
}

function updateTimeReadout() {
  if (!state.frames.length) return;
  const cur = state.frames[Math.min(state.frameIdx, state.frames.length - 1)].time_s;
  const end = state.frames[state.frames.length - 1].time_s;
  $('time-readout').textContent = `t = ${cur.toFixed(0)}s / ${end.toFixed(0)}s`;
}

function updateInterventionBar() {
  const count = state.interventions.length;
  $('iv-count').textContent = `${count} cells`;
  $('btn-compare').disabled = count === 0;
}

function setTool(tool) {
  state.tool = tool;
  document.querySelectorAll('[data-tool]').forEach(el => {
    el.classList.toggle('active', el.dataset.tool === tool);
  });
}

function setLayer(layer) {
  state.layer = layer;
  document.querySelectorAll('[data-layer]').forEach(el => {
    el.classList.toggle('active', el.dataset.layer === layer);
  });
  const bar = $('legend-bar');
  const ticks = $('legend-ticks');
  const note = $('legend-note');
  if (layer === 'depth') {
    bar.style.background = 'linear-gradient(90deg, #0ea5e9, #38bdf8, #f59e0b, #f43f5e)';
    ticks.innerHTML = '<span>0.01m</span><span>0.15m</span><span>0.40m+</span>';
    note.textContent = 'Draped depth on 3D terrain (h < 0.01m transparent; hatched = nodata).';
  } else if (layer === 'velocity') {
    bar.style.background = 'linear-gradient(90deg, #2dd4bf, #eab308, #f43f5e)';
    ticks.innerHTML = '<span>0.0 m/s</span><span>0.75 m/s</span><span>1.5+ m/s</span>';
    note.textContent = 'Depth-averaged velocity |v| with 60 FPS streamlines.';
  } else if (layer === 'hazard') {
    bar.style.background = 'linear-gradient(90deg, #38bdf8 0% 25%, #facc15 25% 50%, #f97316 50% 75%, #f43f5e 75% 100%)';
    ticks.innerHTML = '<span>&lt;0.75 Low</span><span>1.25 Mod</span><span>2.0+ Ext</span>';
    note.textContent = 'DEFRA FD2320 Hazard Rating: HR = h × (v + 0.5), DF = 0.';
  } else {
    bar.style.background = 'linear-gradient(90deg, #14b8a6, #0f172a 50%, #f97316)';
    ticks.innerHTML = '<span>-0.15m Safer</span><span>0</span><span>+0.15m Worse</span>';
    note.textContent = 'Colorblind-safe Δh = Intervention − Baseline max depth.';
  }
  renderFloodCanvas();
}

function animationLoop(now) {
  if (state.playing && state.frames.length > 1 && now - state.lastTickMs > (220 / state.speed)) {
    state.frameIdx = (state.frameIdx + 1) % state.frames.length;
    $('scrubber').value = String(state.frameIdx);
    updateTimeReadout();
    if (state.xsecPts.length === 2) renderCrossSectionInspector();
    else if (state.selectedCell) renderHydrographInspector();
    state.lastTickMs = now;
  }
  renderFloodCanvas();
  requestAnimationFrame(animationLoop);
}

async function openSourcesModal() {
  $('sources-modal').classList.remove('hidden');
  const grid = $('sources-grid');
  grid.innerHTML = 'Loading catalog...';
  try {
    const resp = await fetch('/api/v1/catalog/sources');
    if (!resp.ok) throw new Error('Catalog not configured');
    const data = await resp.json();
    grid.innerHTML = data.sources.map(s => `
      <div class="src-card">
        <strong>${s.title}</strong>
        <div><span class="src-badge ${s.access === 'public' ? 'pub' : ''}">${s.access}</span> <span class="src-badge">${s.data_class}</span></div>
        <div style="color:#94a3b8">${s.provider} • ${s.license.name}</div>
      </div>
    `).join('');
  } catch (e) {
    grid.innerHTML = `<div style="color:#fda4af">${e.message}</div>`;
  }
}

function wireUi() {
  const sel = $('preset-select');
  sel.innerHTML = PRESETS.map(p => `<option value="${p.id}">${p.name}</option>`).join('');
  sel.addEventListener('change', () => {
    const p = PRESETS.find(x => x.id === sel.value);
    if (!p) return;
    state.center = { lon: p.lon, lat: p.lat };
    state.uploadedDem = null;
    state.interventions = [];
    updateInterventionBar();
    map.flyTo({ center: [p.lon, p.lat], zoom: p.zoom, pitch: state.is3D ? 58 : 0, duration: 1100 });
    runSimulation(false);
  });

  $('coord-search').addEventListener('keydown', async (e) => {
    if (e.key !== 'Enter') return;
    const raw = e.target.value.trim();
    if (!raw) return;
    const parts = raw.split(',').map(v => parseFloat(v.trim()));
    if (parts.length === 2 && Number.isFinite(parts[0]) && Number.isFinite(parts[1])) {
      state.center = { lat: parts[0], lon: parts[1] };
      map.flyTo({ center: [parts[1], parts[0]], zoom: 14.1, duration: 900 });
      map.once('idle', () => runSimulation(false));
      return;
    }
    try {
      const resp = await fetch(`https://nominatim.openstreetmap.org/search?format=json&limit=1&q=${encodeURIComponent(raw)}`);
      const hits = await resp.json();
      if (hits && hits.length > 0) {
        const lat = parseFloat(hits[0].lat);
        const lon = parseFloat(hits[0].lon);
        state.center = { lat, lon };
        map.flyTo({ center: [lon, lat], zoom: 14.1, duration: 1100 });
        map.once('idle', () => runSimulation(false));
      }
    } catch (_) { /* offline geocoding unavailable */ }
  });

  document.querySelectorAll('[data-tool]').forEach(el => {
    el.addEventListener('click', () => setTool(el.dataset.tool));
  });
  document.querySelectorAll('[data-layer]').forEach(el => {
    el.addEventListener('click', () => setLayer(el.dataset.layer));
  });

  $('btn-2d3d').addEventListener('click', () => {
    state.is3D = !state.is3D;
    $('btn-2d3d').textContent = state.is3D ? '3D' : '2D';
    $('btn-2d3d').classList.toggle('active', state.is3D);
    map.easeTo({ pitch: state.is3D ? 58 : 0, bearing: state.is3D ? -20 : 0, duration: 600 });
  });

  $('btn-mode').addEventListener('click', () => {
    state.publicMode = !state.publicMode;
    $('drawer').classList.toggle('public-mode', state.publicMode);
    $('btn-mode').textContent = state.publicMode ? 'Public' : 'Engineer';
  });

  $('btn-env').addEventListener('click', () => {
    state.envelopeMode = !state.envelopeMode;
    $('btn-env').textContent = state.envelopeMode ? 'Max Env' : 'Live t';
    $('btn-env').classList.toggle('active', state.envelopeMode);
  });

  $('btn-particles').addEventListener('click', () => {
    state.particlesOn = !state.particlesOn;
    $('btn-particles').classList.toggle('active', state.particlesOn);
  });

  $('btn-play').addEventListener('click', () => {
    state.playing = !state.playing;
    $('btn-play').textContent = state.playing ? '❚❚' : '▶';
  });

  $('btn-speed').addEventListener('click', () => {
    state.speed = state.speed === 1 ? 2 : state.speed === 2 ? 5 : 1;
    $('btn-speed').textContent = `${state.speed}x`;
  });

  $('scrubber').addEventListener('input', (e) => {
    state.playing = false;
    $('btn-play').textContent = '▶';
    state.frameIdx = parseInt(e.target.value, 10) || 0;
    updateTimeReadout();
    if (state.xsecPts.length === 2) renderCrossSectionInspector();
    else if (state.selectedCell) renderHydrographInspector();
  });

  $('ctl-rain').addEventListener('input', (e) => { $('val-rain').textContent = `${e.target.value} mm/hr`; });
  $('ctl-dur').addEventListener('input', (e) => { $('val-dur').textContent = `${e.target.value} s`; });
  $('ctl-manning').addEventListener('input', (e) => { $('val-manning').textContent = Number(e.target.value).toFixed(3); });

  $('btn-iv-undo').addEventListener('click', () => { state.interventions.splice(-4, 4); updateInterventionBar(); });
  $('btn-iv-clear').addEventListener('click', () => { state.interventions = []; updateInterventionBar(); });
  $('btn-run').addEventListener('click', () => runSimulation(false));
  $('btn-compare').addEventListener('click', () => runSimulation(true));

  $('btn-export-asc').addEventListener('click', () => {
    if (state.runId) window.open(`/api/v1/runs/${state.runId}/export/max-depth.asc`, '_blank');
  });
  $('btn-export-csv').addEventListener('click', () => {
    if (state.runId) window.open(`/api/v1/runs/${state.runId}/export/outlooks.csv`, '_blank');
  });

  $('btn-sources').addEventListener('click', openSourcesModal);
  $('btn-sources-close').addEventListener('click', () => $('sources-modal').classList.add('hidden'));
  $('btn-inspector-close').addEventListener('click', () => {
    $('inspector').classList.add('hidden');
    state.xsecPts = [];
    state.selectedCell = null;
  });

  $('file-asc').addEventListener('change', async (e) => {
    const file = e.target.files?.[0];
    if (!file) return;
    const text = await file.text();
    const d = 0.004;
    state.uploadedDem = {
      ascii_content: text,
      projected_crs: 'EPSG:3375',
      vertical_datum: 'local-uploaded-datum',
      wgs84_bounds: [state.center.lon - d, state.center.lat - d, state.center.lon + d, state.center.lat + d],
    };
    runSimulation(false);
  });
}

window.addEventListener('DOMContentLoaded', () => {
  initMap();
  wireUi();
  setLayer('depth');
  requestAnimationFrame(animationLoop);
});
