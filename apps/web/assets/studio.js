// ARUS 2026 Flood Simulation Studio Client
// Driven by real hydr 0.1.0 binary Float32 frames (/api/v1/runs/{id}/grid & frames.bin).

const PRESETS = [
  { id: 'shah-alam', name: 'Shah Alam · Taman Sri Muda (MY)', lon: 101.535, lat: 3.031, zoom: 14.3 },
  { id: 'kl', name: 'Kuala Lumpur · Sg. Klang (MY)', lon: 101.696, lat: 3.149, zoom: 14.3 },
  { id: 'kota-bharu', name: 'Kota Bharu · Sg. Kelantan (MY)', lon: 102.238, lat: 6.125, zoom: 14.0 },
  { id: 'jakarta', name: 'Jakarta · Ciliwung (ID)', lon: 106.845, lat: -6.208, zoom: 14.1 },
  { id: 'rotterdam', name: 'Rotterdam · Nieuwe Maas (NL)', lon: 4.477, lat: 51.924, zoom: 14.0 },
  { id: 'houston', name: 'Houston · Buffalo Bayou (US)', lon: -95.369, lat: 29.760, zoom: 14.0 },
];

const state = {
  basemap: 'topo', // topo | sat | dark
  center: { lon: 101.535, lat: 3.031 },
  initZoom: 14.3,
  initPitch: 48,
  initBearing: -16,
  fixedView: false,
  tool: 'inspect', // inspect | pick | bund | pond | xsection
  layer: 'depth',  // depth | velocity | hazard | diff
  envelopeMode: false,
  particlesOn: true,
  is3D: true,
  playing: true,
  speed: 1,
  frameIdx: 0,
  lastTickMs: 0,
  runId: null,
  grid: null,
  frames: [],
  diffData: null,
  interventions: [],
  xsecPts: [],
  selectedCell: null,
  uploadedDem: null,
  cachedDem: null,
  nwpPeakMmHr: null,
  startEpochMs: Date.now(),
  tzLabel: 'UTC',
  spanHours: 0,
  antecedentMm: 0,
  hourlyRainMm: [],
  spatialRainGrid: null,
  hourlyTideM: [],
  drainageMmHr: 0,
  evapMmDay: 0,
  observedGauges: [],
  boundaryGeoJson: null,
  boundaryGrid: null,
  boundaryCellMask: null,
  outlookHotspots: [],
  particles: [],
};

// High-resolution offscreen water raster canvas pushed via MapLibre ImageSource
const rasterCanvas = document.createElement('canvas');
rasterCanvas.width = 384;
rasterCanvas.height = 384;
const rasterCtx = rasterCanvas.getContext('2d');
let lastRasterKey = '';

let map = null;
let layersReady = false;
let initialSimTriggered = false;

function $(id) { return document.getElementById(id); }

function getBasemapStyle(mode) {
  if (mode === 'dark') return 'https://tiles.openfreemap.org/styles/dark';
  if (mode === 'sat') {
    return {
      version: 8,
      sources: {
        esriSat: {
          type: 'raster',
          tiles: ['https://server.arcgisonline.com/ArcGIS/rest/services/World_Imagery/MapServer/tile/{z}/{y}/{x}'],
          tileSize: 256,
          maxzoom: 18,
        },
        esriLabels: {
          type: 'raster',
          tiles: ['https://services.arcgisonline.com/ArcGIS/rest/services/Reference/World_Boundaries_and_Places/MapServer/tile/{z}/{y}/{x}'],
          tileSize: 256,
          maxzoom: 18,
        },
      },
      layers: [
        { id: 'sat-imagery', type: 'raster', source: 'esriSat' },
        { id: 'sat-labels', type: 'raster', source: 'esriLabels', paint: { 'raster-opacity': 0.9 } },
      ],
    };
  }
  return 'https://tiles.openfreemap.org/styles/positron';
}

function buildOfflineStyle() {
  return {
    version: 8,
    sources: {},
    layers: [{ id: 'bg', type: 'background', paint: { 'background-color': '#e2e8f0' } }],
  };
}

function findFirstLabelLayerId() {
  if (!map) return undefined;
  const style = map.getStyle();
  if (!style || !style.layers) return undefined;
  if (state.basemap === 'sat') return 'sat-labels';
  const sym = style.layers.find((l) => l.type === 'symbol');
  return sym ? sym.id : undefined;
}

