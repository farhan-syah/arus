// ARUS 2026 Flood Simulation Studio Client
// Driven by real hydr 0.1.0 binary Float32 frames (/api/v1/runs/{id}/grid & frames.bin).

const PRESETS = [
  { id: 'shah-alam', name: 'Shah Alam · Taman Sri Muda (MY)', lon: 101.535, lat: 3.031, zoom: 14.3 },
  { id: 'ijok', name: 'Ijok · Kuala Selangor (MY)', lon: 101.415, lat: 3.310, zoom: 13.0 },
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
  profile: 'researcher', // public | researcher
  serverConfig: null,
  serverContext: null,
  directorOutlookSet: false,
  pickingStressSite: false,
  stressSite: null,
  pinnedProbes: [],
  activeAbort: null,
  lastCompareMode: false,
  verdictTarget: null,
  activeEngine: 'Server · F64',
  simWorker: null,
  simWorkerSeq: 0,
  wasmAvailable: true,
  staticVectorsDirty: true,
  frameRasterCache: new Map(),
  displayPrefs: {
    showFlood: true,
    showRivers: true,
    showBoundary: true,
    showMarkers: true,
    showParticles: true,
    is3D: true,
    showLabels: true,
    basemap: 'topo',
    layer: 'depth',
    depthCutoffM: 0.06,
    floodOpacity: 0.95,
    riverOpacity: 0.72,
    units: 'metric', // metric | imperial
    clock: '12h',    // 12h | 24h
    showMetrics: true,
    showLegend: true,
  },
  simConfig: {
    mode: 'forecast', // forecast | stress | physics
    horizon: '5d',    // 5d | 15d | mjo
    rainMult: 1.0,
    stressMmDay: 150,
    stressDays: 3,
    stressPadM: 0,
    rainfallMmHr: 280,
    durationS: 360,
    hyetograph: 'peak-storm',
    soil: 'impervious',
    boundary: 'transmissive',
    manningN: 0.035,
    windX: 0,
    startDry: false,
    enableEvap: true,
    enableDrain: true,
    tideMode: 'live', // live | spring-high | off
    compute: 'auto', // auto | server
  },
};

function fmtDepth(m, decimals = 2) {
  if (!Number.isFinite(m)) return '—';
  if (state.displayPrefs.units === 'imperial') {
    return `${(m * 3.28084).toFixed(decimals)} ft`;
  }
  return `${m.toFixed(decimals)} m`;
}

function fmtArea(km2, decimals = 1) {
  if (!Number.isFinite(km2)) return '—';
  if (state.displayPrefs.units === 'imperial') {
    return `${(km2 * 0.386102).toFixed(decimals)} mi²`;
  }
  return `${km2.toFixed(decimals)} km²`;
}

function fmtSpeed(ms, decimals = 2) {
  if (!Number.isFinite(ms)) return '—';
  if (state.displayPrefs.units === 'imperial') {
    return `${(ms * 3.28084).toFixed(decimals)} ft/s`;
  }
  return `${ms.toFixed(decimals)} m/s`;
}

function fmtRain(mmHr, decimals = 1) {
  if (!Number.isFinite(mmHr)) return '—';
  if (state.displayPrefs.units === 'imperial') {
    return `${(mmHr / 25.4).toFixed(decimals)} in/hr`;
  }
  return `${mmHr.toFixed(decimals)} mm/hr`;
}