function initMap() {
  map = new maplibregl.Map({
    container: 'map',
    style: getBasemapStyle(state.basemap),
    center: [state.center.lon, state.center.lat],
    zoom: state.initZoom,
    pitch: state.initPitch,
    bearing: state.initBearing,
    antialias: true,
    preserveDrawingBuffer: true,
  });

  map.addControl(new maplibregl.ScaleControl({ maxWidth: 110, unit: 'metric' }), 'bottom-left');
  map.addControl(new maplibregl.NavigationControl({ visualizePitch: true }), 'top-right');

  let fellBack = false;
  map.on('error', (e) => {
    if (!fellBack && !map.isStyleLoaded() && (!e || !e.sourceId)) {
      fellBack = true;
      map.setStyle(buildOfflineStyle());
    }
  });

  map.on('style.load', () => {
    layersReady = false;
    lastRasterKey = '';
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
  map.on('moveend', updateFrameTelemetry);
}

function setBasemap(mode) {
  if (state.basemap === mode) return;
  state.basemap = mode;
  layersReady = false;
  lastRasterKey = '';
  document.documentElement.dataset.theme = mode === 'topo' ? 'light' : 'dark';
  document.querySelectorAll('[data-basemap]').forEach((el) => {
    el.classList.toggle('active', el.dataset.basemap === mode);
  });
  map.setStyle(getBasemapStyle(mode));
}

function ensureMapLayers() {
  if (!map || !map.isStyleLoaded() || map.getSource('arus-particles')) return;
  const empty = { type: 'FeatureCollection', features: [] };
  const beforeLabels = findFirstLabelLayerId();
  const isDark = state.basemap !== 'topo';

  const isPres = document.documentElement.dataset.presentation === 'true';
  map.addSource('arus-domain-box', { type: 'geojson', data: empty });
  map.addLayer(
    {
      id: 'arus-domain-outline',
      type: 'line',
      source: 'arus-domain-box',
      layout: { visibility: isPres || state.boundaryGeoJson ? 'none' : 'visible' },
      paint: {
        'line-color': isDark ? 'rgba(148, 163, 184, 0.5)' : 'rgba(15, 23, 42, 0.30)',
        'line-width': 1.2,
        'line-dasharray': [4, 3],
      },
    },
    beforeLabels,
  );

  map.addSource('arus-region-mask', { type: 'geojson', data: empty });
  map.addLayer(
    {
      id: 'arus-region-mask-fill',
      type: 'fill',
      source: 'arus-region-mask',
      paint: {
        'fill-color': isDark ? '#020617' : '#0f172a',
        'fill-opacity': isDark ? 0.32 : 0.10,
      },
    },
    beforeLabels,
  );

  map.addSource('arus-region-boundary', { type: 'geojson', data: empty });
  map.addLayer(
    {
      id: 'arus-region-outline',
      type: 'line',
      source: 'arus-region-boundary',
      paint: {
        'line-color': isDark ? '#38bdf8' : '#0284c7',
        'line-width': 2.2,
        'line-opacity': 0.85,
      },
    },
    beforeLabels,
  );

  // High-contrast emerald green original river channel line above flood water
  const style = map.getStyle();
  const vecSourceId = style && style.sources
    ? Object.keys(style.sources).find((k) => style.sources[k].type === 'vector')
    : null;
  if (vecSourceId) {
    map.addLayer(
      {
        id: 'arus-river-halo',
        type: 'line',
        source: vecSourceId,
        'source-layer': 'waterway',
        filter: ['in', ['get', 'class'], ['literal', ['river', 'canal']]],
        paint: {
          'line-color': isDark ? 'rgba(15, 23, 42, 0.55)' : 'rgba(255, 255, 255, 0.58)',
          'line-width': ['interpolate', ['linear'], ['zoom'], 8, 1.5, 12, 2.3, 15, 3.3],
        },
        layout: { 'line-cap': 'round', 'line-join': 'round' },
      },
      beforeLabels,
    );
    map.addLayer(
      {
        id: 'arus-river-line',
        type: 'line',
        source: vecSourceId,
        'source-layer': 'waterway',
        filter: ['in', ['get', 'class'], ['literal', ['river', 'canal']]],
        paint: {
          'line-color': '#10b981',
          'line-width': ['interpolate', ['linear'], ['zoom'], 8, 0.9, 12, 1.4, 15, 2.0],
          'line-opacity': 0.72,
        },
        layout: { 'line-cap': 'round', 'line-join': 'round' },
      },
      beforeLabels,
    );
  }

  map.addSource('arus-particles', { type: 'geojson', data: empty });
  map.addLayer(
    {
      id: 'arus-particles-line',
      type: 'line',
      source: 'arus-particles',
      paint: {
        'line-color': ['get', 'color'],
        'line-width': 1.8,
        'line-opacity': ['get', 'opacity'],
      },
      layout: { 'line-cap': 'round' },
    },
    beforeLabels,
  );

  map.addSource('arus-iv-polys', { type: 'geojson', data: empty });
  map.addLayer({
    id: 'arus-iv-fill',
    type: 'fill',
    source: 'arus-iv-polys',
    paint: {
      'fill-color': ['get', 'color'],
      'fill-opacity': 0.38,
    },
  });
  map.addLayer({
    id: 'arus-iv-stroke',
    type: 'line',
    source: 'arus-iv-polys',
    paint: {
      'line-color': ['get', 'color'],
      'line-width': 1.6,
    },
  });

  map.addSource('arus-xsec', { type: 'geojson', data: empty });
  map.addLayer({
    id: 'arus-xsec-line',
    type: 'line',
    source: 'arus-xsec',
    paint: {
      'line-color': isDark ? '#f8fafc' : '#0f172a',
      'line-width': 2.5,
      'line-dasharray': [3, 2],
    },
  });

  map.addSource('arus-hotspots', { type: 'geojson', data: empty });
  map.addLayer({
    id: 'arus-hotspots-circle',
    type: 'circle',
    source: 'arus-hotspots',
    paint: {
      'circle-radius': 6,
      'circle-color': ['get', 'color'],
      'circle-stroke-color': '#ffffff',
      'circle-stroke-width': 2,
    },
  });

  layersReady = true;
}

function updateDomainOutline(bounds) {
  if (!map || !layersReady) return;
  const isPres = document.documentElement.dataset.presentation === 'true';
  if (map.getLayer('arus-domain-outline')) {
    map.setLayoutProperty(
      'arus-domain-outline',
      'visibility',
      isPres || state.boundaryGeoJson ? 'none' : 'visible',
    );
  }
  const [minLon, minLat, maxLon, maxLat] = bounds;
  map.getSource('arus-domain-box').setData({
    type: 'Feature',
    geometry: {
      type: 'Polygon',
      coordinates: [[[minLon, minLat], [maxLon, minLat], [maxLon, maxLat], [minLon, maxLat], [minLon, minLat]]],
    },
  });
  if (state.boundaryGeoJson) applyRegionBoundary();
}

function applyRegionBoundary() {
  if (!map || !layersReady || !state.boundaryGeoJson) return;
  const geom = state.boundaryGeoJson;
  const bSrc = map.getSource('arus-region-boundary');
  const mSrc = map.getSource('arus-region-mask');
  if (bSrc) bSrc.setData({ type: 'Feature', geometry: geom, properties: {} });
  if (mSrc) {
    const worldRing = [[-180, -85], [180, -85], [180, 85], [-180, 85], [-180, -85]];
    const holes = [];
    if (geom.type === 'Polygon' && Array.isArray(geom.coordinates?.[0])) {
      holes.push(geom.coordinates[0]);
    } else if (geom.type === 'MultiPolygon' && Array.isArray(geom.coordinates)) {
      for (const poly of geom.coordinates) {
        if (Array.isArray(poly?.[0])) holes.push(poly[0]);
      }
    }
    if (holes.length > 0) {
      mSrc.setData({
        type: 'Feature',
        geometry: { type: 'Polygon', coordinates: [worldRing, ...holes] },
        properties: {},
      });
    }
  }
  if (map.getLayer('arus-domain-outline')) {
    map.setLayoutProperty('arus-domain-outline', 'visibility', 'none');
  }
}

function computeGeomBbox(geom) {
  let minLon = Infinity;
  let minLat = Infinity;
  let maxLon = -Infinity;
  let maxLat = -Infinity;
  const walk = (arr) => {
    if (!Array.isArray(arr)) return;
    if (typeof arr[0] === 'number' && typeof arr[1] === 'number') {
      if (arr[0] < minLon) minLon = arr[0];
      if (arr[0] > maxLon) maxLon = arr[0];
      if (arr[1] < minLat) minLat = arr[1];
      if (arr[1] > maxLat) maxLat = arr[1];
    } else {
      for (const item of arr) walk(item);
    }
  };
  walk(geom.coordinates);
  return [minLon, minLat, maxLon, maxLat];
}

async function loadBoundaryByQuery(query) {
  if (!query) return false;
  try {
    const resp = await fetch(
      `https://nominatim.openstreetmap.org/search?format=json&polygon_geojson=1&limit=1&q=${encodeURIComponent(query)}`,
    );
    const hits = await resp.json();
    const geom = hits?.[0]?.geojson;
    if (geom && (geom.type === 'Polygon' || geom.type === 'MultiPolygon')) {
      state.boundaryGeoJson = geom;
      const [minLon, minLat, maxLon, maxLat] = computeGeomBbox(geom);
      const midLat = (minLat + maxLat) * 0.5;
      const midLon = (minLon + maxLon) * 0.5;
      const mPerDegLat = 111320.0;
      const mPerDegLon = Math.max(10000.0, 111320.0 * Math.cos((midLat * Math.PI) / 180));
      const spanXm = (maxLon - minLon) * mPerDegLon * 1.06;
      const spanYm = (maxLat - minLat) * mPerDegLat * 1.06;
      const aspect = Math.max(0.4, Math.min(2.5, spanXm / Math.max(1, spanYm)));
      const rows = Math.max(24, Math.min(54, Math.round(Math.sqrt(1800 / aspect))));
      const cols = Math.max(24, Math.min(54, Math.round(rows * aspect)));
      const dxM = Math.max(45.0, Math.min(2400.0, Math.max(spanXm / cols, spanYm / rows)));
      state.center = { lat: midLat, lon: midLon };
      state.boundaryGrid = { cols, rows, dxM };
      state.cachedDem = null;
      applyRegionBoundary();
      map.fitBounds([[minLon, minLat], [maxLon, maxLat]], {
        padding: { top: 64, bottom: 74, left: 255, right: 32 },
        pitch: 8,
        bearing: 0,
        duration: 0,
      });
      await runSimulation(false);
      return true;
    }
  } catch (_) { /* offline */ }
  return false;
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
  for (let i = 0; i < 360; i++) {
    state.particles.push({
      x: Math.random(),
      y: Math.random(),
      age: Math.floor(Math.random() * 60),
      maxAge: 45 + Math.floor(Math.random() * 55),
    });
  }
}

// Hydrological blue depth ramp: dry/shallow (< 0.025m) is 100% transparent
function colorForDepth(h) {
  if (h < 0.06) return null;
  const fade = Math.min(1, (h - 0.06) / 0.04);
  const stops = [
    [0.06,   56, 189, 248, 175], // #38bdf8 crisp sky (shallow flood)
    [0.14,    2, 132, 199, 212], // #0284c7 river blue
    [0.32,   29,  78, 216, 232], // #1d4ed8 cobalt
    [0.65,   30,  58, 138, 242], // #1e3a8a deep blue
    [1.10,   30,  27,  75, 248], // #1e1b4b deep indigo
  ];
  if (h >= stops[stops.length - 1][0]) {
    const last = stops[stops.length - 1];
    return [last[1], last[2], last[3], last[4]];
  }
  for (let i = 0; i < stops.length - 1; i++) {
    const a = stops[i];
    const b = stops[i + 1];
    if (h <= b[0]) {
      const k = (h - a[0]) / (b[0] - a[0]);
      return [
        a[1] + k * (b[1] - a[1]),
        a[2] + k * (b[2] - a[2]),
        a[3] + k * (b[3] - a[3]),
        (a[4] + k * (b[4] - a[4])) * fade,
      ];
    }
  }
  return null;
}

function colorForVelocity(v) {
  if (v < 0.04) return null;
  const t = Math.min(1, v / 1.4);
  if (t < 0.5) {
    const k = t / 0.5;
    return [45 - k * 43, 212 - k * 80, 191 + k * 8, 155 + k * 55];
  }
  const k = (t - 0.5) / 0.5;
  return [2 + k * 77, 132 - k * 62, 199 + k * 30, 210 + k * 30];
}

// DEFRA FD2320 Flood Hazard: HR = h * (v + 0.5)
function colorForHazard(hr) {
  if (hr < 0.04) return null;
  if (hr < 0.75) return [56, 189, 248, 165];
  if (hr < 1.25) return [250, 204, 21, 195];
  if (hr < 2.0)  return [249, 115, 22, 215];
  return [225, 29, 72, 232];
}

// Colorblind-safe diverging Teal (safer < 0) <-> Crimson (worse > 0)
function colorForDiff(d) {
  if (Math.abs(d) < 0.012) return null;
  const mag = Math.min(1, Math.abs(d) / 0.14);
  if (d < 0) return [13, 148, 136, 110 + mag * 125];
  return [225, 29, 72, 110 + mag * 125];
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
          data[out] = 225; data[out + 1] = 29; data[out + 2] = 72; data[out + 3] = 110;
        }
        continue;
      }

      let rgba = null;
      if (state.layer === 'depth') {
        const src = state.envelopeMode ? env.max_depth_m : frame.depth;
        const h = sampleBilinear(src, meta.active_mask, cols, rows, fx, fy);
        if (h !== null) rgba = colorForDepth(h);
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
        data[out + 3] = Math.round(rgba[3]);
      }
    }
  }
  rasterCtx.putImageData(img, 0, 0);

  const [minLon, minLat, maxLon, maxLat] = meta.wgs84_bounds;
  if (state.boundaryGeoJson) {
    rasterCtx.save();
    rasterCtx.globalCompositeOperation = 'destination-in';
    rasterCtx.beginPath();
    const traceRing = (ring) => {
      for (let i = 0; i < ring.length; i++) {
        const x = ((ring[i][0] - minLon) / (maxLon - minLon)) * W;
        const y = ((maxLat - ring[i][1]) / (maxLat - minLat)) * H;
        if (i === 0) rasterCtx.moveTo(x, y);
        else rasterCtx.lineTo(x, y);
      }
      rasterCtx.closePath();
    };
    const geom = state.boundaryGeoJson;
    if (geom.type === 'Polygon' && Array.isArray(geom.coordinates?.[0])) {
      traceRing(geom.coordinates[0]);
    } else if (geom.type === 'MultiPolygon' && Array.isArray(geom.coordinates)) {
      for (const poly of geom.coordinates) {
        if (Array.isArray(poly?.[0])) traceRing(poly[0]);
      }
    }
    rasterCtx.fillStyle = '#000000';
    rasterCtx.fill('evenodd');
    rasterCtx.restore();
  }
  const coords = [[minLon, maxLat], [maxLon, maxLat], [maxLon, minLat], [minLon, minLat]];
  const url = rasterCanvas.toDataURL('image/png');
  if (map.getSource('arus-water-img')) {
    map.getSource('arus-water-img').updateImage({ url, coordinates: coords });
  } else {
    map.addSource('arus-water-img', { type: 'image', url, coordinates: coords });
    const anchor = map.getLayer('arus-river-halo') ? 'arus-river-halo' : 'arus-particles-line';
    map.addLayer(
      {
        id: 'arus-water-smooth',
        type: 'raster',
        source: 'arus-water-img',
        paint: { 'raster-opacity': 0.85, 'raster-resampling': 'linear', 'raster-fade-duration': 0 },
      },
      anchor,
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
      if (!meta.active_mask[idx] || h < 0.03 || sp < 0.02 || p.age >= p.maxAge) {
        p.x = Math.random();
        p.y = Math.random();
        p.age = 0;
        continue;
      }
      const lon0 = minLon + p.x * (maxLon - minLon);
      const lat0 = minLat + p.y * (maxLat - minLat);
      const step = 0.015;
      const nx = p.x + vx * step;
      const ny = p.y + vy * step;
      const lon1 = minLon + nx * (maxLon - minLon);
      const lat1 = minLat + ny * (maxLat - minLat);
      const opacity = Math.min(0.85, 0.32 + sp * 0.65) * (1 - p.age / p.maxAge);
      particleFeatures.push({
        type: 'Feature',
        geometry: { type: 'LineString', coordinates: [[lon0, lat0], [lon1, lat1]] },
        properties: { color: '#ffffff', opacity },
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

  const ivFeatures = state.interventions.map((iv) => {
    const lon0 = minLon + iv.col * dLon;
    const lat0 = minLat + iv.row * dLat;
    return {
      type: 'Feature',
      geometry: {
        type: 'Polygon',
        coordinates: [[[lon0, lat0], [lon0 + dLon, lat0], [lon0 + dLon, lat0 + dLat], [lon0, lat0 + dLat], [lon0, lat0]]],
      },
      properties: { color: iv.delta_m > 0 ? '#d97706' : '#0d9488' },
    };
  });

  const xsecFeatures = [];
  if (state.xsecPts.length === 2) {
    const [a, b] = state.xsecPts;
    xsecFeatures.push({
      type: 'Feature',
      geometry: {
        type: 'LineString',
        coordinates: [
          [minLon + (a.col + 0.5) * dLon, minLat + (a.row + 0.5) * dLat],
          [minLon + (b.col + 0.5) * dLon, minLat + (b.row + 0.5) * dLat],
        ],
      },
      properties: {},
    });
  }

  const pSrc = map.getSource('arus-particles');
  const ivSrc = map.getSource('arus-iv-polys');
  const xSrc = map.getSource('arus-xsec');
  if (pSrc) pSrc.setData({ type: 'FeatureCollection', features: particleFeatures });
  if (ivSrc) ivSrc.setData({ type: 'FeatureCollection', features: ivFeatures });
  if (xSrc) xSrc.setData({ type: 'FeatureCollection', features: xsecFeatures });
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
    el.textContent = `${lat.toFixed(4)}°, ${lon.toFixed(4)}°`;
    return;
  }
  const meta = state.grid.metadata;
  if (!meta.active_mask[hit.idx]) {
    el.textContent = 'NODATA (out of coverage)';
    return;
  }
  const frame = state.frames[Math.min(state.frameIdx, state.frames.length - 1)];
  const z = meta.bed_m[hit.idx];
  const h = frame.depth[hit.idx];
  const sp = Math.hypot(frame.vx[hit.idx], frame.vy[hit.idx]);
  el.textContent = `z ${z.toFixed(1)}m · h ${h.toFixed(2)}m · |v| ${sp.toFixed(2)}m/s`;
}

function handleMapClick(lon, lat) {
  if (state.tool === 'pick') {
    state.center = { lon, lat };
    state.uploadedDem = null;
    state.cachedDem = null;
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
    renderFloodCanvas(false);
    return;
  }

  if (state.tool === 'xsection') {
    if (state.xsecPts.length >= 2) state.xsecPts = [];
    state.xsecPts.push({ col: hit.col, row: hit.row });
    if (state.xsecPts.length === 2) renderCrossSectionInspector();
    renderFloodCanvas(false);
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
  $('inspector').classList.remove('hidden');
  $('inspector-svg').classList.remove('hidden');

  if (!meta.active_mask[idx]) {
    $('inspector-title').textContent = `Cell (${col}, ${row}) · Out of coverage`;
    $('inspector-svg').innerHTML = `<text x="170" y="66" text-anchor="middle" fill="#e11d48" font-family="Inter, sans-serif" font-size="11" font-weight="500">NODATA — never treated as dry</text>`;
    $('inspector-stats').innerHTML = '';
    return;
  }

  const times = state.frames.map((f) => f.time_s);
  const depths = state.frames.map((f) => f.depth[idx]);
  const tMax = Math.max(1, times[times.length - 1]);
  const dPeak = Math.max(env.max_depth_m[idx], ...depths);
  const yMax = Math.max(dPeak * 1.15, env.threshold_m * 1.3, 0.05);

  const pts = times.map((t, i) => {
    const x = 38 + (t / tMax) * 288;
    const y = 104 - (depths[i] / yMax) * 82;
    return `${x.toFixed(1)},${y.toFixed(1)}`;
  }).join(' ');
  const ty = 104 - (env.threshold_m / yMax) * 82;
  const curT = state.frames[Math.min(state.frameIdx, state.frames.length - 1)].time_s;
  const cx = 38 + (curT / tMax) * 288;

  $('inspector-title').textContent = `Hydrograph (${col}, ${row})`;
  $('inspector-svg').innerHTML = `
    <line x1="38" y1="22" x2="326" y2="22" stroke="rgba(100,116,139,0.22)"/>
    <line x1="38" y1="104" x2="326" y2="104" stroke="rgba(100,116,139,0.45)"/>
    <polygon points="${pts} 326,104 38,104" fill="rgba(2,132,199,0.18)"/>
    <polyline points="${pts}" fill="none" stroke="#0284c7" stroke-width="2"/>
    <line x1="38" y1="${ty.toFixed(1)}" x2="326" y2="${ty.toFixed(1)}" stroke="#e11d48" stroke-dasharray="4 3" stroke-width="1.2"/>
    <line x1="${cx.toFixed(1)}" y1="18" x2="${cx.toFixed(1)}" y2="104" stroke="#d97706" stroke-width="1.2"/>
    <text x="34" y="25" text-anchor="end" fill="#64748b" font-family="JetBrains Mono, monospace" font-size="10">${yMax.toFixed(2)}</text>
    <text x="34" y="107" text-anchor="end" fill="#64748b" font-family="JetBrains Mono, monospace" font-size="10">0.00</text>
    <text x="324" y="${(ty - 4).toFixed(1)}" text-anchor="end" fill="#e11d48" font-family="Inter, sans-serif" font-size="10" font-weight="500">Threshold ${env.threshold_m.toFixed(2)}m</text>
  `;
  const onset = env.onset_time_s[idx] >= 0 ? `${env.onset_time_s[idx].toFixed(0)}s` : '—';
  const rec = depths[depths.length - 1] >= env.threshold_m ? 'Active' : 'Receded';
  $('inspector-stats').innerHTML = `
    <div class="mini-stat"><div class="mini-k">Peak Depth</div><div class="mini-v">${dPeak.toFixed(2)} m</div></div>
    <div class="mini-stat"><div class="mini-k">Max Speed</div><div class="mini-v">${env.max_speed_m_s[idx].toFixed(2)} m/s</div></div>
    <div class="mini-stat"><div class="mini-k">Onset</div><div class="mini-v">${onset}</div></div>
    <div class="mini-stat"><div class="mini-k">Status</div><div class="mini-v">${rec}</div></div>
  `;
}

function renderCrossSectionInspector() {
  if (state.xsecPts.length !== 2 || !state.grid || !state.frames.length) return;
  const meta = state.grid.metadata;
  const frame = state.frames[Math.min(state.frameIdx, state.frames.length - 1)];
  const [a, b] = state.xsecPts;
  const steps = 40;
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
  const zMin = Math.min(...beds) - 0.15;
  const zMax = Math.max(...wses) + 0.25;
  const span = Math.max(0.25, zMax - zMin);
  const bedPts = beds.map((z, i) => `${(38 + (i / steps) * 288).toFixed(1)},${(104 - ((z - zMin) / span) * 82).toFixed(1)}`).join(' ');
  const wsePts = wses.map((z, i) => `${(38 + (i / steps) * 288).toFixed(1)},${(104 - ((z - zMin) / span) * 82).toFixed(1)}`).join(' ');
  const bedRev = [...beds].reverse().map((z, i) => `${(38 + ((steps - i) / steps) * 288).toFixed(1)},${(104 - ((z - zMin) / span) * 82).toFixed(1)}`).join(' ');

  const lenM = Math.hypot((b.col - a.col) * meta.dx_m, (b.row - a.row) * meta.dy_m);
  const maxH = Math.max(...wses.map((w, i) => w - beds[i]));

  $('inspector').classList.remove('hidden');
  $('inspector-svg').classList.remove('hidden');
  $('inspector-title').textContent = `Cross-Section A → B (${lenM.toFixed(0)} m)`;
  $('inspector-svg').innerHTML = `
    <polygon points="${wsePts} ${bedRev}" fill="rgba(2,132,199,0.42)"/>
    <polygon points="${bedPts} 326,104 38,104" fill="rgba(100,116,139,0.28)"/>
    <polyline points="${bedPts}" fill="none" stroke="#64748b" stroke-width="1.6"/>
    <polyline points="${wsePts}" fill="none" stroke="#0284c7" stroke-width="2"/>
    <text x="34" y="25" text-anchor="end" fill="#64748b" font-family="JetBrains Mono, monospace" font-size="10">${zMax.toFixed(1)}m</text>
    <text x="34" y="107" text-anchor="end" fill="#64748b" font-family="JetBrains Mono, monospace" font-size="10">${zMin.toFixed(1)}m</text>
    <text x="40" y="118" fill="#64748b" font-family="Inter, sans-serif" font-size="10" font-weight="500">A</text>
    <text x="324" y="118" text-anchor="end" fill="#64748b" font-family="Inter, sans-serif" font-size="10" font-weight="500">B (${lenM.toFixed(0)}m)</text>
  `;
  $('inspector-stats').innerHTML = `
    <div class="mini-stat"><div class="mini-k">Length</div><div class="mini-v">${lenM.toFixed(0)} m</div></div>
    <div class="mini-stat"><div class="mini-k">Max Depth</div><div class="mini-v">${maxH.toFixed(2)} m</div></div>
    <div class="mini-stat"><div class="mini-k">Bed Min</div><div class="mini-v">${Math.min(...beds).toFixed(2)} m</div></div>
    <div class="mini-stat"><div class="mini-k">WSE Max</div><div class="mini-v">${Math.max(...wses).toFixed(2)} m</div></div>
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

// Gentle 1-pass 3x3 smoothing to remove single-pixel Terrarium quantization pits while keeping channels
function smoothScreeningDem(raw, cols, rows) {
  const out = new Array(raw.length);
  for (let r = 0; r < rows; r++) {
    for (let c = 0; c < cols; c++) {
      const idx = r * cols + c;
      const center = raw[idx];
      if (center === null) { out[idx] = null; continue; }
      let sum = 0;
      let cnt = 0;
      for (let dr = -1; dr <= 1; dr++) {
        for (let dc = -1; dc <= 1; dc++) {
          if (dr === 0 && dc === 0) continue;
          const nr = r + dr;
          const nc = c + dc;
          if (nr >= 0 && nr < rows && nc >= 0 && nc < cols) {
            const v = raw[nr * cols + nc];
            if (v !== null) { sum += v; cnt++; }
          }
        }
      }
      out[idx] = cnt > 0 ? center * 0.55 + (sum / cnt) * 0.45 : center;
    }
  }
  return out;
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

  const zoom = dxM >= 600 ? 10 : dxM >= 200 ? 12 : 14;
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

  const raw = [];
  let validCount = 0;
  let minZ = Infinity;
  let maxZ = -Infinity;
  for (const s of samples) {
    const data = tileData.get(s.tkey);
    if (!data) { raw.push(null); continue; }
    const off = (s.py * 256 + s.px) * 4;
    const z = (data[off] * 256 + data[off + 1] + data[off + 2] / 256) - 32768;
    if (Number.isFinite(z)) {
      raw.push(z);
      validCount++;
      if (z < minZ) minZ = z;
      if (z > maxZ) maxZ = z;
    } else {
      raw.push(null);
    }
  }
  if (validCount < cols * rows * 0.8 || (maxZ - minZ) < 0.2) return null;
  const elevations = smoothScreeningDem(raw, cols, rows);
  state.cachedDem = { key, elevations };
  return elevations;
}

async function buildRequestPayload() {
  const landChoice = $('ctl-soil').value === 'green-ampt'
    ? { kind: 'modified-green-ampt', suction_m: 0.11, conductivity_m_per_s: 0.000015, maximum_deficit: 0.35 }
    : { kind: 'impervious' };
  const boundary = $('ctl-boundary').value === 'closed'
    ? { kind: 'closed' }
    : { kind: 'transmissive' };
  const cols = state.boundaryGrid ? state.boundaryGrid.cols : 32;
  const rows = state.boundaryGrid ? state.boundaryGrid.rows : 28;
  const dxM = state.boundaryGrid ? state.boundaryGrid.dxM : 45.0;
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
    antecedent_rain_mm: state.antecedentMm || 0,
    hourly_rain_mm: state.hourlyRainMm || [],
    spatial_rain_grid: state.spatialRainGrid || null,
    hourly_tide_m: state.hourlyTideM || [],
    drainage_mm_per_hr: state.drainageMmHr || 0,
    evaporation_mm_per_day: state.evapMmDay || 0,
    observed_gauges: state.observedGauges || [],
    wind_x_m_per_s: parseFloat($('ctl-wind').value) || 0,
    wind_y_m_per_s: 0,
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
  state.frameIdx = state.playing
    ? Math.min(Math.floor(state.frames.length * 0.72), state.frames.length - 1)
    : 0;
  rebuildBoundaryCellMask();
  seedParticles();
  ensureMapLayers();
  updateDomainOutline(state.grid.metadata.wgs84_bounds);
  lastRasterKey = '';

  const scrubber = $('scrubber');
  scrubber.max = String(Math.max(0, state.frames.length - 1));
  scrubber.value = String(state.frameIdx);
  updateHonestyPill();
  updateTimeReadout();
  renderFloodCanvas(true);
}

async function runSimulation(compareMode) {
  const btn = compareMode ? $('btn-compare') : $('btn-run');
  const orig = btn.innerHTML;
  btn.disabled = true;
  btn.innerHTML = '<span class="spinner"></span>Solving…';
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
      $('inspector-svg').classList.add('hidden');
      $('inspector-title').textContent = 'Intervention Δh Impact';
      $('inspector-stats').innerHTML = `
        <div class="mini-stat"><div class="mini-k">Protected</div><div class="mini-v good">${state.diffData.improved_cells} cells (−${state.diffData.max_reduction_m.toFixed(2)}m)</div></div>
        <div class="mini-stat"><div class="mini-k">Worsened</div><div class="mini-v bad">${state.diffData.worsened_cells} cells (+${state.diffData.max_worsening_m.toFixed(2)}m)</div></div>
        <div class="mini-stat"><div class="mini-k">Earthwork</div><div class="mini-v">${state.diffData.earthwork.fill_volume_m3.toFixed(0)} m³ fill</div></div>
        <div class="mini-stat"><div class="mini-k">Mass Check</div><div class="mini-v good">${state.diffData.conservation_valid ? 'PASS' : 'FAIL'}</div></div>
      `;
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
      if (!state.fixedView) {
        const [minLon, minLat, maxLon, maxLat] = summary.wgs84_bounds;
        map.flyTo({
          center: [(minLon + maxLon) * 0.5, (minLat + maxLat) * 0.5],
          zoom: 14.4,
          pitch: state.is3D ? 48 : 0,
          duration: 800,
        });
      }
    }
  } catch (err) {
    $('inspector').classList.remove('hidden');
    $('inspector-svg').classList.add('hidden');
    $('inspector-title').textContent = 'Simulation Notice';
    $('inspector-stats').innerHTML = `<div class="mini-stat" style="grid-column:span 2"><div class="mini-k">Error</div><div class="mini-v bad">${String(err.message).slice(0, 96)}</div></div>`;
  } finally {
    btn.disabled = false;
    btn.innerHTML = orig;
  }
}

function updateFrameTelemetry() {
  if (!map) return;
  const c = map.getCenter();
  const z = map.getZoom();
  let rainPart;
  if (state.spatialRainGrid && Array.isArray(state.spatialRainGrid.hourly_mm) && state.spatialRainGrid.hourly_mm.length > 0 && state.frames.length > 0) {
    const nHours = state.spatialRainGrid.hourly_mm[0].length;
    const frac = state.frames.length > 1
      ? Math.min(state.frameIdx, state.frames.length - 1) / (state.frames.length - 1)
      : 0;
    const hIdx = Math.min(nHours - 1, Math.floor(frac * nHours));
    let sum = 0;
    for (const pt of state.spatialRainGrid.hourly_mm) sum += (pt[hIdx] || 0);
    const meanRate = sum / state.spatialRainGrid.hourly_mm.length;
    rainPart = `${meanRate.toFixed(1)} mm/hr spatial NWP`;
  } else if (state.hourlyRainMm && state.hourlyRainMm.length > 0 && state.frames.length > 0) {
    const frac = state.frames.length > 1
      ? Math.min(state.frameIdx, state.frames.length - 1) / (state.frames.length - 1)
      : 0;
    const hIdx = Math.min(
      state.hourlyRainMm.length - 1,
      Math.floor(frac * state.hourlyRainMm.length),
    );
    const curRate = state.hourlyRainMm[hIdx] || 0;
    rainPart = `${curRate.toFixed(1)} mm/hr NWP`;
  } else {
    const rain = $('ctl-rain')?.value || '0';
    rainPart = `${rain} mm/hr`;
  }
  const wind = parseFloat($('ctl-wind')?.value || '0') || 0;
  const windPart = wind > 0 ? ` · ${wind} m/s` : '';
  $('telemetry').textContent = `${c.lat.toFixed(4)}°, ${c.lng.toFixed(4)}° · z${z.toFixed(1)} · ${rainPart}${windPart}`;
}

function updateHonestyPill() {
  const pill = $('honesty-pill');
  const txt = $('honesty-text');
  if (!state.grid) return;
  const ok = state.grid.budget_assessment.passed;
  const res = Math.abs(state.grid.water_budget.residual_m3);
  const prov = state.grid.metadata.terrain_provenance;
  const demTag = prov === 'BrowserTerrariumScreening'
    ? 'Terrarium 30m'
    : prov === 'UploadedAsciiDem'
      ? 'Uploaded .ASC'
      : 'Synthetic DEM';
  pill.classList.toggle('fail', !ok);
  pill.title = `Mass residual: ${res.toExponential(1)} m³ (screening only)`;
  txt.textContent = `${demTag} · Mass ${ok ? 'OK' : 'FAIL'}`;
  updateFrameTelemetry();
}

function fmtMMSS(sec) {
  const s = Math.max(0, Math.round(sec));
  const m = Math.floor(s / 60);
  const rem = s % 60;
  return `${String(m).padStart(2, '0')}:${String(rem).padStart(2, '0')}`;
}

function fmtCalendarTime(epochMs, tzLabel) {
  const d = new Date(epochMs);
  const yyyy = d.getUTCFullYear();
  const mm = String(d.getUTCMonth() + 1).padStart(2, '0');
  const dd = String(d.getUTCDate()).padStart(2, '0');
  const hh = String(d.getUTCHours()).padStart(2, '0');
  const mi = String(d.getUTCMinutes()).padStart(2, '0');
  const ss = String(d.getUTCSeconds()).padStart(2, '0');
  return `${yyyy}-${mm}-${dd} ${hh}:${mi}:${ss} ${tzLabel}`;
}

const DAYS_SHORT = ['Sun', 'Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat'];

function fmtCalloutTime(epochMs, tzLabel) {
  const d = new Date(epochMs);
  const dow = DAYS_SHORT[d.getUTCDay()];
  const day = d.getUTCDate();
  const h24 = d.getUTCHours();
  const mi = d.getUTCMinutes();
  const ampm = h24 >= 12 ? 'PM' : 'AM';
  const h12 = h24 % 12 || 12;
  const minPart = mi === 0 ? '' : `:${String(mi).padStart(2, '0')}`;
  return `${dow} ${day} · ${h12}${minPart} ${ampm} ${tzLabel}`;
}

function renderForecastSegments() {
  const bar = $('forecast-bar');
  const tl = $('timeline');
  if (!bar || !tl) return;
  if (!(state.spanHours > 0)) {
    bar.classList.add('hidden');
    tl.classList.remove('hidden');
    return;
  }
  tl.classList.add('hidden');
  bar.classList.remove('hidden');
  const segEl = $('fb-segments');
  const startMs = state.startEpochMs;
  const spanMs = state.spanHours * 3600_000;
  const segs = [];
  if (state.spanHours >= 24) {
    const days = Math.max(2, Math.min(10, Math.round(state.spanHours / 24)));
    for (let i = 0; i < days; i++) {
      const d = new Date(startMs + i * 86_400_000);
      const dowIdx = d.getUTCDay();
      segs.push(`<div class="fb-seg" data-seg="${i}">${DAYS_SHORT[dowIdx]} ${d.getUTCDate()}</div>`);
    }
  } else {
    const slots = Math.max(4, Math.min(8, Math.round(state.spanHours / 3) || 4));
    for (let i = 0; i < slots; i++) {
      const d = new Date(startMs + (spanMs * i) / slots);
      const hh = String(d.getUTCHours()).padStart(2, '0');
      const mi = String(d.getUTCMinutes()).padStart(2, '0');
      segs.push(`<div class="fb-seg" data-seg="${i}">${hh}:${mi}</div>`);
    }
  }
  segEl.innerHTML = segs.join('');
}

function pointInRing(lon, lat, ring) {
  let inside = false;
  for (let i = 0, j = ring.length - 1; i < ring.length; j = i++) {
    const xi = ring[i][0], yi = ring[i][1];
    const xj = ring[j][0], yj = ring[j][1];
    const intersect = ((yi > lat) !== (yj > lat))
      && (lon < ((xj - xi) * (lat - yi)) / ((yj - yi) || 1e-12) + xi);
    if (intersect) inside = !inside;
  }
  return inside;
}

function pointInGeom(lon, lat, geom) {
  if (!geom) return true;
  if (geom.type === 'Polygon' && Array.isArray(geom.coordinates?.[0])) {
    return pointInRing(lon, lat, geom.coordinates[0]);
  }
  if (geom.type === 'MultiPolygon' && Array.isArray(geom.coordinates)) {
    for (const poly of geom.coordinates) {
      if (Array.isArray(poly?.[0]) && pointInRing(lon, lat, poly[0])) return true;
    }
    return false;
  }
  return true;
}

function rebuildBoundaryCellMask() {
  if (!state.grid) { state.boundaryCellMask = null; return; }
  const meta = state.grid.metadata;
  const { cols, rows, active_mask } = meta;
  const [minLon, minLat, maxLon, maxLat] = meta.wgs84_bounds;
  const mask = new Uint8Array(cols * rows);
  for (let r = 0; r < rows; r++) {
    const lat = minLat + ((r + 0.5) / rows) * (maxLat - minLat);
    for (let c = 0; c < cols; c++) {
      const idx = r * cols + c;
      if (!active_mask[idx]) continue;
      const lon = minLon + ((c + 0.5) / cols) * (maxLon - minLon);
      if (!state.boundaryGeoJson || pointInGeom(lon, lat, state.boundaryGeoJson)) {
        mask[idx] = 1;
      }
    }
  }
  state.boundaryCellMask = mask;
}

function updateLiveMetrics() {
  const card = $('live-metrics');
  if (!card || !state.grid || !state.frames.length) return;
  const isPres = document.documentElement.dataset.presentation === 'true';
  const show = isPres || !$('public-outlook').classList.contains('hidden');
  card.classList.toggle('hidden', !show);
  if (!show) return;

  if (!state.boundaryCellMask || state.boundaryCellMask.length !== state.grid.metadata.cols * state.grid.metadata.rows) {
    rebuildBoundaryCellMask();
  }
  const meta = state.grid.metadata;
  const mask = state.boundaryCellMask;
  const idx = Math.min(state.frameIdx, state.frames.length - 1);
  const frame = state.frames[idx];
  const depth = frame.depth;
  const vx = frame.vx;
  const vy = frame.vy;
  let totalCells = 0;
  let floodedCells = 0;
  let stagnantCells = 0;
  let sumDepth = 0;
  let peakDepth = 0;
  for (let i = 0; i < depth.length; i++) {
    if (!mask[i]) continue;
    totalCells++;
    const h = depth[i];
    if (h > 0) sumDepth += h;
    if (h >= 0.06) {
      floodedCells++;
      if (Math.hypot(vx[i], vy[i]) < 0.03) stagnantCells++;
    }
    if (h > peakDepth) peakDepth = h;
  }
  const avgDepth = totalCells > 0 ? sumDepth / totalCells : 0;
  const floodedPct = totalCells > 0 ? (floodedCells / totalCells) * 100 : 0;
  const cellKm2 = (meta.dx_m * meta.dx_m) / 1_000_000;
  const stagnantKm2 = stagnantCells * cellKm2;

  // Area-relative flood severity index in [0, 1] combining mean depth (0..0.45m) and flooded fraction
  const depthScore = Math.min(1, avgDepth / 0.45);
  const areaScore = Math.min(1, floodedPct / 85);
  const score = Math.min(1, depthScore * 0.65 + areaScore * 0.35);

  const badge = $('lm-badge');
  let lvl = 'low';
  let label = 'LOW';
  if (score >= 0.72) { lvl = 'danger'; label = 'DANGER'; }
  else if (score >= 0.46) { lvl = 'high'; label = 'HIGH'; }
  else if (score >= 0.22) { lvl = 'moderate'; label = 'MODERATE'; }
  badge.className = `lm-badge ${lvl}`;
  badge.textContent = label;
  const trendEl = $('lm-trend');
  if (trendEl) {
    if (idx === 0) {
      trendEl.className = 'lm-trend';
      trendEl.textContent = 'Current';
    } else {
      const lookback = Math.max(0, idx - 3);
      const prevDepth = state.frames[lookback].depth;
      let prevSum = 0;
      let prevFlooded = 0;
      for (let i = 0; i < prevDepth.length; i++) {
        if (!mask[i]) continue;
        if (prevDepth[i] > 0) prevSum += prevDepth[i];
        if (prevDepth[i] >= 0.06) prevFlooded++;
      }
      const prevAvg = totalCells > 0 ? prevSum / totalCells : 0;
      const prevPct = totalCells > 0 ? (prevFlooded / totalCells) * 100 : 0;
      const dAvg = avgDepth - prevAvg;
      const dPct = floodedPct - prevPct;
      if (dAvg > 0.0004 || dPct > 0.6) {
        trendEl.className = 'lm-trend rising';
        trendEl.textContent = 'Rising ▲';
      } else if (dAvg < -0.0004 || dPct < -0.6) {
        trendEl.className = 'lm-trend receding';
        trendEl.textContent = 'Receding ▼';
      } else {
        trendEl.className = 'lm-trend';
        trendEl.textContent = 'Steady ●';
      }
    }
  }
  $('lm-marker').style.left = `${(score * 100).toFixed(1)}%`;
  $('lm-avg').textContent = `${avgDepth.toFixed(2)} m`;
  $('lm-peak').textContent = `${peakDepth.toFixed(2)} m`;
  $('lm-pct').textContent = `${floodedPct.toFixed(0)}%`;
  const stagEl = $('lm-stagnant');
  if (stagEl) stagEl.textContent = `${stagnantKm2.toFixed(1)} km²`;
  updateLiveHotspots(idx);
}

function sampleLocalPeakDepth(frame, lat, lon) {
  if (!state.grid || !frame) return 0;
  const hit = lngLatToCell(lon, lat);
  if (!hit) return 0;
  const { cols, rows, active_mask } = state.grid.metadata;
  let best = 0;
  for (let dr = -1; dr <= 1; dr++) {
    for (let dc = -1; dc <= 1; dc++) {
      const r = hit.row + dr;
      const c = hit.col + dc;
      if (r < 0 || c < 0 || r >= rows || c >= cols) continue;
      const i = r * cols + c;
      if (active_mask[i] && frame.depth[i] > best) best = frame.depth[i];
    }
  }
  return best;
}

function updateLiveHotspots(frameIdx) {
  const spots = state.outlookHotspots;
  if (!Array.isArray(spots) || spots.length === 0 || !state.frames.length) return;
  const f0 = state.frames[0];
  const fCur = state.frames[Math.min(frameIdx, state.frames.length - 1)];
  const frac = state.frames.length > 1 ? frameIdx / (state.frames.length - 1) : 0;

  const pinFeatures = [];
  const rowsHtml = spots.map((s) => {
    let statusText = s.status || '';
    let tone = s.tone || '';

    if (Number.isFinite(s.lat) && Number.isFinite(s.lon)) {
      const h0 = sampleLocalPeakDepth(f0, s.lat, s.lon);
      const hCur = sampleLocalPeakDepth(fCur, s.lat, s.lon);
      if (s.kind === 'tide' && state.hourlyTideM && state.hourlyTideM.length > 0) {
        const tIdx = Math.min(state.hourlyTideM.length - 1, Math.floor(frac * state.hourlyTideM.length));
        const tideM = state.hourlyTideM[tIdx];
        const sign = tideM >= 0 ? '+' : '';
        const tag = tideM >= 1.85 ? 'HIGH TIDE' : tideM >= 1.2 ? 'Rising' : 'Low';
        tone = tideM >= 1.85 ? 'danger' : tideM >= 1.2 ? 'warn' : '';
        statusText = `${sign}${tideM.toFixed(2)} m · ${tag}`;
      } else if (Number.isFinite(s.base_stage_m)) {
        const liveStage = s.base_stage_m + (hCur - h0);
        if (Number.isFinite(s.alert_m)) {
          const diff = liveStage - s.alert_m;
          if (diff >= 0) {
            tone = 'danger';
            statusText = `${liveStage.toFixed(2)} m · ABOVE ALERT`;
          } else if (diff >= -0.4) {
            tone = 'warn';
            statusText = `${liveStage.toFixed(2)} m · ${diff.toFixed(2)} m Alert`;
          } else {
            tone = hCur >= 0.12 ? 'warn' : '';
            statusText = `${liveStage.toFixed(2)} m · ${hCur.toFixed(2)} m depth`;
          }
        } else {
          tone = hCur >= 0.35 ? 'danger' : hCur >= 0.12 ? 'warn' : '';
          statusText = `${liveStage.toFixed(2)} m · ${hCur.toFixed(2)} m`;
        }
      } else {
        tone = hCur >= 0.35 ? 'danger' : hCur >= 0.08 ? 'warn' : '';
        statusText = hCur >= 0.06 ? `${hCur.toFixed(2)} m depth` : 'Dry (<6 cm)';
      }

      pinFeatures.push({
        type: 'Feature',
        geometry: { type: 'Point', coordinates: [s.lon, s.lat] },
        properties: {
          name: s.name || '',
          color: tone === 'danger' ? '#e11d48' : tone === 'warn' ? '#d97706' : '#0284c7',
        },
      });
    }

    const cls = tone ? ` ${tone}` : '';
    return `<div class="po-spot"><span class="po-spot-name">${s.name}</span><span class="po-spot-val${cls}">${statusText}</span></div>`;
  }).join('');

  $('po-hotspots').innerHTML = rowsHtml;
  if (map && map.getSource('arus-hotspots') && pinFeatures.length > 0) {
    map.getSource('arus-hotspots').setData({ type: 'FeatureCollection', features: pinFeatures });
  }
}

function updateTimeReadout() {
  if (!state.frames.length) return;
  const idx = Math.min(state.frameIdx, state.frames.length - 1);
  const cur = state.frames[idx].time_s;
  const ts = fmtCalendarTime(state.startEpochMs + cur * 1000, state.tzLabel);
  $('time-readout').textContent = `${ts} (+${fmtMMSS(cur)})`;

  if (state.spanHours > 0) {
    const frac = state.frames.length > 1 ? idx / (state.frames.length - 1) : 0;
    const epochMs = state.startEpochMs + frac * state.spanHours * 3600_000;
    const pct = frac * 100;
    $('fb-fill').style.width = `${pct.toFixed(2)}%`;
    const callout = $('fb-callout');
    callout.textContent = fmtCalloutTime(epochMs, state.tzLabel);
    callout.style.left = `${Math.max(12, Math.min(88, pct)).toFixed(2)}%`;
    const segNodes = document.querySelectorAll('#fb-segments .fb-seg');
    if (segNodes.length > 0) {
      let activeIdx;
      if (state.spanHours >= 24) {
        const d0 = new Date(state.startEpochMs);
        const day0Midnight = Date.UTC(d0.getUTCFullYear(), d0.getUTCMonth(), d0.getUTCDate());
        const dCur = new Date(epochMs);
        const curMidnight = Date.UTC(dCur.getUTCFullYear(), dCur.getUTCMonth(), dCur.getUTCDate());
        activeIdx = Math.max(
          0,
          Math.min(segNodes.length - 1, Math.round((curMidnight - day0Midnight) / 86_400_000)),
        );
      } else {
        activeIdx = Math.min(segNodes.length - 1, Math.floor(frac * segNodes.length));
      }
      segNodes.forEach((n, i) => n.classList.toggle('active', i === activeIdx));
    }
  }
  updateFrameTelemetry();
  updateLiveMetrics();
}

function updateInterventionBar() {
  const count = state.interventions.length;
  $('iv-count').textContent = `${count} cells`;
  $('intervention-bar').classList.toggle('hidden', count === 0);
  $('btn-compare').classList.toggle('hidden', count === 0);
  $('btn-compare').disabled = count === 0;
}

function setTool(tool) {
  state.tool = tool;
  document.querySelectorAll('[data-tool]').forEach((el) => {
    el.classList.toggle('active', el.dataset.tool === tool);
  });
}

function setLayer(layer) {
  state.layer = layer;
  document.querySelectorAll('[data-layer]').forEach((el) => {
    el.classList.toggle('active', el.dataset.layer === layer);
  });
  const bar = $('legend-bar');
  const ticks = $('legend-ticks');
  const note = $('legend-note');
  if (layer === 'depth') {
    bar.style.background = 'linear-gradient(90deg, #38bdf8, #0284c7, #1d4ed8, #1e1b4b)';
    ticks.innerHTML = '<span>0.06m</span><span>0.30m</span><span>1.1m+</span>';
    note.textContent = 'Flood depth h (sheet runoff < 6 cm hidden).';
  } else if (layer === 'velocity') {
    bar.style.background = 'linear-gradient(90deg, #2dd4bf, #0284c7, #4f46e5)';
    ticks.innerHTML = '<span>0.0 m/s</span><span>0.7 m/s</span><span>1.4+ m/s</span>';
    note.textContent = 'Depth-averaged velocity |v| with streamlines.';
  } else if (layer === 'hazard') {
    bar.style.background = 'linear-gradient(90deg, #38bdf8 0% 25%, #facc15 25% 50%, #f97316 50% 75%, #e11d48 75% 100%)';
    ticks.innerHTML = '<span>Low</span><span>Mod</span><span>Ext</span>';
    note.textContent = 'DEFRA FD2320 hazard rating: h × (v + 0.5).';
  } else {
    bar.style.background = 'linear-gradient(90deg, #0d9488, #cbd5e1 50%, #e11d48)';
    ticks.innerHTML = '<span>−0.14m Safer</span><span>0</span><span>+0.14m Worse</span>';
    note.textContent = 'Max depth change: Intervention minus Baseline.';
  }
  renderFloodCanvas(true);
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
  renderFloodCanvas(false);
  requestAnimationFrame(animationLoop);
}

async function fetchLiveMultiModelNwp() {
  const btn = $('btn-nwp');
  const card = $('nwp-card');
  const badge = $('nwp-agree');
  const rows = $('nwp-rows');
  btn.disabled = true;
  btn.textContent = 'Fetching…';
  try {
    const { lat, lon } = state.center;
    const url = `https://api.open-meteo.com/v1/forecast?latitude=${lat.toFixed(4)}&longitude=${lon.toFixed(4)}&models=ecmwf_ifs025,gfs_global,icon_seamless&hourly=precipitation,soil_moisture_0_to_7cm&past_days=2&forecast_days=5&timezone=GMT`;
    const resp = await fetch(url);
    if (!resp.ok) throw new Error(`HTTP ${resp.status}`);
    const data = await resp.json();
    const h = data.hourly || {};
    const models = [
      { id: 'ecmwf_ifs025', label: 'ECMWF' },
      { id: 'gfs_global', label: 'GFS' },
      { id: 'icon_seamless', label: 'ICON' },
    ];
    const stats = models.map((m) => {
      const arr = (h[`precipitation_${m.id}`] || []).slice(48);
      const total = arr.reduce((acc, v) => acc + (typeof v === 'number' ? v : 0), 0);
      let peak24 = 0;
      for (let i = 0; i + 24 <= arr.length; i++) {
        let s = 0;
        for (let j = i; j < i + 24; j++) s += typeof arr[j] === 'number' ? arr[j] : 0;
        if (s > peak24) peak24 = s;
      }
      const peakHr = arr.reduce((mx, v) => (typeof v === 'number' && v > mx ? v : mx), 0);
      const smArr = h[`soil_moisture_0_to_7cm_${m.id}`] || [];
      let latestSm = null;
      for (let i = Math.min(48, smArr.length - 1); i >= 0; i--) {
        if (typeof smArr[i] === 'number') { latestSm = smArr[i]; break; }
      }
      return { ...m, total, peak24, peakHr, latestSm };
    });

    const peaks = stats.map((s) => s.peak24);
    const maxP = Math.max(...peaks);
    const minP = Math.min(...peaks);
    const spread = maxP - minP;
    const ratio = maxP > 1e-6 ? spread / maxP : 0;
    const level = ratio >= 0.5 && spread >= 25 ? 'low' : ratio >= 0.25 && spread >= 10 ? 'moderate' : 'high';
    state.nwpPeakMmHr = Math.max(...stats.map((s) => s.peakHr));

    badge.className = `nwp-badge ${level}`;
    badge.textContent = `${level[0].toUpperCase()}${level.slice(1)} agreement`;
    rows.innerHTML = stats.map((s) => {
      const smTxt = s.latestSm !== null ? ` · SM ${s.latestSm.toFixed(2)}` : '';
      return `<div class="nwp-row"><span>${s.label}${smTxt}</span><strong>${s.total.toFixed(0)}mm (24h ${s.peak24.toFixed(0)})</strong></div>`;
    }).join('');
    card.classList.remove('hidden');
  } catch (err) {
    badge.className = 'nwp-badge low';
    badge.textContent = 'Offline / Error';
    rows.innerHTML = `<div class="nwp-row"><span>${String(err.message).slice(0, 36)}</span></div>`;
    card.classList.remove('hidden');
  } finally {
    btn.disabled = false;
    btn.textContent = 'Fetch Live';
  }
}

async function openSourcesModal() {
  $('sources-modal').classList.remove('hidden');
  const grid = $('sources-grid');
  grid.innerHTML = 'Loading catalog…';
  try {
    const resp = await fetch('/api/v1/catalog/sources');
    if (!resp.ok) throw new Error('Catalog not configured');
    const data = await resp.json();
    grid.innerHTML = data.sources.map((s) => `
      <div class="src-card">
        <strong>${s.title}</strong>
        <div><span class="src-badge ${s.access === 'public' ? 'pub' : ''}">${s.access}</span> <span class="src-badge">${s.data_class}</span></div>
        <div style="color:var(--ink-3)">${s.provider} · ${s.license.name}</div>
      </div>
    `).join('');
  } catch (e) {
    grid.innerHTML = `<div style="color:var(--danger)">${e.message}</div>`;
  }
}

function setCustomPresetLabel(label) {
  const sel = $('preset-select');
  if (!sel) return;
  let opt = sel.querySelector('option[value="__custom"]');
  if (!opt) {
    opt = document.createElement('option');
    opt.value = '__custom';
    sel.insertBefore(opt, sel.firstChild);
  }
  opt.textContent = label;
  sel.value = '__custom';
}

function wireUi() {
  const sel = $('preset-select');
  sel.innerHTML = PRESETS.map((p) => `<option value="${p.id}">${p.name}</option>`).join('');
  sel.addEventListener('change', () => {
    const p = PRESETS.find((x) => x.id === sel.value);
    if (!p) return;
    state.center = { lon: p.lon, lat: p.lat };
    state.uploadedDem = null;
    state.cachedDem = null;
    state.interventions = [];
    updateInterventionBar();
    map.flyTo({ center: [p.lon, p.lat], zoom: p.zoom, pitch: state.is3D ? 48 : 0, duration: 900 });
    runSimulation(false);
  });

  $('coord-search').addEventListener('keydown', async (e) => {
    if (e.key !== 'Enter') return;
    const raw = e.target.value.trim();
    if (!raw) return;
    const parts = raw.split(',').map((v) => parseFloat(v.trim()));
    if (parts.length === 2 && Number.isFinite(parts[0]) && Number.isFinite(parts[1])) {
      state.center = { lat: parts[0], lon: parts[1] };
      state.cachedDem = null;
      setCustomPresetLabel(`Custom · ${parts[0].toFixed(4)}, ${parts[1].toFixed(4)}`);
      map.flyTo({ center: [parts[1], parts[0]], zoom: 14.2, duration: 800 });
      runSimulation(false);
      return;
    }
    try {
      const resp = await fetch(`https://nominatim.openstreetmap.org/search?format=json&limit=1&q=${encodeURIComponent(raw)}`);
      const hits = await resp.json();
      if (hits && hits.length > 0) {
        const lat = parseFloat(hits[0].lat);
        const lon = parseFloat(hits[0].lon);
        state.center = { lat, lon };
        state.cachedDem = null;
        setCustomPresetLabel(raw);
        map.flyTo({ center: [lon, lat], zoom: 14.2, duration: 900 });
        runSimulation(false);
      }
    } catch (_) { /* offline geocoding unavailable */ }
  });

  document.querySelectorAll('[data-basemap]').forEach((el) => {
    el.addEventListener('click', () => setBasemap(el.dataset.basemap));
  });
  document.querySelectorAll('[data-tool]').forEach((el) => {
    el.addEventListener('click', () => setTool(el.dataset.tool));
  });
  document.querySelectorAll('[data-layer]').forEach((el) => {
    el.addEventListener('click', () => setLayer(el.dataset.layer));
  });

  $('btn-2d3d').addEventListener('click', () => {
    state.is3D = !state.is3D;
    $('btn-2d3d').textContent = state.is3D ? '3D' : '2D';
    $('btn-2d3d').classList.toggle('active', state.is3D);
    map.easeTo({ pitch: state.is3D ? 48 : 0, bearing: state.is3D ? -16 : 0, duration: 500 });
  });

  $('btn-env').addEventListener('click', () => {
    state.envelopeMode = !state.envelopeMode;
    $('btn-env').textContent = state.envelopeMode ? 'Peak' : 'Live';
    $('btn-env').classList.toggle('active', state.envelopeMode);
    renderFloodCanvas(true);
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
    renderFloodCanvas(true);
  });

  $('ctl-rain').addEventListener('input', (e) => { $('val-rain').textContent = `${e.target.value} mm/hr`; });
  $('ctl-dur').addEventListener('input', (e) => { $('val-dur').textContent = `${e.target.value} s`; });
  $('ctl-manning').addEventListener('input', (e) => { $('val-manning').textContent = Number(e.target.value).toFixed(3); });
  $('ctl-wind').addEventListener('input', (e) => { $('val-wind').textContent = `${e.target.value} m/s`; });

  $('btn-iv-undo').addEventListener('click', () => { state.interventions.splice(-4, 4); updateInterventionBar(); renderFloodCanvas(false); });
  $('btn-iv-clear').addEventListener('click', () => { state.interventions = []; updateInterventionBar(); renderFloodCanvas(false); });
  $('btn-run').addEventListener('click', () => runSimulation(false));
  $('btn-compare').addEventListener('click', () => runSimulation(true));
  $('btn-nwp').addEventListener('click', fetchLiveMultiModelNwp);
  $('btn-nwp-apply').addEventListener('click', () => {
    if (state.nwpPeakMmHr !== null) {
      const clamped = Math.min(400, Math.max(40, Math.round(state.nwpPeakMmHr / 10) * 10));
      $('ctl-rain').value = String(clamped);
      $('val-rain').textContent = `${clamped} mm/hr`;
      runSimulation(false);
    }
  });

  $('btn-export-asc').addEventListener('click', () => {
    if (state.runId) window.open(`/api/v1/runs/${state.runId}/export/max-depth.asc`, '_blank');
  });
  $('btn-export-csv').addEventListener('click', () => {
    if (state.runId) window.open(`/api/v1/runs/${state.runId}/export/outlooks.csv`, '_blank');
  });
  $('btn-snap').addEventListener('click', captureSnapshotPng);
  $('btn-rec').addEventListener('click', toggleVideoRecording);

  $('btn-sources').addEventListener('click', openSourcesModal);
  $('btn-sources-close').addEventListener('click', () => $('sources-modal').classList.add('hidden'));
  $('btn-inspector-close').addEventListener('click', () => {
    $('inspector').classList.add('hidden');
    state.xsecPts = [];
    state.selectedCell = null;
    renderFloodCanvas(false);
  });

  $('file-asc').addEventListener('change', async (e) => {
    const file = e.target.files?.[0];
    if (!file) return;
    const text = await file.text();
    const d = 0.006;
    state.uploadedDem = {
      ascii_content: text,
      projected_crs: 'EPSG:3375',
      vertical_datum: 'local-uploaded-datum',
      wgs84_bounds: [state.center.lon - d, state.center.lat - d, state.center.lon + d, state.center.lat + d],
    };
    runSimulation(false);
  });
}

let activeRecorder = null;
let recordedChunks = [];

function downloadBlob(blob, filename) {
  const url = URL.createObjectURL(blob);
  const a = document.createElement('a');
  a.href = url;
  a.download = filename;
  a.click();
  setTimeout(() => URL.revokeObjectURL(url), 1000);
}

function captureSnapshotPng() {
  if (!map) return;
  map.getCanvas().toBlob((blob) => {
    if (blob) downloadBlob(blob, `arus-${state.runId || 'view'}.png`);
  }, 'image/png');
}

function toggleVideoRecording() {
  const btn = $('btn-rec');
  if (activeRecorder && activeRecorder.state === 'recording') {
    activeRecorder.stop();
    btn.classList.remove('recording');
    btn.textContent = 'Rec';
    return;
  }
  if (!map || typeof MediaRecorder === 'undefined') return;
  const stream = map.getCanvas().captureStream(30);
  const mime = MediaRecorder.isTypeSupported('video/webm;codecs=vp9')
    ? 'video/webm;codecs=vp9'
    : 'video/webm';
  recordedChunks = [];
  activeRecorder = new MediaRecorder(stream, { mimeType: mime, videoBitsPerSecond: 4_000_000 });
  activeRecorder.ondataavailable = (e) => {
    if (e.data && e.data.size > 0) recordedChunks.push(e.data);
  };
  activeRecorder.onstop = async () => {
    const blob = new Blob(recordedChunks, { type: mime });
    recordedChunks = [];
    const baseName = `arus-${state.runId || 'recording'}`;
    btn.disabled = true;
    btn.textContent = 'MP4…';
    try {
      const resp = await fetch('/api/v1/export/mp4', {
        method: 'POST',
        headers: { 'content-type': 'video/webm' },
        body: blob,
      });
      if (resp.ok) {
        const mp4Blob = await resp.blob();
        downloadBlob(mp4Blob, `${baseName}.mp4`);
      } else {
        downloadBlob(blob, `${baseName}.webm`);
      }
    } catch (_) {
      downloadBlob(blob, `${baseName}.webm`);
    } finally {
      btn.disabled = false;
      btn.textContent = 'Rec';
    }
  };
  activeRecorder.start();
  btn.classList.add('recording');
  btn.textContent = 'Stop';
}

// Public Director API for declarative storyboard scripts (scripts/record_storyboard.py)
window.arusDirector = {
  isReady: () => Boolean(state.grid && state.frames.length > 0),
  isSettled: () => Boolean(state.grid && state.frames.length > 0 && map && map.loaded()),
  getState: () => ({
    runId: state.runId,
    basemap: state.basemap,
    layer: state.layer,
    frameIdx: state.frameIdx,
    frameCount: state.frames.length,
    center: state.center,
    interventions: state.interventions.length,
  }),
  flyTo: (lat, lon, zoom = 14.3, pitch = 48, bearing = -16, duration = 900) => {
    state.center = { lat, lon };
    state.cachedDem = null;
    setCustomPresetLabel(`Custom · ${lat.toFixed(4)}, ${lon.toFixed(4)}`);
    map.flyTo({ center: [lon, lat], zoom, pitch, bearing, duration });
  },
  setBasemap: (mode) => setBasemap(mode),
  setLayer: (layer) => setLayer(layer),
  setPlaying: (playing) => {
    state.playing = Boolean(playing);
    $('btn-play').textContent = state.playing ? '❚❚' : '▶';
  },
  setFrame: (idx) => {
    if (!state.frames.length) return;
    state.playing = false;
    $('btn-play').textContent = '▶';
    state.frameIdx = Math.max(0, Math.min(state.frames.length - 1, idx));
    $('scrubber').value = String(state.frameIdx);
    updateTimeReadout();
    if (state.xsecPts.length === 2) renderCrossSectionInspector();
    else if (state.selectedCell) renderHydrographInspector();
    renderFloodCanvas(true);
  },
  setScenario: (opts = {}) => {
    if (typeof opts.rainfall_mm_per_hr === 'number') {
      $('ctl-rain').value = String(opts.rainfall_mm_per_hr);
      $('val-rain').textContent = `${opts.rainfall_mm_per_hr} mm/hr`;
    }
    if (typeof opts.duration_s === 'number') {
      $('ctl-dur').value = String(opts.duration_s);
      $('val-dur').textContent = `${opts.duration_s} s`;
    }
    if (typeof opts.wind_x_m_per_s === 'number') {
      $('ctl-wind').value = String(opts.wind_x_m_per_s);
      $('val-wind').textContent = `${opts.wind_x_m_per_s} m/s`;
    }
    if (typeof opts.manning_n === 'number') {
      $('ctl-manning').value = String(opts.manning_n);
      $('val-manning').textContent = Number(opts.manning_n).toFixed(3);
    }
    if (opts.hyetograph) $('ctl-hyeto').value = opts.hyetograph;
    if (opts.soil) $('ctl-soil').value = opts.soil;
    if (opts.boundary) $('ctl-boundary').value = opts.boundary;
    if (opts.start_iso) {
      const parsed = Date.parse(opts.start_iso.endsWith('Z') ? opts.start_iso : `${opts.start_iso}Z`);
      if (Number.isFinite(parsed)) state.startEpochMs = parsed;
    }
    if (opts.tz_label) state.tzLabel = String(opts.tz_label);
    if (typeof opts.span_hours === 'number' && Number.isFinite(opts.span_hours)) {
      state.spanHours = Math.max(0, opts.span_hours);
      renderForecastSegments();
    }
    if (typeof opts.antecedent_rain_mm === 'number' && Number.isFinite(opts.antecedent_rain_mm)) {
      state.antecedentMm = Math.max(0, Math.min(500, opts.antecedent_rain_mm));
    }
    if (Array.isArray(opts.hourly_rain_mm)) {
      state.hourlyRainMm = opts.hourly_rain_mm.map(Number).filter(Number.isFinite);
    }
    if (opts.spatial_rain_grid && typeof opts.spatial_rain_grid === 'object') {
      state.spatialRainGrid = opts.spatial_rain_grid;
    }
    if (Array.isArray(opts.hourly_tide_m)) {
      state.hourlyTideM = opts.hourly_tide_m.map(Number).filter(Number.isFinite);
    }
    if (typeof opts.drainage_mm_per_hr === 'number' && Number.isFinite(opts.drainage_mm_per_hr)) {
      state.drainageMmHr = Math.max(0, opts.drainage_mm_per_hr);
    }
    if (typeof opts.evaporation_mm_per_day === 'number' && Number.isFinite(opts.evaporation_mm_per_day)) {
      state.evapMmDay = Math.max(0, opts.evaporation_mm_per_day);
    }
    if (Array.isArray(opts.observed_gauges)) {
      state.observedGauges = opts.observed_gauges;
    }
    updateTimeReadout();
  },
  runSimulation: () => runSimulation(false),
  compare: () => runSimulation(true),
  fetchNwp: () => fetchLiveMultiModelNwp(),
  crossSectionLatLon: (aLat, aLon, bLat, bLon) => {
    const ca = lngLatToCell(aLon, aLat);
    const cb = lngLatToCell(bLon, bLat);
    if (!ca || !cb) return false;
    state.xsecPts = [{ col: ca.col, row: ca.row }, { col: cb.col, row: cb.row }];
    renderCrossSectionInspector();
    renderFloodCanvas(false);
    return true;
  },
  clearInspector: () => {
    $('inspector').classList.add('hidden');
    state.xsecPts = [];
    state.selectedCell = null;
    renderFloodCanvas(false);
  },
  addInterventionLatLon: (kind, points) => {
    const delta = kind === 'pond' ? -0.8 : 0.8;
    const reason = kind === 'pond' ? 'detention pond' : 'raised flood bund';
    for (const [lat, lon] of points) {
      const hit = lngLatToCell(lon, lat);
      if (!hit) continue;
      for (let dr = 0; dr <= 1; dr++) {
        for (let dc = 0; dc <= 1; dc++) {
          const c = hit.col + dc;
          const r = hit.row + dr;
          if (c < state.grid.metadata.cols && r < state.grid.metadata.rows) {
            state.interventions.push({ col: c, row: r, delta_m: delta, reason });
          }
        }
      }
    }
    updateInterventionBar();
    renderFloodCanvas(false);
  },
  clearInterventions: () => {
    state.interventions = [];
    updateInterventionBar();
    renderFloodCanvas(false);
  },
  setCaption: (text) => {
    const el = $('director-caption');
    if (!text) {
      el.classList.add('hidden');
      el.textContent = '';
    } else {
      el.textContent = text;
      el.classList.remove('hidden');
    }
  },
  setPresentationMode: (enabled) => {
    document.documentElement.dataset.presentation = enabled ? 'true' : 'false';
    if (map && map.getLayer('arus-domain-outline')) {
      map.setLayoutProperty(
        'arus-domain-outline',
        'visibility',
        enabled || state.boundaryGeoJson ? 'none' : 'visible',
      );
    }
  },
  loadBoundaryByQuery: (query) => loadBoundaryByQuery(query),
  setPublicOutlook: (cfg) => {
    const card = $('public-outlook');
    if (!cfg) {
      card.classList.add('hidden');
      return;
    }
    $('po-title').textContent = cfg.title || 'Flood Outlook';
    const risk = $('po-risk');
    const lvl = (cfg.risk_level || 'moderate').toLowerCase();
    risk.className = `po-risk ${lvl}`;
    risk.textContent = cfg.risk_text || lvl.toUpperCase();
    $('po-summary').textContent = cfg.summary || '';
    const spots = Array.isArray(cfg.hotspots) ? cfg.hotspots : [];
    state.outlookHotspots = spots;
    $('po-hotspots').innerHTML = spots.map((s) => {
      const cls = s.tone ? ` ${s.tone}` : '';
      return `<div class="po-spot"><span class="po-spot-name">${s.name}</span><span class="po-spot-val${cls}">${s.status}</span></div>`;
    }).join('');
    if (map && map.getSource('arus-hotspots')) {
      const features = spots
        .filter((s) => Number.isFinite(s.lat) && Number.isFinite(s.lon))
        .map((s) => ({
          type: 'Feature',
          geometry: { type: 'Point', coordinates: [s.lon, s.lat] },
          properties: {
            name: s.name || '',
            color: s.tone === 'danger' ? '#e11d48' : s.tone === 'warn' ? '#d97706' : '#0284c7',
          },
        }));
      map.getSource('arus-hotspots').setData({ type: 'FeatureCollection', features });
    }
    if (cfg.footnote) $('po-foot').textContent = cfg.footnote;
    card.classList.remove('hidden');
    if (state.frames.length > 0) updateLiveHotspots(state.frameIdx);
  },
};

function applyStartupParams() {
  const qs = new URLSearchParams(window.location.search);
  const lat = parseFloat(qs.get('lat'));
  const lon = parseFloat(qs.get('lon'));
  if (Number.isFinite(lat) && Number.isFinite(lon)) {
    state.center = { lat, lon };
    setCustomPresetLabel(`Custom · ${lat.toFixed(4)}, ${lon.toFixed(4)}`);
  }
  const zoom = parseFloat(qs.get('zoom'));
  if (Number.isFinite(zoom)) state.initZoom = zoom;
  const pitch = parseFloat(qs.get('pitch'));
  if (Number.isFinite(pitch)) state.initPitch = pitch;
  const bearing = parseFloat(qs.get('bearing'));
  if (Number.isFinite(bearing)) state.initBearing = bearing;
  const basemap = qs.get('basemap');
  if (basemap === 'topo' || basemap === 'sat' || basemap === 'dark') {
    state.basemap = basemap;
    document.documentElement.dataset.theme = basemap === 'topo' ? 'light' : 'dark';
    document.querySelectorAll('[data-basemap]').forEach((el) => {
      el.classList.toggle('active', el.dataset.basemap === basemap);
    });
  }
  if (qs.get('presentation') === '1' || qs.get('presentation') === 'true') {
    document.documentElement.dataset.presentation = 'true';
  }
  if (qs.get('fixed') === '1' || qs.get('fixed') === 'true') {
    state.fixedView = true;
  }
  if (qs.get('autoplay') === '0' || qs.get('autoplay') === 'false') {
    state.playing = false;
    $('btn-play').textContent = '▶';
  }
  const scen = {};
  const rain = parseFloat(qs.get('rain'));
  if (Number.isFinite(rain)) scen.rainfall_mm_per_hr = rain;
  const dur = parseFloat(qs.get('dur'));
  if (Number.isFinite(dur)) scen.duration_s = dur;
  const wind = parseFloat(qs.get('wind'));
  if (Number.isFinite(wind)) scen.wind_x_m_per_s = wind;
  if (qs.get('soil')) scen.soil = qs.get('soil');
  if (qs.get('hyetograph')) scen.hyetograph = qs.get('hyetograph');
  if (qs.get('boundary')) scen.boundary = qs.get('boundary');
  if (qs.get('start_iso')) scen.start_iso = qs.get('start_iso');
  if (qs.get('tz')) scen.tz_label = qs.get('tz');
  const spanHours = parseFloat(qs.get('span_hours'));
  if (Number.isFinite(spanHours) && spanHours > 0) scen.span_hours = spanHours;
  const antMm = parseFloat(qs.get('antecedent_mm'));
  if (Number.isFinite(antMm) && antMm > 0) scen.antecedent_rain_mm = antMm;
  if (Object.keys(scen).length > 0) window.arusDirector.setScenario(scen);
}

window.addEventListener('DOMContentLoaded', () => {
  wireUi();
  applyStartupParams();
  initMap();
  setLayer('depth');
  requestAnimationFrame(animationLoop);
});