function fmtClockTime(epochMs, tzLabel) {
  const d = new Date(epochMs);
  const days = ['Sun', 'Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat'];
  const day = days[d.getUTCDay()];
  const dd = d.getUTCDate();
  const h24 = d.getUTCHours();
  const mm = String(d.getUTCMinutes()).padStart(2, '0');
  if (state.displayPrefs.clock === '24h') {
    return `${day} ${dd} · ${String(h24).padStart(2, '0')}:${mm} ${tzLabel}`;
  }
  const ap = h24 >= 12 ? 'PM' : 'AM';
  const h12 = h24 % 12 === 0 ? 12 : h24 % 12;
  return `${day} ${dd} · ${h12}:${mm} ${ap} ${tzLabel}`;
}

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
  if (!map.getSource('arus-terrarium-dem')) {
    map.addSource('arus-terrarium-dem', {
      type: 'raster-dem',
      tiles: ['https://s3.amazonaws.com/elevation-tiles-prod/terrarium/{z}/{x}/{y}.png'],
      encoding: 'terrarium',
      tileSize: 256,
      maxzoom: 15,
    });
    map.addLayer(
      {
        id: 'arus-hillshade',
        type: 'hillshade',
        source: 'arus-terrarium-dem',
        paint: {
          'hillshade-exaggeration': isDark ? 0.25 : 0.32,
          'hillshade-shadow-color': isDark ? '#020617' : '#334155',
          'hillshade-highlight-color': isDark ? '#334155' : '#ffffff',
        },
      },
      beforeLabels,
    );
    if (state.is3D && !isPres && !state.boundaryGeoJson) {
      try { map.setTerrain({ source: 'arus-terrarium-dem', exaggeration: 1.35 }); } catch (_) {}
    } else {
      try { map.setTerrain(null); } catch (_) {}
    }
  }
  const showBox = !isPres && !state.boundaryGeoJson && ['pick', 'bund', 'pond'].includes(state.tool);
  map.addSource('arus-domain-box', { type: 'geojson', data: empty });
  map.addLayer(
    {
      id: 'arus-domain-outline',
      type: 'line',
      source: 'arus-domain-box',
      layout: { visibility: showBox ? 'visible' : 'none' },
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
          'line-opacity': state.displayPrefs.riverOpacity ?? 0.72,
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

function registerTideMarkerIcons(targetMap) {
  const states = [
    ['normal', '#0f766e'],
    ['high', '#d97706'],
  ];
  const trends = ['steady', 'up', 'down'];
  for (const [level, bg] of states) {
    for (const trend of trends) {
      const id = `arus-tide-${trend}-${level}`;
      if (targetMap.hasImage(id)) continue;
      const size = 48;
      const c = document.createElement('canvas');
      c.width = size;
      c.height = size;
      const ctx = c.getContext('2d');

      // Rounded ocean badge
      const rx = 6;
      const ry = 10;
      const rw = 34;
      const rh = 28;
      const rad = 8;
      ctx.beginPath();
      ctx.moveTo(rx + rad, ry);
      ctx.lineTo(rx + rw - rad, ry);
      ctx.quadraticCurveTo(rx + rw, ry, rx + rw, ry + rad);
      ctx.lineTo(rx + rw, ry + rh - rad);
      ctx.quadraticCurveTo(rx + rw, ry + rh, rx + rw - rad, ry + rh);
      ctx.lineTo(rx + rad, ry + rh);
      ctx.quadraticCurveTo(rx, ry + rh, rx, ry + rh - rad);
      ctx.lineTo(rx, ry + rad);
      ctx.quadraticCurveTo(rx, ry, rx + rad, ry);
      ctx.closePath();
      ctx.fillStyle = bg;
      ctx.fill();
      ctx.lineWidth = 3;
      ctx.strokeStyle = '#ffffff';
      ctx.stroke();

      // Two crisp white wave lines inside badge
      ctx.strokeStyle = '#ffffff';
      ctx.lineWidth = 2.6;
      ctx.lineCap = 'round';
      for (const wy of [21, 29]) {
        ctx.beginPath();
        ctx.moveTo(13, wy);
        ctx.bezierCurveTo(17, wy - 4, 21, wy + 4, 25, wy);
        ctx.bezierCurveTo(28, wy - 3, 31, wy + 2, 33, wy);
        ctx.stroke();
      }

      // Superscript trend pill (top-right corner: chevron up, chevron down, or dash)
      ctx.beginPath();
      ctx.arc(38, 12, 8.5, 0, Math.PI * 2);
      ctx.fillStyle = '#0f172a';
      ctx.fill();
      ctx.lineWidth = 2;
      ctx.strokeStyle = '#ffffff';
      ctx.stroke();

      ctx.strokeStyle = '#ffffff';
      ctx.lineWidth = 2.4;
      ctx.lineCap = 'round';
      ctx.lineJoin = 'round';
      ctx.beginPath();
      if (trend === 'up') {
        ctx.moveTo(34.5, 13.8);
        ctx.lineTo(38, 9.8);
        ctx.lineTo(41.5, 13.8);
      } else if (trend === 'down') {
        ctx.moveTo(34.5, 10.2);
        ctx.lineTo(38, 14.2);
        ctx.lineTo(41.5, 10.2);
      } else {
        ctx.moveTo(34.5, 12);
        ctx.lineTo(41.5, 12);
      }
      ctx.stroke();

      const img = ctx.getImageData(0, 0, size, size);
      targetMap.addImage(id, img, { pixelRatio: 2 });
    }
  }
}

  map.addSource('arus-hotspots', { type: 'geojson', data: empty });
  registerTideMarkerIcons(map);
  map.addLayer({
    id: 'arus-hotspots-circle',
    type: 'circle',
    source: 'arus-hotspots',
    filter: ['!=', ['get', 'kind'], 'tide'],
    paint: {
      'circle-radius': 6,
      'circle-color': ['get', 'color'],
      'circle-stroke-color': '#ffffff',
      'circle-stroke-width': 2,
    },
  });
  try {
    map.addLayer({
      id: 'arus-hotspots-label',
      type: 'symbol',
      source: 'arus-hotspots',
      filter: ['!=', ['get', 'kind'], 'tide'],
      layout: {
        'text-field': ['get', 'name'],
        'text-size': 11,
        'text-font': ['Noto Sans Bold'],
        'text-offset': [0, -1.15],
        'text-anchor': 'bottom',
        'text-allow-overlap': true,
      },
      paint: {
        'text-color': '#0f172a',
        'text-halo-color': 'rgba(255,255,255,0.95)',
        'text-halo-width': 2.0,
      },
    });
  } catch (_) {}
  map.addLayer({
    id: 'arus-hotspots-tide',
    type: 'symbol',
    source: 'arus-hotspots',
    filter: ['==', ['get', 'kind'], 'tide'],
    layout: {
      'icon-image': ['get', 'icon'],
      'icon-allow-overlap': true,
      'icon-ignore-placement': true,
    },
  });

  layersReady = true;
}

function updateDomainOutline(bounds) {
  if (!map || !layersReady) return;
  const isPres = document.documentElement.dataset.presentation === 'true';
  if (map.getLayer('arus-domain-outline')) {
    const showBox = !isPres && !state.boundaryGeoJson && ['pick', 'bund', 'pond'].includes(state.tool);
    map.setLayoutProperty(
      'arus-domain-outline',
      'visibility',
      showBox ? 'visible' : 'none',
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

function buildRadiusPolygon(lon, lat, radiusKm = 5.0, shape = 'hex') {
  const rMeters = Math.max(0.5, Math.min(50, Number(radiusKm) || 5.0)) * 1000.0;
  const mPerDegLat = 111320.0;
  const mPerDegLon = Math.max(10000.0, 111320.0 * Math.cos((lat * Math.PI) / 180));
  const sides = shape === 'circle' ? 48 : 6;
  const rotOffset = shape === 'circle' ? 0 : Math.PI / 6; // pointy-top hexagon
  const ring = [];
  for (let i = 0; i <= sides; i++) {
    const theta = rotOffset + (i / sides) * Math.PI * 2;
    const dx = Math.cos(theta) * rMeters;
    const dy = Math.sin(theta) * rMeters;
    ring.push([lon + dx / mPerDegLon, lat + dy / mPerDegLat]);
  }
  return { type: 'Polygon', coordinates: [ring] };
}

async function applyBoundaryGeometry(geom, runSim = true) {
  if (!geom || (geom.type !== 'Polygon' && geom.type !== 'MultiPolygon')) return false;
  state.boundaryGeoJson = geom;
  const [minLon, minLat, maxLon, maxLat] = computeGeomBbox(geom);
  const midLat = (minLat + maxLat) * 0.5;
  const midLon = (minLon + maxLon) * 0.5;
  const mPerDegLat = 111320.0;
  const mPerDegLon = Math.max(10000.0, 111320.0 * Math.cos((midLat * Math.PI) / 180));
  const spanXm = (maxLon - minLon) * mPerDegLon * 1.06;
  const spanYm = (maxLat - minLat) * mPerDegLat * 1.06;
  const aspect = Math.max(0.4, Math.min(2.5, spanXm / Math.max(1, spanYm)));
  const rows = Math.max(28, Math.min(56, Math.round(Math.sqrt(2000 / aspect))));
  const cols = Math.max(28, Math.min(56, Math.round(rows * aspect)));
  const dxM = Math.max(45.0, Math.min(2400.0, Math.max(spanXm / cols, spanYm / rows)));
  state.center = { lat: midLat, lon: midLon };
  state.boundaryGrid = { cols, rows, dxM };
  state.cachedDem = null;
  if (map) { try { map.setTerrain(null); } catch (_) {} }
  applyRegionBoundary();
  map.fitBounds([[minLon, minLat], [maxLon, maxLat]], {
    padding: { top: 64, bottom: 78, left: 310, right: 235 },
    pitch: 12,
    bearing: 0,
    duration: 0,
  });
  if (runSim) await runSimulation(false);
  return true;
}

async function setCustomBoundary(lon, lat, radiusKm = 5.0, shape = 'hex', runSim = false) {
  const geom = buildRadiusPolygon(lon, lat, radiusKm, shape);
  return applyBoundaryGeometry(geom, runSim);
}

async function loadBoundaryByQuery(query, opts = {}) {
  if (!query) return false;
  try {
    const resp = await fetch(
      `https://nominatim.openstreetmap.org/search?format=json&polygon_geojson=1&limit=1&q=${encodeURIComponent(query)}`,
    );
    const hits = await resp.json();
    const hit = hits?.[0];
    const geom = hit?.geojson;
    if (geom && (geom.type === 'Polygon' || geom.type === 'MultiPolygon')) {
      return applyBoundaryGeometry(geom, opts.runSim !== false);
    }
    if (hit && hit.lat && hit.lon) {
      const custom = buildRadiusPolygon(
        parseFloat(hit.lon),
        parseFloat(hit.lat),
        opts.radiusKm || 5.0,
        opts.shape || 'hex',
      );
      return applyBoundaryGeometry(custom, opts.runSim !== false);
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

// Hydrological blue depth ramp: dry/shallow (< depthCutoffM) is 100% transparent
function colorForDepth(h) {
  const cut = state.displayPrefs.depthCutoffM || 0.06;
  if (h < cut) return null;
  const fade = Math.min(1, (h - cut) / 0.04);
  const stops = [
    [cut,   56, 189, 248, 175], // #38bdf8 crisp sky (shallow flood)
    [Math.max(cut + 0.06, 0.14), 2, 132, 199, 212], // #0284c7 river blue
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

// Flood arrival time (onset_time_s): early arrival = urgent crimson/amber, late arrival = cool blue
function colorForArrival(onsetS, maxT) {
  if (!Number.isFinite(onsetS) || onsetS < 0) return null;
  const frac = Math.min(1, Math.max(0, onsetS / Math.max(1, maxT)));
  if (frac < 0.25) return [225, 29, 72, 215];
  if (frac < 0.50) return [249, 115, 22, 200];
  if (frac < 0.75) return [250, 204, 21, 185];
  return [56, 189, 248, 175];
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
      } else if (state.layer === 'arrival') {
        const onset = env.onset_time_s[rNearest * cols + cNearest];
        const maxT = state.frames.length ? (state.frames[state.frames.length - 1].time_s || 360) : 360;
        rgba = colorForArrival(onset, maxT);
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
  let url = state.frameRasterCache.get(lastRasterKey);
  if (!url) {
    url = rasterCanvas.toDataURL('image/png');
    if (state.frameRasterCache.size > 64) state.frameRasterCache.clear();
    state.frameRasterCache.set(lastRasterKey, url);
  }
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
        paint: { 'raster-opacity': state.displayPrefs.floodOpacity ?? 0.95, 'raster-resampling': 'linear', 'raster-fade-duration': 0 },
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

  const rasterKey = `${state.runId}:${state.frameIdx}:${state.layer}:${state.envelopeMode}:${state.displayPrefs.depthCutoffM}`;
  if (forceRaster || rasterKey !== lastRasterKey) {
    lastRasterKey = rasterKey;
    const cachedUrl = state.frameRasterCache.get(rasterKey);
    if (cachedUrl && map.getSource('arus-water-img')) {
      const coords = [[minLon, maxLat], [maxLon, maxLat], [maxLon, minLat], [minLon, minLat]];
      map.getSource('arus-water-img').updateImage({ url: cachedUrl, coordinates: coords });
    } else {
      updateSmoothWaterImage(meta, env, frame);
    }
  }

  const particleFeatures = [];
  if (state.particlesOn && state.displayPrefs.showParticles && state.displayPrefs.showFlood && state.layer !== 'diff') {
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

  const pSrc = map.getSource('arus-particles');
  if (pSrc && (state.playing || forceRaster || particleFeatures.length === 0)) {
    pSrc.setData({ type: 'FeatureCollection', features: particleFeatures });
  }
  if (forceRaster || state.staticVectorsDirty) {
    state.staticVectorsDirty = false;
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
    if (state.xsecPts.length === 1) {
      const a = state.xsecPts[0];
      const cx = minLon + (a.col + 0.5) * dLon;
      const cy = minLat + (a.row + 0.5) * dLat;
      xsecFeatures.push({
        type: 'Feature',
        geometry: { type: 'LineString', coordinates: [[cx - dLon * 0.4, cy], [cx + dLon * 0.4, cy]] },
        properties: {},
      });
    } else if (state.xsecPts.length === 2) {
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
    const ivSrc = map.getSource('arus-iv-polys');
    const xSrc = map.getSource('arus-xsec');
    if (ivSrc) ivSrc.setData({ type: 'FeatureCollection', features: ivFeatures });
    if (xSrc) xSrc.setData({ type: 'FeatureCollection', features: xsecFeatures });
  }
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
  el.textContent = `z ${fmtDepth(z, 1)} · h ${fmtDepth(h, 2)} · |v| ${fmtSpeed(sp, 2)}`;
}

function handleMapClick(lon, lat) {
  if (state.pickingStressSite) {
    state.pickingStressSite = false;
    document.body.classList.remove('picking-site');
    state.stressSite = { lat, lon };
    if ($('sim-stress-coord')) {
      $('sim-stress-coord').textContent = `${lat.toFixed(4)}°, ${lon.toFixed(4)}°`;
    }
    if ($('sim-modal')) $('sim-modal').classList.remove('hidden');
    return;
  }
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

  if (state.profile === 'public') {
    state.verdictTarget = { col: hit.col, row: hit.row, lat, lon };
    renderSiteVerdict();
    return;
  }

  if (state.tool === 'floodpin') {
    const depthM = parseFloat($('fp-depth')?.value || '0.5') || 0.5;
    state.observedGauges.push({ lat, lon, depth_m: depthM, radius_cells: 3 });
    updateFloodPinBar();
    showToast(`Added observed ${fmtDepth(depthM, 1)} flood pin (${state.observedGauges.length} total). Click Run Simulation to update top-level map.`);
    return;
  }

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
    state.staticVectorsDirty = true;
    updateInterventionBar();
    renderFloodCanvas(false);
    return;
  }

  if (state.tool === 'xsection') {
    if (state.xsecPts.length >= 2) state.xsecPts = [];
    state.xsecPts.push({ col: hit.col, row: hit.row });
    state.staticVectorsDirty = true;
    if (state.xsecPts.length === 2) renderCrossSectionInspector();
    renderFloodCanvas(false);
    return;
  }

  state.selectedCell = { col: hit.col, row: hit.row };
  renderHydrographInspector();
}

function updateFloodPinBar() {
  const bar = $('floodpin-bar');
  if (!bar) return;
  const n = (state.observedGauges || []).length;
  $('fp-count').textContent = String(n);
  bar.classList.toggle('hidden', n === 0 && state.tool !== 'floodpin');
}

function fmtSimOffset(sec, totalSec) {
  if (!Number.isFinite(sec) || sec < 0) return '—';
  if (state.spanHours > 0 && totalSec > 0) {
    const realHrs = (sec / totalSec) * state.spanHours;
    if (realHrs >= 24) {
      const d = Math.floor(realHrs / 24);
      const h = Math.round(realHrs % 24);
      return `+${d}d ${h}h`;
    }
    return `+${realHrs.toFixed(1)}h`;
  }
  return `+${fmtMMSS(sec)}`;
}

function computeRecessionSummary(idx, thresholdM) {
  if (!state.frames || state.frames.length < 2) return { label: '—', receded: false, endDepth: 0 };
  const totalSec = Math.max(1, state.frames[state.frames.length - 1].time_s || 1);
  let peakIdx = -1;
  let peakVal = 0;
  for (let i = 0; i < state.frames.length; i++) {
    const d = state.frames[i].depth[idx] || 0;
    if (d > peakVal) {
      peakVal = d;
      peakIdx = i;
    }
  }
  const endDepth = state.frames[state.frames.length - 1].depth[idx] || 0;
  if (peakVal < thresholdM) return { label: 'Dry', receded: true, endDepth };
  for (let i = Math.max(0, peakIdx); i < state.frames.length; i++) {
    if ((state.frames[i].depth[idx] || 0) < thresholdM) {
      return {
        label: `Recedes ${fmtSimOffset(state.frames[i].time_s, totalSec)}`,
        receded: true,
        endDepth,
      };
    }
  }
  return {
    label: `Still flooded (${fmtDepth(endDepth, 2)})`,
    receded: false,
    endDepth,
  };
}

function renderSiteVerdict() {
  const card = $('site-verdict');
  if (!card || !state.verdictTarget || !state.grid || !state.frames.length) return;
  const { col, row, lat, lon } = state.verdictTarget;
  const meta = state.grid.metadata;
  const env = state.grid.envelope;
  const idx = row * meta.cols + col;
  card.classList.remove('hidden');

  if (!meta.active_mask[idx]) {
    $('sv-title').textContent = `${lat.toFixed(4)}°, ${lon.toFixed(4)}°`;
    $('sv-badge').className = 'lm-badge low';
    $('sv-badge').textContent = 'NO DATA';
    $('sv-summary').textContent = 'This coordinate lies outside the active terrain coverage mask.';
    $('sv-stats').innerHTML = '';
    return;
  }

  const ffeM = parseFloat($('sv-ffe-select')?.value || '0') || 0;
  const peakM = env.max_depth_m[idx] || 0;
  const netM = Math.max(0, peakM - ffeM);
  const bedM = meta.bed_m[idx] || 0;
  const onsetS = env.onset_time_s[idx];
  const cut = state.displayPrefs.depthCutoffM || 0.06;
  const totalSec = Math.max(1, state.frames[state.frames.length - 1].time_s || 1);
  const recInfo = computeRecessionSummary(idx, env.threshold_m || 0.15);

  let lvl = 'low';
  let label = 'DRY';
  let msg = `Water stays below your ${fmtDepth(ffeM, 1)} floor level in this scenario.`;
  if (netM >= 0.30) {
    lvl = 'danger';
    label = 'FLOOD EXPECTED';
    msg = `Peak reaches ${fmtDepth(netM, 2)} above floor (${fmtDepth(peakM, 2)} ground). ${recInfo.receded ? recInfo.label : 'Water does not recede within forecast window'}.`;
  } else if (netM >= cut) {
    lvl = 'moderate';
    label = 'WATCH';
    msg = `Shallow water up to ${fmtDepth(netM, 2)} above floor (${fmtDepth(peakM, 2)} ground). ${recInfo.receded ? recInfo.label : 'Stagnant ponding persists through forecast end'}.`;
  }

  $('sv-title').textContent = `${lat.toFixed(4)}°, ${lon.toFixed(4)}°`;
  $('sv-badge').className = `lm-badge ${lvl}`;
  $('sv-badge').textContent = label;
  $('sv-summary').textContent = msg;
  const onsetTxt = onsetS >= 0 ? fmtSimOffset(onsetS, totalSec) : 'Never';
  $('sv-stats').innerHTML = `
    <div class="mini-stat"><div class="mini-k">Peak Depth</div><div class="mini-v">${fmtDepth(peakM, 2)}</div></div>
    <div class="mini-stat"><div class="mini-k">Above Floor</div><div class="mini-v">${fmtDepth(netM, 2)}</div></div>
    <div class="mini-stat"><div class="mini-k">Arrival</div><div class="mini-v">${onsetTxt}</div></div>
    <div class="mini-stat"><div class="mini-k">Recession</div><div class="mini-v">${recInfo.label}</div></div>
  `;
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
  const pinColors = ['#0d9488', '#d97706', '#7c3aed'];
  const pinLines = state.pinnedProbes
    .filter((p) => !(p.col === col && p.row === row))
    .map((p, k) => {
      const pidx = p.row * meta.cols + p.col;
      if (pidx < 0 || pidx >= meta.cols * meta.rows) return '';
      const ppts = times.map((t, i) => {
        const x = 38 + (t / tMax) * 288;
        const y = 104 - (Math.min(yMax, state.frames[i].depth[pidx]) / yMax) * 82;
        return `${x.toFixed(1)},${y.toFixed(1)}`;
      }).join(' ');
      return `<polyline points="${ppts}" fill="none" stroke="${pinColors[k % pinColors.length]}" stroke-width="1.5" stroke-dasharray="3 2"/>`;
    }).join('');
  $('inspector-svg').innerHTML = `
    <line x1="38" y1="22" x2="326" y2="22" stroke="rgba(100,116,139,0.22)"/>
    <line x1="38" y1="104" x2="326" y2="104" stroke="rgba(100,116,139,0.45)"/>
    <polygon points="${pts} 326,104 38,104" fill="rgba(2,132,199,0.18)"/>
    ${pinLines}
    <polyline points="${pts}" fill="none" stroke="#0284c7" stroke-width="2"/>
    <line x1="38" y1="${ty.toFixed(1)}" x2="326" y2="${ty.toFixed(1)}" stroke="#e11d48" stroke-dasharray="4 3" stroke-width="1.2"/>
    <line x1="${cx.toFixed(1)}" y1="18" x2="${cx.toFixed(1)}" y2="104" stroke="#d97706" stroke-width="1.2"/>
    <text x="34" y="25" text-anchor="end" fill="#64748b" font-family="JetBrains Mono, monospace" font-size="10">${yMax.toFixed(2)}</text>
    <text x="34" y="107" text-anchor="end" fill="#64748b" font-family="JetBrains Mono, monospace" font-size="10">0.00</text>
    <text x="324" y="${(ty - 4).toFixed(1)}" text-anchor="end" fill="#e11d48" font-family="Inter, sans-serif" font-size="10" font-weight="500">Threshold ${fmtDepth(env.threshold_m, 2)}</text>
  `;
  const totalSec = Math.max(1, times[times.length - 1] || 1);
  const onset = env.onset_time_s[idx] >= 0 ? fmtSimOffset(env.onset_time_s[idx], totalSec) : '—';
  const rec = computeRecessionSummary(idx, env.threshold_m || 0.15).label;
  $('inspector-stats').innerHTML = `
    <div class="mini-stat"><div class="mini-k">Peak Depth</div><div class="mini-v">${fmtDepth(dPeak, 2)}</div></div>
    <div class="mini-stat"><div class="mini-k">Max Speed</div><div class="mini-v">${fmtSpeed(env.max_speed_m_s[idx], 2)}</div></div>
    <div class="mini-stat"><div class="mini-k">Onset</div><div class="mini-v">${onset}</div></div>
    <div class="mini-stat"><div class="mini-k">Recession</div><div class="mini-v">${rec}</div></div>
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

function loadTerrariumTileOnce(z, tx, ty) {
  return new Promise((resolve) => {
    const img = new Image();
    img.crossOrigin = 'anonymous';
    const timer = setTimeout(() => resolve(null), 8000);
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

async function loadTerrariumTile(z, tx, ty) {
  const first = await loadTerrariumTileOnce(z, tx, ty);
  if (first) return first;
  return loadTerrariumTileOnce(z, tx, ty);
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

  const zoom = dxM >= 600 ? 10 : dxM >= 120 ? 12 : dxM >= 45 ? 13 : 14;
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
  const sc = state.simConfig;
  const landChoice = sc.soil === 'green-ampt'
    ? { kind: 'modified-green-ampt', suction_m: 0.11, conductivity_m_per_s: 0.000015, maximum_deficit: 0.35 }
    : { kind: 'impervious' };
  const boundary = sc.boundary === 'closed'
    ? { kind: 'closed' }
    : { kind: 'transmissive' };
  const cols = state.boundaryGrid ? state.boundaryGrid.cols : 64;
  const rows = state.boundaryGrid ? state.boundaryGrid.rows : 56;
  const dxM = state.boundaryGrid ? state.boundaryGrid.dxM : 30.0;
  const sampled = state.uploadedDem
    ? null
    : await fetchTerrariumElevations(state.center.lon, state.center.lat, cols, rows, dxM);

  const mult = Number.isFinite(sc.rainMult) && sc.rainMult > 0 ? sc.rainMult : 1.0;
  let hourlyRain = (state.hourlyRainMm || []).map((v) => v * mult);
  let spatialGrid = state.spatialRainGrid ? {
    ...state.spatialRainGrid,
    hourly_mm: (state.spatialRainGrid.hourly_mm || []).map((row) => row.map((v) => v * mult)),
  } : null;
  let durationS = sc.durationS;

  if (sc.mode === 'stress') {
    const days = Math.max(1, Math.min(5, Math.round(sc.stressDays || 3)));
    const nHours = days * 24;
    const mmHr = Math.max(0.5, (sc.stressMmDay || 150) / 24.0);
    hourlyRain = Array.from({ length: nHours }, (_, h) => {
      const phase = Math.sin((h / Math.max(1, nHours - 1)) * Math.PI);
      return Number((mmHr * (0.4 + 1.2 * phase)).toFixed(2));
    });
    spatialGrid = null;
    durationS = Math.min(3600, Math.max(360, Math.round((nHours * 3600) / 200)));
  }

  const evapDay = sc.enableEvap ? (state.evapMmDay > 0 ? state.evapMmDay : 4.5) : 0;
  const drainHr = sc.enableDrain ? (state.drainageMmHr > 0 ? state.drainageMmHr : 3.0) : 0;

  return {
    center_lon: state.center.lon,
    center_lat: state.center.lat,
    cols,
    rows,
    dx_m: dxM,
    rainfall_mm_per_hr: Math.min(1000, Math.max(0, sc.rainfallMmHr * mult)),
    hyetograph: sc.hyetograph,
    duration_s: durationS,
    manning_n: sc.manningN,
    land: landChoice,
    boundary,
    threshold_m: 0.12,
    precision: 'f64',
    interventions: state.interventions,
    ascii_dem: state.uploadedDem,
    terrain_elevations_m: sampled,
    terrain_provenance: sampled ? 'BrowserTerrariumScreening' : null,
    antecedent_rain_mm: sc.startDry ? 0 : (state.antecedentMm || 0),
    hourly_rain_mm: hourlyRain,
    spatial_rain_grid: spatialGrid,
    hourly_tide_m: sc.tideMode === 'off' ? [] : (state.hourlyTideM || []),
    drainage_mm_per_hr: drainHr,
    evaporation_mm_per_day: evapDay,
    observed_gauges: sc.startDry ? [] : (state.observedGauges || []),
    wind_x_m_per_s: sc.windX || 0,
    wind_y_m_per_s: 0,
    start_dry: Boolean(sc.startDry),
    enable_evaporation: Boolean(sc.enableEvap),
    enable_drainage: Boolean(sc.enableDrain),
    tide_mode: sc.tideMode || 'live',
  };
}

function applyLoadedSimulation(grid, frames, runId, engineLabel) {
  state.grid = grid;
  state.frames = frames;
  state.runId = runId;
  state.activeEngine = engineLabel || 'Server · F64';
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

async function loadRun(runId) {
  const [gridResp, binResp] = await Promise.all([
    fetch(`/api/v1/runs/${runId}/grid`),
    fetch(`/api/v1/runs/${runId}/frames.bin`),
  ]);
  if (!gridResp.ok || !binResp.ok) throw new Error('Failed to load simulation frames');
  const grid = await gridResp.json();
  const frames = decodeFramesBin(await binResp.arrayBuffer());
  applyLoadedSimulation(grid, frames, runId, 'Server · F64');
}

function runBrowserSimulation(payload, abortSignal) {
  return new Promise((resolve, reject) => {
    if (abortSignal && abortSignal.aborted) {
      const err = new Error('Aborted');
      err.name = 'AbortError';
      reject(err);
      return;
    }
    if (!state.simWorker) {
      state.simWorker = new Worker('/assets/sim-worker.js', { type: 'module' });
    }
    const worker = state.simWorker;
    const id = ++state.simWorkerSeq;
    const cleanup = () => {
      worker.removeEventListener('message', onMsg);
      worker.removeEventListener('error', onErr);
      if (abortSignal) abortSignal.removeEventListener('abort', onAbort);
    };
    const onAbort = () => {
      cleanup();
      try { worker.terminate(); } catch (_) {}
      state.simWorker = null;
      const err = new Error('Aborted');
      err.name = 'AbortError';
      reject(err);
    };
    const onErr = (e) => {
      cleanup();
      try { worker.terminate(); } catch (_) {}
      state.simWorker = null;
      reject(new Error((e && e.message) || 'Browser WASM worker error'));
    };
    const onMsg = (e) => {
      const msg = e.data;
      if (!msg || msg.id !== id) return;
      cleanup();
      if (!msg.ok) {
        reject(new Error(msg.error || 'Browser WASM solver error'));
        return;
      }
      try {
        const grid = JSON.parse(msg.gridJson);
        const frames = decodeFramesBin(msg.framesBuffer);
        resolve({ engine: msg.engine || 'WASM · F32', grid, frames });
      } catch (parseErr) {
        reject(parseErr);
      }
    };
    worker.addEventListener('message', onMsg);
    worker.addEventListener('error', onErr);
    if (abortSignal) abortSignal.addEventListener('abort', onAbort, { once: true });
    worker.postMessage({
      id,
      payloadJson: JSON.stringify(payload),
      preferWebGpu: true,
    });
  });
}

function showToast(message, canRetry = false) {
  const bar = $('toast-bar');
  if (!bar) return;
  $('toast-text').textContent = message;
  $('btn-toast-retry').classList.toggle('hidden', !canRetry);
  bar.classList.remove('hidden');
  clearTimeout(bar._timer);
  if (!canRetry) {
    bar._timer = setTimeout(() => bar.classList.add('hidden'), 4200);
  }
}

function setSimProgress(text) {
  const pill = $('sim-progress');
  if (!pill) return;
  if (!text) {
    pill.classList.add('hidden');
  } else {
    $('sim-progress-text').textContent = text;
    pill.classList.remove('hidden');
  }
}

async function runSimulation(compareMode) {
  if (state.activeAbort) {
    try { state.activeAbort.abort(); } catch (_) {}
  }
  const abort = new AbortController();
  state.activeAbort = abort;
  state.lastCompareMode = Boolean(compareMode);

  const btn = compareMode ? $('btn-compare') : $('btn-run');
  const orig = btn ? btn.innerHTML : '';
  if (btn) {
    btn.disabled = true;
    btn.innerHTML = '<span class="spinner"></span>Solving…';
  }
  setSimProgress('Sampling terrain DEM…');
  try {
    const payload = await buildRequestPayload();
    if (abort.signal.aborted) return;
    setSimProgress(compareMode ? 'Solving Baseline vs Intervention…' : 'Solving 2D shallow water…');
    if (compareMode) {
      const resp = await fetch('/api/v1/compare', {
        method: 'POST',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify(payload),
        signal: abort.signal,
      });
      if (!resp.ok) throw new Error(await parseErrorText(resp));
      state.diffData = await resp.json();
      state.xsecPts = [];
      state.selectedCell = null;
      state.frameRasterCache.clear();
      setSimProgress('Loading frames…');
      await loadRun(state.diffData.intervention_run_id);
      setLayer('diff');
      $('inspector').classList.remove('hidden');
      $('inspector-svg').classList.add('hidden');
      $('inspector-title').textContent = 'Intervention Δh Impact';
      $('inspector-stats').innerHTML = `
        <div class="mini-stat"><div class="mini-k">Protected</div><div class="mini-v good">${state.diffData.improved_cells} cells (−${fmtDepth(state.diffData.max_reduction_m, 2)})</div></div>
        <div class="mini-stat"><div class="mini-k">Worsened</div><div class="mini-v bad">${state.diffData.worsened_cells} cells (+${fmtDepth(state.diffData.max_worsening_m, 2)})</div></div>
        <div class="mini-stat"><div class="mini-k">Earthwork</div><div class="mini-v">${state.diffData.earthwork.fill_volume_m3.toFixed(0)} m³ fill</div></div>
        <div class="mini-stat"><div class="mini-k">Mass Check</div><div class="mini-v good">${state.diffData.conservation_valid ? 'PASS' : 'FAIL'}</div></div>
      `;
    } else {
      state.diffData = null;
      const canUseBrowser =
        !state.fixedView &&
        !payload.ascii_dem &&
        state.simConfig.compute === 'auto' &&
        state.wasmAvailable &&
        typeof Worker !== 'undefined';

      let browserRes = null;
      if (canUseBrowser) {
        try {
          setSimProgress('Solving in browser (WebGPU / WASM)…');
          browserRes = await runBrowserSimulation(payload, abort.signal);
        } catch (browserErr) {
          if (browserErr && browserErr.name === 'AbortError') throw browserErr;
          state.wasmAvailable = false;
          setSimProgress('Falling back to Server F64…');
        }
      }

      if (browserRes) {
        state.frameRasterCache.clear();
        applyLoadedSimulation(browserRes.grid, browserRes.frames, browserRes.grid.run_id, browserRes.engine);
        if (!state.fixedView) {
          const [minLon, minLat, maxLon, maxLat] = browserRes.grid.metadata.wgs84_bounds;
          map.flyTo({
            center: [(minLon + maxLon) * 0.5, (minLat + maxLat) * 0.5],
            zoom: 14.4,
            pitch: state.is3D ? 48 : 0,
            duration: 800,
          });
        }
      } else {
        const useAsyncJob = payload.duration_s > 900 || payload.cols * payload.rows > 8192;
        let summary = null;
        if (!useAsyncJob) {
          const resp = await fetch('/api/v1/simulations', {
            method: 'POST',
            headers: { 'content-type': 'application/json' },
            body: JSON.stringify(payload),
            signal: abort.signal,
          });
          if (resp.status === 408) {
            setSimProgress('Long run queued as async basin job…');
          } else if (!resp.ok) {
            throw new Error(await parseErrorText(resp));
          } else {
            summary = await resp.json();
          }
        }
        if (!summary) {
          const jobResp = await fetch('/api/v1/jobs', {
            method: 'POST',
            headers: { 'content-type': 'application/json' },
            body: JSON.stringify(payload),
            signal: abort.signal,
          });
          if (!jobResp.ok) throw new Error(await parseErrorText(jobResp));
          const job = await jobResp.json();
          const jobId = job.job_id;
          const onCancelJob = () => {
            fetch(`/api/v1/jobs/${jobId}/cancel`, { method: 'POST' }).catch(() => {});
          };
          abort.signal.addEventListener('abort', onCancelJob, { once: true });
          try {
            while (true) {
              if (abort.signal.aborted) {
                const err = new Error('Aborted');
                err.name = 'AbortError';
                throw err;
              }
              const pollResp = await fetch(`/api/v1/jobs/${jobId}`, { signal: abort.signal });
              if (!pollResp.ok) throw new Error(await parseErrorText(pollResp));
              const rec = await pollResp.json();
              if (rec.status === 'completed' && rec.summary) {
                summary = rec.summary;
                break;
              }
              if (rec.status === 'failed') {
                throw new Error(rec.error || 'Async basin job failed');
              }
              if (rec.status === 'cancelled') {
                const err = new Error('Aborted');
                err.name = 'AbortError';
                throw err;
              }
              setSimProgress(`Solving basin job… ${Math.round(rec.progress_pct || 0)}%`);
              await new Promise((r) => setTimeout(r, 450));
            }
          } finally {
            abort.signal.removeEventListener('abort', onCancelJob);
          }
        }
        state.frameRasterCache.clear();
        setSimProgress('Loading frames…');
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
    }
  } catch (err) {
    if (err && err.name === 'AbortError') {
      showToast('Simulation cancelled.');
    } else {
      showToast(`Simulation error: ${String(err.message || err).slice(0, 140)}`, true);
    }
  } finally {
    if (state.activeAbort === abort) state.activeAbort = null;
    setSimProgress(null);
    if (btn) {
      btn.disabled = false;
      btn.innerHTML = orig;
    }
  }
}

async function parseErrorText(resp) {
  const txt = await resp.text();
  try {
    const j = JSON.parse(txt);
    if (j && j.error) return j.error;
  } catch (_) {}
  return txt || `HTTP ${resp.status}`;
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
    rainPart = fmtRain(state.simConfig.rainfallMmHr || 0, 0);
  }
  const wind = state.simConfig.windX || 0;
  const windPart = wind > 0 ? ` · ${fmtSpeed(wind, 0)}` : '';
  $('telemetry').textContent = `${c.lat.toFixed(2)}°, ${c.lng.toFixed(2)}° · z${z.toFixed(1)} · ${rainPart}${windPart}`;
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
  pill.title = `Engine: ${state.activeEngine} · Mass residual: ${res.toExponential(1)} m³ (screening only)`;
  const isPres = document.documentElement.dataset.presentation === 'true';
  txt.textContent = isPres
    ? `${demTag} screening · Mass ${ok ? 'OK' : 'FAIL'}`
    : `${state.activeEngine} · ${demTag} · Mass ${ok ? 'OK' : 'FAIL'}`;
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
  if (state.displayPrefs.clock === '24h') {
    return `${dow} ${day} · ${String(h24).padStart(2, '0')}:${String(mi).padStart(2, '0')} ${tzLabel}`;
  }
  const ampm = h24 >= 12 ? 'PM' : 'AM';
  const h12 = h24 % 12 || 12;
  const minPart = mi === 0 ? '' : `:${String(mi).padStart(2, '0')}`;
  return `${dow} ${day} · ${h12}${minPart} ${ampm} ${tzLabel}`;
}

function renderForecastSegments() {
  const bar = $('forecast-bar');
  const tl = $('timeline');
  if (!bar || !tl) return;
  document.body.classList.toggle('has-forecast-bar', state.spanHours > 0);
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
  const show = isPres
    ? true
    : (state.displayPrefs.showMetrics && (state.profile === 'public' || !$('public-outlook').classList.contains('hidden')));
  card.classList.toggle('hidden', !show);
  if (!show) return;

  if (!state.boundaryCellMask || state.boundaryCellMask.length !== state.grid.metadata.cols * state.grid.metadata.rows) {
    rebuildBoundaryCellMask();
  }
  const meta = state.grid.metadata;
  const mask = state.boundaryCellMask;
  const cut = state.displayPrefs.depthCutoffM || 0.06;
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
    if (h >= cut) {
      floodedCells++;
      stagnantCells++;
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
        if (prevDepth[i] >= cut) prevFlooded++;
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
  $('lm-avg').textContent = fmtDepth(avgDepth, 2);
  $('lm-peak').textContent = fmtDepth(peakDepth, 2);
  $('lm-pct').textContent = `${floodedPct.toFixed(0)}%`;
  const stagEl = $('lm-stagnant');
  if (stagEl) stagEl.textContent = fmtArea(stagnantKm2, 1);
  if (state.profile === 'public' && !isPres && !state.directorOutlookSet) {
    const po = $('public-outlook');
    if (po) {
      const sel = $('preset-select');
      const locName = sel && sel.selectedOptions[0] ? sel.selectedOptions[0].textContent : 'Local Flood Screening';
      $('po-title').textContent = locName;
      const risk = $('po-risk');
      risk.className = `po-risk ${lvl}`;
      risk.textContent = label;
      const sc = state.simConfig;
      const modeDesc = sc.mode === 'stress'
        ? `${sc.stressMmDay} mm/day × ${sc.stressDays}d stress-test`
        : `${fmtRain(sc.rainfallMmHr * (sc.rainMult || 1), 0)} scenario`;
      $('po-summary').textContent =
        `Peak simulated depth ${fmtDepth(peakDepth, 2)} across ${floodedPct.toFixed(0)}% of the area (${modeDesc}). Open Simulate… to test other rain or site pad scenarios.`;
      $('po-hotspots').innerHTML = '';
      if (state.serverConfig?.disclaimer) $('po-foot').textContent = state.serverConfig.disclaimer;
      po.classList.remove('hidden');
    }
  }
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
      let tideIcon = 'arus-tide-steady-normal';
      if (s.kind === 'tide' && state.hourlyTideM && state.hourlyTideM.length > 0) {
        const tIdx = Math.min(state.hourlyTideM.length - 1, Math.floor(frac * state.hourlyTideM.length));
        const prevIdx = Math.max(0, tIdx - 2);
        const tideM = state.hourlyTideM[tIdx];
        const delta = tideM - state.hourlyTideM[prevIdx];
        const trend = delta > 0.04 ? 'up' : delta < -0.04 ? 'down' : 'steady';
        const level = tideM >= 1.85 ? 'high' : 'normal';
        tideIcon = `arus-tide-${trend}-${level}`;
        const sign = tideM >= 0 ? '+' : '';
        const arrow = trend === 'up' ? '▲' : trend === 'down' ? '▼' : '–';
        const tag = tideM >= 1.85 ? `High ${arrow}` : trend === 'up' ? `Rising ${arrow}` : trend === 'down' ? `Falling ${arrow}` : `Steady ${arrow}`;
        tone = tideM >= 1.85 ? 'warn' : '';
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
        statusText = hCur >= 0.06 ? `~${hCur.toFixed(1)} m est.` : 'Dry (<6 cm)';
      }

      pinFeatures.push({
        type: 'Feature',
        geometry: { type: 'Point', coordinates: [s.lon, s.lat] },
        properties: {
          name: s.name || '',
          kind: s.kind || 'gauge',
          icon: tideIcon,
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
  const tot = state.frames[state.frames.length - 1].time_s || 0;
  if (state.spanHours > 0) {
    const ts = fmtCalendarTime(state.startEpochMs + cur * 1000, state.tzLabel);
    $('time-readout').textContent = `${ts} (+${fmtMMSS(cur)})`;
  } else {
    $('time-readout').textContent = `${fmtMMSS(cur)} / ${fmtMMSS(tot)}`;
  }

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
  updateFloodPinBar();
  if (map && layersReady && map.getLayer('arus-domain-outline')) {
    const isPres = document.documentElement.dataset.presentation === 'true';
    const showBox = !isPres && !state.boundaryGeoJson && ['pick', 'floodpin', 'bund', 'pond'].includes(tool);
    map.setLayoutProperty('arus-domain-outline', 'visibility', showBox ? 'visible' : 'none');
  }
}

function setLayer(layer) {
  state.layer = layer;
  document.querySelectorAll('[data-layer]').forEach((el) => {
    el.classList.toggle('active', el.dataset.layer === layer);
  });
  const bar = $('legend-bar');
  const ticks = $('legend-ticks');
  const note = $('legend-note');
  const cut = state.displayPrefs.depthCutoffM || 0.06;
  if (layer === 'depth') {
    bar.style.background = 'linear-gradient(90deg, #38bdf8, #0284c7, #1d4ed8, #1e1b4b)';
    ticks.innerHTML = `<span>${fmtDepth(cut, 2)}</span><span>${fmtDepth(0.30, 2)}</span><span>${fmtDepth(1.10, 1)}+</span>`;
    note.textContent = `Flood depth h (sheet runoff < ${Math.round(cut * 100)} cm hidden).`;
  } else if (layer === 'velocity') {
    bar.style.background = 'linear-gradient(90deg, #2dd4bf, #0284c7, #4f46e5)';
    ticks.innerHTML = `<span>${fmtSpeed(0.0, 1)}</span><span>${fmtSpeed(0.7, 1)}</span><span>${fmtSpeed(1.4, 1)}+</span>`;
    note.textContent = 'Depth-averaged velocity |v| with streamlines.';
  } else if (layer === 'hazard') {
    bar.style.background = 'linear-gradient(90deg, #38bdf8 0% 25%, #facc15 25% 50%, #f97316 50% 75%, #e11d48 75% 100%)';
    ticks.innerHTML = '<span>Low</span><span>Mod</span><span>Ext</span>';
    note.textContent = 'DEFRA FD2320 hazard rating: h × (v + 0.5).';
  } else if (layer === 'arrival') {
    bar.style.background = 'linear-gradient(90deg, #e11d48, #f97316, #facc15, #38bdf8)';
    ticks.innerHTML = '<span>Early</span><span>Mid</span><span>Late</span>';
    note.textContent = 'Flood arrival time (onset ETA from storm start).';
  } else {
    bar.style.background = 'linear-gradient(90deg, #0d9488, #cbd5e1 50%, #e11d48)';
    ticks.innerHTML = `<span>−${fmtDepth(0.14, 2)} Safer</span><span>0</span><span>+${fmtDepth(0.14, 2)} Worse</span>`;
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
    renderFloodCanvas(false);
  }
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

  document.querySelectorAll('[data-profile]').forEach((el) => {
    el.addEventListener('click', () => setUiProfile(el.dataset.profile, true));
  });
  document.querySelectorAll('[data-tool]').forEach((el) => {
    el.addEventListener('click', () => setTool(el.dataset.tool));
  });
  document.querySelectorAll('[data-layer]').forEach((el) => {
    el.addEventListener('click', () => {
      state.displayPrefs.layer = el.dataset.layer;
      setLayer(el.dataset.layer);
      saveUserPrefs({ displayPrefs: state.displayPrefs });
    });
  });

  $('btn-open-layers').addEventListener('click', openLayersModal);
  $('btn-layers-close').addEventListener('click', () => $('layers-modal').classList.add('hidden'));
  $('btn-layers-cancel').addEventListener('click', () => $('layers-modal').classList.add('hidden'));
  $('btn-layers-apply').addEventListener('click', applyLayersModalDraft);
  $('btn-layers-reset').addEventListener('click', () => {
    resetUserPrefs();
    state.displayPrefs = {
      showFlood: true, showRivers: true, showBoundary: true, showMarkers: true,
      showParticles: true, is3D: true, showLabels: true, basemap: 'topo', layer: 'depth',
      depthCutoffM: 0.06, floodOpacity: 0.95, riverOpacity: 0.72,
      units: 'metric', clock: '12h', showMetrics: true, showLegend: true,
    };
    populateLayersModal();
    applyDisplayPrefsToMap();
    $('layers-modal').classList.add('hidden');
  });
  $('opt-flood-opacity').addEventListener('input', (e) => { $('val-flood-opacity').textContent = `${e.target.value}%`; });
  $('opt-river-opacity').addEventListener('input', (e) => { $('val-river-opacity').textContent = `${e.target.value}%`; });

  $('btn-open-sim').addEventListener('click', openSimModal);
  $('btn-sim-close').addEventListener('click', () => $('sim-modal').classList.add('hidden'));
  $('btn-sim-cancel').addEventListener('click', () => $('sim-modal').classList.add('hidden'));
  $('btn-sim-apply').addEventListener('click', applySimModalDraftAndRun);
  $('btn-sim-reset').addEventListener('click', () => {
    state.simConfig = {
      mode: 'forecast', horizon: '5d', rainMult: 1.0, stressMmDay: 150, stressDays: 3, stressPadM: 0,
      rainfallMmHr: 280, durationS: 360, hyetograph: 'peak-storm', soil: 'impervious',
      boundary: 'transmissive', manningN: 0.035, windX: 0, startDry: false,
      enableEvap: true, enableDrain: true, tideMode: 'live', compute: 'server',
    };
    populateSimModal();
  });
  document.querySelectorAll('[data-sim-tab]').forEach((el) => {
    el.addEventListener('click', () => switchSimTab(el.dataset.simTab));
  });
  $('sim-stress-preset').addEventListener('change', (e) => {
    if (e.target.value !== 'custom') $('sim-stress-mm-day').value = e.target.value;
  });
  $('btn-stress-pick').addEventListener('click', () => {
    state.pickingStressSite = true;
    document.body.classList.add('picking-site');
    $('sim-modal').classList.add('hidden');
    showToast('Click anywhere on the map to select your site (Esc to cancel).');
  });

  $('btn-share').addEventListener('click', copyShareLink);
  $('btn-sim-abort').addEventListener('click', () => { if (state.activeAbort) state.activeAbort.abort(); });
  $('btn-toast-close').addEventListener('click', () => $('toast-bar').classList.add('hidden'));
  $('btn-toast-retry').addEventListener('click', () => {
    $('toast-bar').classList.add('hidden');
    runSimulation(state.lastCompareMode);
  });
  $('btn-sv-close').addEventListener('click', () => $('site-verdict').classList.add('hidden'));
  $('sv-ffe-select').addEventListener('change', renderSiteVerdict);
  $('btn-sv-raise').addEventListener('click', () => {
    if (!state.verdictTarget) return;
    state.stressSite = { lat: state.verdictTarget.lat, lon: state.verdictTarget.lon };
    state.simConfig.mode = 'stress';
    state.simConfig.stressPadM = 0.8;
    $('site-verdict').classList.add('hidden');
    openSimModal();
  });
  $('btn-inspector-pin').addEventListener('click', () => {
    if (!state.selectedCell) return;
    const exists = state.pinnedProbes.some((p) => p.col === state.selectedCell.col && p.row === state.selectedCell.row);
    if (!exists) {
      if (state.pinnedProbes.length >= 3) state.pinnedProbes.shift();
      state.pinnedProbes.push({ ...state.selectedCell });
      showToast(`Pinned probe (${state.selectedCell.col}, ${state.selectedCell.row}) · ${state.pinnedProbes.length} pinned.`);
    }
    renderHydrographInspector();
  });

  const togglePlay = () => {
    state.playing = !state.playing;
    const icon = state.playing ? '❚❚' : '▶';
    if ($('btn-play')) $('btn-play').textContent = icon;
    if ($('fb-play')) $('fb-play').textContent = icon;
  };
  if ($('fb-play')) $('fb-play').addEventListener('click', togglePlay);
  if ($('fb-track')) {
    const scrubForecast = (ev) => {
      if (!state.frames.length) return;
      const rect = $('fb-track').getBoundingClientRect();
      const frac = Math.min(1, Math.max(0, (ev.clientX - rect.left) / Math.max(1, rect.width)));
      state.playing = false;
      if ($('btn-play')) $('btn-play').textContent = '▶';
      if ($('fb-play')) $('fb-play').textContent = '▶';
      state.frameIdx = Math.round(frac * (state.frames.length - 1));
      $('scrubber').value = String(state.frameIdx);
      updateTimeReadout();
      renderFloodCanvas(true);
    };
    let dragging = false;
    $('fb-track').addEventListener('pointerdown', (e) => { dragging = true; scrubForecast(e); });
    window.addEventListener('pointermove', (e) => { if (dragging) scrubForecast(e); });
    window.addEventListener('pointerup', () => { dragging = false; });
  }
  if ($('fb-segments')) {
    $('fb-segments').addEventListener('click', (e) => {
      const seg = e.target.closest('.fb-seg');
      if (!seg || !state.frames.length) return;
      const all = Array.from(document.querySelectorAll('#fb-segments .fb-seg'));
      const i = all.indexOf(seg);
      if (i < 0) return;
      state.playing = false;
      if ($('btn-play')) $('btn-play').textContent = '▶';
      if ($('fb-play')) $('fb-play').textContent = '▶';
      state.frameIdx = Math.min(state.frames.length - 1, Math.round((i / Math.max(1, all.length)) * (state.frames.length - 1)));
      $('scrubber').value = String(state.frameIdx);
      updateTimeReadout();
      renderFloodCanvas(true);
    });
  }

  ['sources-modal', 'layers-modal', 'sim-modal'].forEach((id) => {
    const el = $(id);
    if (el) el.addEventListener('click', (e) => { if (e.target === el) el.classList.add('hidden'); });
  });

  window.addEventListener('keydown', (e) => {
    const tag = (e.target && e.target.tagName || '').toLowerCase();
    const typing = tag === 'input' || tag === 'select' || tag === 'textarea';
    if (e.key === 'Escape') {
      ['sources-modal', 'layers-modal', 'sim-modal'].forEach((id) => { const el = $(id); if (el) el.classList.add('hidden'); });
      if (state.pickingStressSite) {
        state.pickingStressSite = false;
        document.body.classList.remove('picking-site');
        showToast('Site pick cancelled.');
      } else if (state.tool !== 'inspect') {
        setTool('inspect');
      }
      return;
    }
    if (typing) return;
    if (e.key === ' ') {
      e.preventDefault();
      togglePlay();
    } else if (e.key === 'ArrowRight' && state.frames.length) {
      e.preventDefault();
      state.playing = false;
      if ($('btn-play')) $('btn-play').textContent = '▶';
      if ($('fb-play')) $('fb-play').textContent = '▶';
      state.frameIdx = (state.frameIdx + 1) % state.frames.length;
      $('scrubber').value = String(state.frameIdx);
      updateTimeReadout();
      renderFloodCanvas(true);
    } else if (e.key === 'ArrowLeft' && state.frames.length) {
      e.preventDefault();
      state.playing = false;
      if ($('btn-play')) $('btn-play').textContent = '▶';
      if ($('fb-play')) $('fb-play').textContent = '▶';
      state.frameIdx = (state.frameIdx - 1 + state.frames.length) % state.frames.length;
      $('scrubber').value = String(state.frameIdx);
      updateTimeReadout();
      renderFloodCanvas(true);
    }
  });

  $('btn-env').addEventListener('click', () => {
    state.envelopeMode = !state.envelopeMode;
    $('btn-env').textContent = state.envelopeMode ? 'Peak' : 'Live';
    $('btn-env').classList.toggle('active', state.envelopeMode);
    renderFloodCanvas(true);
  });

  $('btn-particles').addEventListener('click', () => {
    state.particlesOn = !state.particlesOn;
    state.displayPrefs.showParticles = state.particlesOn;
    $('btn-particles').classList.toggle('active', state.particlesOn);
    saveUserPrefs({ displayPrefs: state.displayPrefs });
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

  $('btn-iv-undo').addEventListener('click', () => { state.interventions.splice(-4, 4); updateInterventionBar(); renderFloodCanvas(false); });
  $('btn-iv-clear').addEventListener('click', () => { state.interventions = []; updateInterventionBar(); renderFloodCanvas(false); });
  if ($('btn-fp-clear')) {
    $('btn-fp-clear').addEventListener('click', () => {
      state.observedGauges = [];
      updateFloodPinBar();
      showToast('Cleared observed flood pins.');
    });
  }
  $('btn-run').addEventListener('click', () => runSimulation(false));
  $('btn-compare').addEventListener('click', () => runSimulation(true));
  $('btn-nwp').addEventListener('click', fetchLiveMultiModelNwp);
  $('btn-nwp-apply').addEventListener('click', () => {
    if (state.nwpPeakMmHr !== null) {
      const clamped = Math.min(400, Math.max(40, Math.round(state.nwpPeakMmHr / 10) * 10));
      state.simConfig.rainfallMmHr = clamped;
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
    if (typeof opts.rainfall_mm_per_hr === 'number') state.simConfig.rainfallMmHr = opts.rainfall_mm_per_hr;
    if (typeof opts.duration_s === 'number') state.simConfig.durationS = opts.duration_s;
    if (typeof opts.wind_x_m_per_s === 'number') state.simConfig.windX = opts.wind_x_m_per_s;
    if (typeof opts.manning_n === 'number') state.simConfig.manningN = opts.manning_n;
    if (opts.hyetograph) state.simConfig.hyetograph = opts.hyetograph;
    if (opts.soil) state.simConfig.soil = opts.soil;
    if (opts.boundary) state.simConfig.boundary = opts.boundary;
    if (typeof opts.start_dry === 'boolean') state.simConfig.startDry = opts.start_dry;
    if (typeof opts.enable_evaporation === 'boolean') state.simConfig.enableEvap = opts.enable_evaporation;
    if (typeof opts.enable_drainage === 'boolean') state.simConfig.enableDrain = opts.enable_drainage;
    if (opts.tide_mode) state.simConfig.tideMode = opts.tide_mode;
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
  loadBoundaryByQuery: (query, opts) => loadBoundaryByQuery(query, opts),
  setCustomBoundary: (lon, lat, radiusKm, shape, runSim) =>
    setCustomBoundary(lon, lat, radiusKm, shape, runSim),
  setPublicOutlook: (cfg) => {
    const card = $('public-outlook');
    if (!cfg) {
      state.directorOutlookSet = false;
      card.classList.add('hidden');
      return;
    }
    state.directorOutlookSet = true;
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
            kind: s.kind || 'gauge',
            icon: 'arus-tide-steady-normal',
            color: s.tone === 'danger' ? '#e11d48' : s.tone === 'warn' ? '#d97706' : '#0284c7',
          },
        }));
      map.getSource('arus-hotspots').setData({ type: 'FeatureCollection', features });
    }
    if (cfg.footnote) $('po-foot').textContent = cfg.footnote;
    card.classList.remove('hidden');
    if (state.frames.length > 0) updateLiveHotspots(state.frameIdx);
  },
  getPrefs: () => loadUserPrefs(),
  resetPrefs: () => resetUserPrefs(),
  getServerConfig: () => state.serverConfig || null,
};

const PREFS_KEY = 'arus.studio.prefs.v1';

function loadUserPrefs() {
  try {
    const raw = window.localStorage.getItem(PREFS_KEY);
    if (!raw) return {};
    const parsed = JSON.parse(raw);
    return parsed && typeof parsed === 'object' ? parsed : {};
  } catch (_) {
    return {};
  }
}

function saveUserPrefs(patch) {
  if (document.documentElement.dataset.presentation === 'true' || state.fixedView) return;
  try {
    const merged = { ...loadUserPrefs(), ...patch };
    window.localStorage.setItem(PREFS_KEY, JSON.stringify(merged));
  } catch (_) { /* storage unavailable in private mode */ }
}

function resetUserPrefs() {
  try {
    window.localStorage.removeItem(PREFS_KEY);
  } catch (_) { /* ignore */ }
}

function setUiProfile(profile, persist = false) {
  state.profile = profile === 'public' ? 'public' : 'researcher';
  document.documentElement.dataset.profile = state.profile;
  document.querySelectorAll('[data-profile]').forEach((el) => {
    el.classList.toggle('active', el.dataset.profile === state.profile);
  });
  if (state.profile === 'public') {
    if (state.tool !== 'inspect') setTool('inspect');
    if (state.layer === 'diff') setLayer('depth');
    if ($('inspector')) $('inspector').classList.add('hidden');
  } else {
    if ($('site-verdict')) $('site-verdict').classList.add('hidden');
  }
  updateLiveMetrics();
  if (persist) saveUserPrefs({ profile: state.profile });
}

function buildShareUrl() {
  const qs = new URLSearchParams();
  const c = map ? map.getCenter() : { lat: state.center.lat, lng: state.center.lon };
  qs.set('lat', c.lat.toFixed(4));
  qs.set('lon', c.lng.toFixed(4));
  if (map) qs.set('zoom', map.getZoom().toFixed(2));
  qs.set('profile', state.profile);
  qs.set('basemap', state.basemap);
  qs.set('layer', state.layer);
  const sc = state.simConfig;
  qs.set('mode', sc.mode);
  if (sc.mode === 'stress') {
    qs.set('stress_mm', String(sc.stressMmDay));
    qs.set('stress_days', String(sc.stressDays));
    if (sc.stressPadM > 0) qs.set('stress_pad', String(sc.stressPadM));
  } else {
    qs.set('rain', String(sc.rainfallMmHr));
    qs.set('dur', String(sc.durationS));
    if (sc.rainMult !== 1) qs.set('rain_mult', String(sc.rainMult));
  }
  if (sc.startDry) qs.set('start_dry', '1');
  if (!sc.enableEvap) qs.set('evap', '0');
  if (!sc.enableDrain) qs.set('drain', '0');
  if (sc.tideMode !== 'live') qs.set('tide', sc.tideMode);
  if (sc.soil !== 'impervious') qs.set('soil', sc.soil);
  return `${window.location.origin}${window.location.pathname}?${qs.toString()}`;
}

async function copyShareLink() {
  const url = buildShareUrl();
  try { window.history.replaceState(null, '', url); } catch (_) {}
  try {
    await navigator.clipboard.writeText(url);
    showToast('Shareable scenario link copied to clipboard.');
  } catch (_) {
    showToast('Scenario URL updated in address bar.');
  }
}

function populateLayersModal() {
  const p = state.displayPrefs;
  $('opt-show-flood').checked = p.showFlood;
  $('opt-show-rivers').checked = p.showRivers;
  $('opt-show-boundary').checked = p.showBoundary;
  $('opt-show-markers').checked = p.showMarkers;
  $('opt-show-particles').checked = p.showParticles;
  $('opt-is-3d').checked = p.is3D;
  $('opt-show-labels').checked = p.showLabels;
  $('opt-basemap').value = p.basemap;
  $('opt-layer').value = p.layer;
  $('opt-cutoff').value = String(p.depthCutoffM);
  const fPct = Math.round((p.floodOpacity || 0.95) * 100);
  $('opt-flood-opacity').value = String(fPct);
  $('val-flood-opacity').textContent = `${fPct}%`;
  const rPct = Math.round((p.riverOpacity || 0.72) * 100);
  $('opt-river-opacity').value = String(rPct);
  $('val-river-opacity').textContent = `${rPct}%`;
  $('opt-units').value = p.units;
  $('opt-clock').value = p.clock;
  $('opt-show-metrics').checked = p.showMetrics;
  $('opt-show-legend').checked = p.showLegend;
}

function openLayersModal() {
  populateLayersModal();
  $('layers-modal').classList.remove('hidden');
}

function applyDisplayPrefsToMap() {
  const p = state.displayPrefs;
  state.particlesOn = p.showParticles;
  if ($('btn-particles')) $('btn-particles').classList.toggle('active', p.showParticles);
  if (p.basemap !== state.basemap) setBasemap(p.basemap);
  if (p.is3D !== state.is3D) {
    state.is3D = p.is3D;
    if (map) {
      try {
        map.setTerrain(state.is3D && map.getSource('arus-terrarium-dem')
          ? { source: 'arus-terrarium-dem', exaggeration: 1.35 }
          : null);
      } catch (_) {}
      map.easeTo({ pitch: state.is3D ? 48 : 0, bearing: state.is3D ? -16 : 0, duration: 400 });
    }
  }
  const layerPanel = document.querySelector('.layer-panel');
  if (layerPanel && document.documentElement.dataset.presentation !== 'true') {
    layerPanel.classList.toggle('hidden', !p.showLegend);
  }
  if (map && layersReady) {
    const setVis = (id, on) => { if (map.getLayer(id)) map.setLayoutProperty(id, 'visibility', on ? 'visible' : 'none'); };
    setVis('arus-water-smooth', p.showFlood);
    setVis('arus-particles-line', p.showFlood && p.showParticles);
    setVis('arus-river-line', p.showRivers);
    setVis('arus-river-halo', p.showRivers);
    setVis('arus-region-outline', p.showBoundary);
    setVis('arus-region-mask-fill', p.showBoundary);
    setVis('sat-labels', p.showLabels);
    setVis('arus-domain-outline', p.showBoundary && !state.boundaryGeoJson && document.documentElement.dataset.presentation !== 'true');
    setVis('arus-hotspots-circle', p.showMarkers);
    setVis('arus-hotspots-label', p.showMarkers);
    setVis('arus-hotspots-tide', p.showMarkers);
    if (map.getLayer('arus-water-smooth')) map.setPaintProperty('arus-water-smooth', 'raster-opacity', p.floodOpacity);
    if (map.getLayer('arus-river-line')) map.setPaintProperty('arus-river-line', 'line-opacity', p.riverOpacity);
    if (map.getLayer('arus-river-halo')) map.setPaintProperty('arus-river-halo', 'line-opacity', p.riverOpacity * 0.45);
    const style = map.getStyle();
    if (style && Array.isArray(style.layers)) {
      for (const lyr of style.layers) {
        if (lyr.type === 'symbol' && !lyr.id.startsWith('arus-')) {
          map.setLayoutProperty(lyr.id, 'visibility', p.showLabels ? 'visible' : 'none');
        }
      }
    }
  }
  setLayer(p.layer);
  updateLiveMetrics();
  updateTimeReadout();
}

function applyLayersModalDraft() {
  state.displayPrefs = {
    showFlood: $('opt-show-flood').checked,
    showRivers: $('opt-show-rivers').checked,
    showBoundary: $('opt-show-boundary').checked,
    showMarkers: $('opt-show-markers').checked,
    showParticles: $('opt-show-particles').checked,
    is3D: $('opt-is-3d').checked,
    showLabels: $('opt-show-labels').checked,
    basemap: $('opt-basemap').value,
    layer: $('opt-layer').value,
    depthCutoffM: parseFloat($('opt-cutoff').value) || 0.06,
    floodOpacity: (parseInt($('opt-flood-opacity').value, 10) || 95) / 100,
    riverOpacity: (parseInt($('opt-river-opacity').value, 10) || 72) / 100,
    units: $('opt-units').value,
    clock: $('opt-clock').value,
    showMetrics: $('opt-show-metrics').checked,
    showLegend: $('opt-show-legend').checked,
  };
  saveUserPrefs({ displayPrefs: state.displayPrefs });
  applyDisplayPrefsToMap();
  $('layers-modal').classList.add('hidden');
}

function switchSimTab(tab) {
  state.simConfig.mode = tab;
  document.querySelectorAll('[data-sim-tab]').forEach((el) => {
    el.classList.toggle('active', el.dataset.simTab === tab);
  });
  ['forecast', 'stress', 'physics'].forEach((t) => {
    const pane = $(`sim-tab-${t}`);
    if (pane) pane.classList.toggle('hidden', t !== tab);
  });
}

function populateSimModal() {
  const sc = state.simConfig;
  switchSimTab(sc.mode || 'forecast');
  $('sim-horizon').value = sc.horizon;
    const multStr = String(Number(sc.rainMult));
    $('sim-rain-mult').value = multStr;
  $('sim-stress-mm-day').value = String(sc.stressMmDay);
  $('sim-stress-days').value = String(sc.stressDays);
  $('sim-stress-pad').value = String(sc.stressPadM);
  const site = state.stressSite || state.center;
  $('sim-stress-coord').textContent = `${site.lat.toFixed(4)}°, ${site.lon.toFixed(4)}°`;
  $('sim-start-dry').checked = sc.startDry;
  $('sim-enable-evap').checked = sc.enableEvap;
  $('sim-enable-drain').checked = sc.enableDrain;
  $('sim-tide-mode').value = sc.tideMode;
  $('sim-compute').value = sc.compute;
  $('sim-rain').value = String(sc.rainfallMmHr);
  $('sim-dur').value = String(sc.durationS);
  $('sim-hyeto').value = sc.hyetograph;
  $('sim-soil').value = sc.soil;
  $('sim-boundary').value = sc.boundary;
  $('sim-manning').value = String(sc.manningN);
  $('sim-wind').value = String(sc.windX);
  renderServerContextBox();
}

function openSimModal() {
  populateSimModal();
  $('sim-modal').classList.remove('hidden');
}

function applySimModalDraftAndRun() {
  const sc = state.simConfig;
  sc.horizon = $('sim-horizon').value;
  sc.rainMult = parseFloat($('sim-rain-mult').value) || 1.0;
  sc.stressMmDay = parseFloat($('sim-stress-mm-day').value) || 150;
  sc.stressDays = parseInt($('sim-stress-days').value, 10) || 3;
  sc.stressPadM = parseFloat($('sim-stress-pad').value) || 0;
  sc.startDry = $('sim-start-dry').checked;
  sc.enableEvap = $('sim-enable-evap').checked;
  sc.enableDrain = $('sim-enable-drain').checked;
  sc.tideMode = $('sim-tide-mode').value;
  sc.compute = $('sim-compute').value;
  sc.rainfallMmHr = parseFloat($('sim-rain').value) || 280;
  sc.durationS = parseFloat($('sim-dur').value) || 360;
  sc.hyetograph = $('sim-hyeto').value;
  sc.soil = $('sim-soil').value;
  sc.boundary = $('sim-boundary').value;
  sc.manningN = parseFloat($('sim-manning').value) || 0.035;
  sc.windX = parseFloat($('sim-wind').value) || 0;
  saveUserPrefs({ simConfig: sc });
  $('sim-modal').classList.add('hidden');

  let runCompare = false;
  if (sc.mode === 'stress' && sc.stressPadM > 0 && state.grid) {
    const pt = state.stressSite || state.center;
    const hit = lngLatToCell(pt.lon, pt.lat);
    if (hit) {
      state.interventions = [];
      for (let dr = 0; dr <= 1; dr++) {
        for (let dc = 0; dc <= 1; dc++) {
          const c = hit.col + dc;
          const r = hit.row + dr;
          if (c < state.grid.metadata.cols && r < state.grid.metadata.rows) {
            state.interventions.push({ col: c, row: r, delta_m: sc.stressPadM, reason: 'raised site pad' });
          }
        }
      }
      updateInterventionBar();
      runCompare = true;
    }
  }
  runSimulation(runCompare);
}

function renderServerContextBox() {
  const el = $('sim-context-info');
  if (!el) return;
  const resp = state.serverContext;
  if (!resp || !resp.context) {
    el.textContent = 'No regional server context configured. Running with studio scenario parameters.';
    return;
  }
  const c = resp.context;
  const parts = [];
  parts.push(`<div><strong>Basin:</strong> ${c.basin_id} · <strong>Cycle:</strong> ${c.cycle_iso}</div>`);
  if (resp.is_stale) {
    parts.push(`<div class="stale-warn">Context is stale (${Math.round(resp.age_seconds / 3600)}h old).</div>`);
  }
  if (c.mjo) {
    parts.push(`<div><span class="mjo-pill">MJO Phase ${c.mjo.phase} · Amp ${c.mjo.amplitude.toFixed(1)}</span> ${c.mjo.label}</div>`);
  }
  if (c.ensemble_15d && Array.isArray(c.ensemble_15d.days) && c.ensemble_15d.days.length > 0) {
    const peakP90 = Math.max(...c.ensemble_15d.days.map((d) => d.p90_mm || 0));
    parts.push(`<div><strong>15-Day Ensemble (${c.ensemble_15d.model}):</strong> ${c.ensemble_15d.members} members, peak P90 ${peakP90.toFixed(0)} mm/day.</div>`);
  }
  el.innerHTML = parts.join('');
}

async function fetchRegionalContext(qs) {
  if (qs.get('presentation') === '1' || qs.get('presentation') === 'true') return;
  if (qs.get('fixed') === '1' || qs.get('fixed') === 'true') return;
  try {
    const resp = await fetch('/api/v1/context');
    if (!resp.ok) return;
    const data = await resp.json();
    state.serverContext = data;
    const ctx = data.context;
    if (ctx && Array.isArray(ctx.wgs84_bounds) && ctx.wgs84_bounds.length === 4) {
      const [minLon, minLat, maxLon, maxLat] = ctx.wgs84_bounds;
      const inBounds = state.center.lon >= minLon && state.center.lon <= maxLon
        && state.center.lat >= minLat && state.center.lat <= maxLat;
      if (inBounds) {
        if (ctx.spatial_rain_grid) state.spatialRainGrid = ctx.spatial_rain_grid;
        if (Array.isArray(ctx.hourly_tide_m) && ctx.hourly_tide_m.length > 0) state.hourlyTideM = ctx.hourly_tide_m;
      }
    }
    renderServerContextBox();
  } catch (_) { /* optional server context */ }
}

function applyUserPrefs(qs) {
  if (qs.get('presentation') === '1' || qs.get('presentation') === 'true') return;
  if (qs.get('fixed') === '1' || qs.get('fixed') === 'true') return;
  const prefs = loadUserPrefs();
  if (prefs.profile) setUiProfile(prefs.profile, false);
  if (prefs.displayPrefs && typeof prefs.displayPrefs === 'object') {
    state.displayPrefs = { ...state.displayPrefs, ...prefs.displayPrefs };
    if (!qs.has('basemap')) {
      state.basemap = state.displayPrefs.basemap;
      document.documentElement.dataset.theme = state.basemap === 'topo' ? 'light' : 'dark';
    }
    state.is3D = Boolean(state.displayPrefs.is3D);
    state.layer = state.displayPrefs.layer || 'depth';
    state.particlesOn = Boolean(state.displayPrefs.showParticles);
  }
  if (prefs.simConfig && typeof prefs.simConfig === 'object') {
    state.simConfig = { ...state.simConfig, ...prefs.simConfig };
  }
}

async function fetchAndApplyServerConfig(qs) {
  try {
    const resp = await fetch('/api/v1/config');
    if (!resp.ok) return;
    const cfg = await resp.json();
    state.serverConfig = cfg;
    if (cfg.instance_title) document.title = cfg.instance_title;
    const defaultProfile = cfg.default_ui_profile || (cfg.mode === 'hosted' ? 'public' : 'researcher');
    setUiProfile(defaultProfile, false);
    const f = cfg.features || {};
    const hide = (el) => { if (el) el.style.display = 'none'; };
    if (f.allow_interventions === false) {
      document.querySelectorAll('[data-tool="bund"], [data-tool="pond"]').forEach(hide);
      hide($('btn-compare'));
      if ($('sim-stress-pad')) $('sim-stress-pad').disabled = true;
      hide($('btn-stress-pick'));
    }
    if (f.allow_byod_dem_upload === false) {
      hide($('file-asc'));
      document.querySelectorAll('label[for="file-asc"]').forEach(hide);
    }
    if (f.allow_mp4_export === false) {
      hide($('btn-rec'));
    }
    if (f.allow_direct_nwp_fetch === false) {
      hide($('nwp-section'));
    }
    if (f.allow_gis_export === false) {
      hide($('btn-export-asc'));
      hide($('btn-export-csv'));
    }
    const prefs = loadUserPrefs();
    const dv = cfg.default_view;
    if (dv && !qs.has('lat') && !qs.has('lon') && !prefs.last_center) {
      if (Number.isFinite(dv.lat) && Number.isFinite(dv.lon)) {
        state.center = { lat: dv.lat, lon: dv.lon };
        const matched = PRESETS.find((p) => Math.abs(p.lat - dv.lat) < 0.02 && Math.abs(p.lon - dv.lon) < 0.02);
        if (matched && $('preset-select')) {
          $('preset-select').value = matched.id;
        } else {
          setCustomPresetLabel(`Default · ${dv.lat.toFixed(4)}, ${dv.lon.toFixed(4)}`);
        }
      }
      if (!qs.has('zoom') && Number.isFinite(dv.zoom)) state.initZoom = dv.zoom;
      if (!qs.has('basemap') && !prefs.displayPrefs?.basemap && (dv.basemap === 'topo' || dv.basemap === 'sat' || dv.basemap === 'dark')) {
        state.basemap = dv.basemap;
        state.displayPrefs.basemap = dv.basemap;
        document.documentElement.dataset.theme = dv.basemap === 'topo' ? 'light' : 'dark';
      }
    }
  } catch (_) { /* server config optional in offline mode */ }
}

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
  if (qs.get('profile') === 'public' || qs.get('profile') === 'researcher') {
    setUiProfile(qs.get('profile'), false);
  }
  if (qs.get('layer') && ['depth', 'velocity', 'hazard', 'arrival', 'diff'].includes(qs.get('layer'))) {
    state.layer = qs.get('layer');
    state.displayPrefs.layer = state.layer;
  }
  if (qs.get('mode') && ['forecast', 'stress', 'physics'].includes(qs.get('mode'))) {
    state.simConfig.mode = qs.get('mode');
  }
  const rMult = parseFloat(qs.get('rain_mult'));
  if (Number.isFinite(rMult) && rMult > 0) state.simConfig.rainMult = rMult;
  const sMm = parseFloat(qs.get('stress_mm'));
  if (Number.isFinite(sMm) && sMm > 0) state.simConfig.stressMmDay = sMm;
  const sDays = parseInt(qs.get('stress_days'), 10);
  if (Number.isFinite(sDays) && sDays >= 1 && sDays <= 5) state.simConfig.stressDays = sDays;
  const sPad = parseFloat(qs.get('stress_pad'));
  if (Number.isFinite(sPad) && sPad >= 0) state.simConfig.stressPadM = sPad;
  if (qs.get('start_dry') === '1') state.simConfig.startDry = true;
  if (qs.get('evap') === '0') state.simConfig.enableEvap = false;
  if (qs.get('drain') === '0') state.simConfig.enableDrain = false;
  if (qs.get('tide') && ['live', 'spring-high', 'off'].includes(qs.get('tide'))) {
    state.simConfig.tideMode = qs.get('tide');
  }
  if (Object.keys(scen).length > 0) window.arusDirector.setScenario(scen);
}

window.addEventListener('DOMContentLoaded', async () => {
  const qs = new URLSearchParams(window.location.search);
  wireUi();
  await fetchAndApplyServerConfig(qs);
  applyUserPrefs(qs);
  applyStartupParams();
  await fetchRegionalContext(qs);
  initMap();
  setLayer(state.layer || 'depth');
  requestAnimationFrame(animationLoop);
});
