// ARUS 2026 Flood Simulation Studio Client
// Driven by real hydr 0.1.0 binary Float32 frames (/api/v1/runs/{id}/grid & frames.bin).

// Resolve API and asset URLs relative to the mounted Studio directory (works at / and /arus/).
const APP_BASE = new URL('.', window.location.href).href.replace(/\/$/, '');
const apiUrl = (path) => `${APP_BASE}/${String(path).replace(/^\/+/, '')}`;

let SERVER_PRESETS = [
  { id: 'default', name: 'Kuala Lumpur, Malaysia', lon: 101.696, lat: 3.149, zoom: 10.2, pitch: 0, bearing: 0, isServer: true },
];
let PRESETS = [...SERVER_PRESETS];

const state = {
  basemap: 'topo', // topo | sat | dark
  center: { lon: 109.2, lat: 4.15 },
  initZoom: 5.5,
  initPitch: 0,
  initBearing: 0,
  fixedView: false,
  tool: 'inspect', // inspect | pick | bund | pond | xsection
  layer: 'depth',  // depth | velocity | hazard | diff
  envelopeMode: false,
  particlesOn: true,
  is3D: true,
  playing: false,
  speed: 1,
  frameIdx: 0,
  lastTickMs: 0,
  runId: null,
  grid: null,
  frames: [],
  diffData: null,
  interventions: [],
  baselineInterventions: [],
  lastBundCell: null,
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
    depthCutoffM: 0.08,
    floodOpacity: 0.95,
    riverOpacity: 0.72,
    units: 'metric', // metric | imperial
    clock: '12h',    // 12h | 24h
    timezone: 'local', // 'local' (browser locale) or IANA zone (e.g. 'UTC', 'Asia/Kuala_Lumpur')
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
    stormDrainScore: 0.5,
    tideMode: 'live', // live | spring-high | off
    compute: 'auto', // auto | server
  },
  stormDrainGrid: null,
  liveInterventions: [],
  liveBasinId: null,
  liveOverlays: new Map(),
  catalogBoundaries: [],
  hoveredBoundary: null,
  activeBoundaryName: null,
  lastFramesBuf: null,
  userOverrideCutoff: false,
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

const TZ_FRIENDLY_SHORT = {
  'Asia/Kuala_Lumpur': 'MYT',
  'Asia/Singapore': 'SGT',
  'Asia/Jakarta': 'WIB',
  'Asia/Bangkok': 'ICT',
  'Asia/Manila': 'PHT',
  'Asia/Tokyo': 'JST',
  'Asia/Kolkata': 'IST',
  'UTC': 'UTC',
};

function getBrowserTimeZone() {
  try {
    return Intl.DateTimeFormat().resolvedOptions().timeZone || 'UTC';
  } catch (_) {
    return 'UTC';
  }
}

function getActiveTimeZone() {
  const pref = state.displayPrefs.timezone;
  if (pref && pref !== 'local') return pref;
  return getBrowserTimeZone();
}

const tzFmtCache = new Map();
function getTzFormatter(tz) {
  let fmt = tzFmtCache.get(tz);
  if (fmt) return fmt;
  try {
    fmt = new Intl.DateTimeFormat('en-US', {
      timeZone: tz,
      year: 'numeric',
      month: '2-digit',
      day: '2-digit',
      weekday: 'short',
      hour: '2-digit',
      minute: '2-digit',
      second: '2-digit',
      hourCycle: 'h23',
      timeZoneName: 'short',
    });
  } catch (_) {
    fmt = tzFmtCache.get('UTC') || new Intl.DateTimeFormat('en-US', {
      timeZone: 'UTC',
      year: 'numeric',
      month: '2-digit',
      day: '2-digit',
      weekday: 'short',
      hour: '2-digit',
      minute: '2-digit',
      second: '2-digit',
      hourCycle: 'h23',
      timeZoneName: 'short',
    });
  }
  tzFmtCache.set(tz, fmt);
  return fmt;
}

function getTimePartsInTz(epochMs, tzOverride) {
  const tz = tzOverride || getActiveTimeZone();
  const d = new Date(epochMs);
  const parts = getTzFormatter(tz).formatToParts(d);
  const mapParts = {};
  for (const p of parts) mapParts[p.type] = p.value;
  const h24 = parseInt(mapParts.hour, 10) % 24;
  const mi = parseInt(mapParts.minute, 10) || 0;
  const sec = parseInt(mapParts.second, 10) || 0;
  const tzShort = TZ_FRIENDLY_SHORT[tz] || mapParts.timeZoneName || 'UTC';
  return {
    year: mapParts.year,
    month: mapParts.month,
    dayNum: parseInt(mapParts.day, 10) || 1,
    day: mapParts.day,
    weekday: mapParts.weekday,
    h24,
    mi,
    sec,
    dayKey: `${mapParts.year}-${mapParts.month}-${mapParts.day}`,
    tzShort,
    tz,
  };
}

function fmtClockTime(epochMs, tzLabel) {
  const p = getTimePartsInTz(epochMs);
  const label = tzLabel || p.tzShort;
  const mm = String(p.mi).padStart(2, '0');
  if (state.displayPrefs.clock === '24h') {
    return `${p.weekday} ${p.dayNum} · ${String(p.h24).padStart(2, '0')}:${mm} ${label}`;
  }
  const ap = p.h24 >= 12 ? 'PM' : 'AM';
  const h12 = p.h24 % 12 === 0 ? 12 : p.h24 % 12;
  return `${p.weekday} ${p.dayNum} · ${h12}:${mm} ${ap} ${label}`;
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

function findFloodInsertBeforeId() {
  if (!map) return undefined;
  const style = map.getStyle();
  if (!style || !style.layers) return undefined;
  if (state.basemap === 'sat') return 'sat-labels';
  // Place flood water beneath vector buildings, roads, highways, and labels
  // so street geometry and structures stay crisp when zoomed in (< 300 m).
  const struct = style.layers.find((l) =>
    !l.id.startsWith('arus-')
    && /^(building|tunnel|road|highway|bridge|railway|aeroway)/i.test(l.id),
  );
  if (struct) return struct.id;
  return findFirstLabelLayerId();
}

function floodOpacityExpr(base) {
  const hi = Math.max(0.1, Math.min(1.0, Number.isFinite(base) ? base : 0.95));
  const lo = Math.min(hi, Math.max(0.45, hi * 0.58));
  return ['interpolate', ['linear'], ['zoom'], 11, hi, 13.5, lo];
}

function pushpinSvg(headColor = '#ef4444', w = 26, h = 38) {
  return `<svg width="${w}" height="${h}" viewBox="0 0 28 42" xmlns="http://www.w3.org/2000/svg" aria-hidden="true">`
    + '<polygon points="12.6,18 15.4,18 14.6,37 14,41.5 13.4,37" fill="#64748b"/>'
    + '<polygon points="13.2,18 14.3,18 14.0,39 13.6,37" fill="#94a3b8"/>'
    + `<circle cx="14" cy="11" r="9.5" fill="${headColor}"/>`
    + '<circle cx="11.2" cy="7.6" r="2.3" fill="#ffffff" opacity="0.92"/>'
    + '<circle cx="8.6" cy="11.2" r="1.15" fill="#ffffff" opacity="0.88"/>'
    + '</svg>';
}

function setPinDropMode(on) {
  state.pinDropMode = Boolean(on);
  document.body.classList.toggle('pin-drop-mode', state.pinDropMode);
  const btn = document.querySelector('.arus-pin-ctrl-btn');
  if (btn) btn.classList.toggle('active', state.pinDropMode);
}

function initMap() {
  map = new maplibregl.Map({
    container: 'map',
    style: getBasemapStyle(state.basemap),
    center: [state.center.lon, state.center.lat],
    zoom: Math.min(state.initZoom, state.profile === 'public' ? 15.5 : 17.5),
    maxZoom: state.profile === 'public' ? 15.5 : 17.5,
    pitch: state.initPitch,
    bearing: state.initBearing,
    antialias: true,
    preserveDrawingBuffer: true,
  });

class PinDropControl {
  onAdd(mapInstance) {
    this._map = mapInstance;
    const container = document.createElement('div');
    container.className = 'maplibregl-ctrl arus-pin-ctrl';
    const btn = document.createElement('button');
    btn.type = 'button';
    btn.className = 'arus-pin-ctrl-btn';
    btn.title = 'Drag pin onto map (or click) to check water depth';
    btn.setAttribute('aria-label', 'Drag pin onto map to check water depth');
    btn.innerHTML = pushpinSvg('#ef4444', 26, 38);

    btn.addEventListener('pointerdown', (ev) => {
      if (ev.button !== 0 || window.innerWidth <= 768) return;
      ev.preventDefault();
      ev.stopPropagation();
      const startX = ev.clientX;
      const startY = ev.clientY;
      let dragging = false;
      let ghost = null;

      const onMove = (me) => {
        const dx = me.clientX - startX;
        const dy = me.clientY - startY;
        if (!dragging && Math.hypot(dx, dy) > 5) {
          dragging = true;
          setPinDropMode(false);
          ghost = document.createElement('div');
          ghost.className = 'arus-pin-ghost';
          ghost.innerHTML = pushpinSvg('#ef4444', 30, 44);
          document.body.appendChild(ghost);
        }
        if (ghost) {
          ghost.style.left = `${me.clientX}px`;
          ghost.style.top = `${me.clientY}px`;
        }
      };

      const cleanupDrag = () => {
        window.removeEventListener('pointermove', onMove);
        window.removeEventListener('pointerup', onUp);
        window.removeEventListener('pointercancel', cleanupDrag);
        if (ghost) {
          ghost.remove();
          ghost = null;
        }
      };

      const onUp = (ue) => {
        cleanupDrag();
        if (dragging) {
          const rect = this._map.getContainer().getBoundingClientRect();
          if (
            ue.clientX >= rect.left && ue.clientX <= rect.right
            && ue.clientY >= rect.top && ue.clientY <= rect.bottom
          ) {
            const lngLat = this._map.unproject([ue.clientX - rect.left, ue.clientY - rect.top]);
            dropLivePinAt(lngLat.lng, lngLat.lat);
          }
        } else {
          setPinDropMode(!state.pinDropMode);
        }
      };

      window.addEventListener('pointermove', onMove);
      window.addEventListener('pointerup', onUp);
      window.addEventListener('pointercancel', cleanupDrag);
    });

    container.appendChild(btn);
    this._container = container;
    return container;
  }

  onRemove() {
    if (this._container && this._container.parentNode) {
      this._container.parentNode.removeChild(this._container);
    }
    this._map = null;
  }
}

  map.addControl(new maplibregl.ScaleControl({ maxWidth: 110, unit: 'metric' }), 'bottom-left');
  map.addControl(new PinDropControl(), 'top-right');
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
      loadInitialLiveOrSim();
    }
  });

  map.on('load', () => {
    if (!layersReady) ensureMapLayers();
    renderIncidentMarkers();
    // On narrow portrait mobile screens, fit full configured country/alert bounds so the map
    // doesn't open in empty sea between Peninsula and Borneo.
    if (window.innerWidth <= 768 && state.initZoom <= 6.2 && !state.fixedView) {
      const ab = state.serverConfig?.alert_bounds;
      if (Array.isArray(ab) && ab.length === 4) {
        map.fitBounds([[ab[0], ab[1]], [ab[2], ab[3]]], {
          padding: { top: 76, bottom: 92, left: 10, right: 10 },
          duration: 0,
        });
      }
    }
    if (state.grid) {
      updateDomainOutline(state.grid.metadata.wgs84_bounds);
      renderFloodCanvas(true);
    }
  });

  setTimeout(() => {
    if (!initialSimTriggered) {
      initialSimTriggered = true;
      loadInitialLiveOrSim();
    }
  }, 2400);

  map.on('mousemove', (e) => handleHover(e.lngLat.lng, e.lngLat.lat));
  map.on('click', (e) => handleMapClick(e.lngLat.lng, e.lngLat.lat));
  map.on('moveend', () => {
    renderSecondaryLiveOverlays(false);
    updateFrameTelemetry();
    updateLiveMetrics();
  });
  map.on('zoomend', () => updateBeaconsForZoom());
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
  if (!map || !map.getStyle() || map.getSource('arus-particles')) return;
  const empty = { type: 'FeatureCollection', features: [] };
  const beforeLabels = findFirstLabelLayerId();
  const isDark = state.basemap !== 'topo';

  const isPres = document.documentElement.dataset.presentation === 'true';
  if (!map.getSource('arus-terrarium-dem')) {
    // Separate hillshade and 3D terrain DEM sources capped at maxzoom 12 to eliminate
    // MapLibre shared-source warnings and excessive aborted high-zoom S3 tile fetches.
    map.addSource('arus-terrarium-dem', {
      type: 'raster-dem',
      tiles: ['https://s3.amazonaws.com/elevation-tiles-prod/terrarium/{z}/{x}/{y}.png'],
      encoding: 'terrarium',
      tileSize: 256,
      maxzoom: 12,
    });
    map.addSource('arus-terrarium-hillshade', {
      type: 'raster-dem',
      tiles: ['https://s3.amazonaws.com/elevation-tiles-prod/terrarium/{z}/{x}/{y}.png'],
      encoding: 'terrarium',
      tileSize: 256,
      maxzoom: 12,
    });
    const beforeFlood = findFloodInsertBeforeId() || beforeLabels;
    map.addLayer(
      {
        id: 'arus-hillshade',
        type: 'hillshade',
        source: 'arus-terrarium-hillshade',
        paint: {
          'hillshade-exaggeration': isDark ? 0.25 : 0.32,
          'hillshade-shadow-color': isDark ? '#020617' : '#334155',
          'hillshade-highlight-color': isDark ? '#334155' : '#ffffff',
        },
      },
      beforeFlood,
    );
    // Keep crisp 2D hillshade relief in Live mode (avoids MapLibre v5 3D terrain-tile
    // seam clipping on wide regional ImageSource overlays, e.g. z=7 lat 2.811°N).
    if (state.is3D && !isPres && !state.boundaryGeoJson && state.profile !== 'public') {
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

  // Place hover boundary ABOVE region mask so adjacent states stay bright on hover
  map.addSource('arus-hover-boundary', { type: 'geojson', data: empty });
  map.addLayer(
    {
      id: 'arus-hover-fill',
      type: 'fill',
      source: 'arus-hover-boundary',
      paint: { 'fill-color': '#0284c7', 'fill-opacity': 0.12 },
    },
    beforeLabels,
  );
  map.addLayer(
    {
      id: 'arus-hover-line',
      type: 'line',
      source: 'arus-hover-boundary',
      paint: {
        'line-color': '#0284c7',
        'line-width': 2.0,
        'line-opacity': 0.72,
        'line-dasharray': [3, 2],
      },
    },
    beforeLabels,
  );

  // Emerald river channels above flood water but beneath highway bridges and buildings
  const style = map.getStyle();
  const vecSourceId = style && style.sources
    ? Object.keys(style.sources).find((k) => style.sources[k].type === 'vector')
    : null;
  if (vecSourceId) {
    const riverAnchor = findFloodInsertBeforeId() || beforeLabels;
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
      riverAnchor,
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
          'line-width': ['interpolate', ['linear'], ['zoom'], 6, 1.1, 8, 1.4, 12, 1.8, 15, 2.4],
          'line-opacity': state.displayPrefs.riverOpacity ?? 0.78,
        },
        layout: { 'line-cap': 'round', 'line-join': 'round' },
      },
      riverAnchor,
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
  applyPendingFocusIfReady();
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

let pendingFocusApplied = false;
function applyPendingFocusIfReady() {
  if (pendingFocusApplied || !map || !layersReady || !state.catalogBoundaries.length || state.boundaryGeoJson) return;
  const focusParam = new URLSearchParams(window.location.search).get('focus');
  if (!focusParam) return;
  const target = focusParam.trim().toLowerCase();
  const hit = state.catalogBoundaries.find((b) =>
    b.name.toLowerCase() === target
    || (b.name === 'Malacca' && target === 'melaka'),
  );
  if (hit) {
    pendingFocusApplied = true;
    const qsNow = new URLSearchParams(window.location.search);
    const keepCamera = qsNow.has('lat') && qsNow.has('lon');
    activateCatalogBoundary(hit, { fitView: !keepCamera });
  }
}

function clearBoundaryFocus() {
  if (!state.boundaryGeoJson) return;
  state.boundaryVersion = (state.boundaryVersion || 0) + 1;
  state.boundaryGeoJson = null;
  state.boundaryGrid = null;
  state.boundaryCellMask = null;
  state.boundaryBounds = null;
  state.activeBoundaryName = null;
  const empty = { type: 'FeatureCollection', features: [] };
  if (map && layersReady) {
    const bSrc = map.getSource('arus-region-boundary');
    const mSrc = map.getSource('arus-region-mask');
    const hSrc = map.getSource('arus-hover-boundary');
    if (bSrc) bSrc.setData(empty);
    if (mSrc) mSrc.setData(empty);
    if (hSrc) hSrc.setData(empty);
  }
  state.hoveredBoundary = null;
  $('focus-chip')?.classList.add('hidden');
  renderFloodCanvas(true);
  updateLiveMetrics();
}

function pointInRing(lon, lat, ring) {
  let inside = false;
  for (let i = 0, j = ring.length - 1; i < ring.length; j = i++) {
    const xi = ring[i][0], yi = ring[i][1];
    const xj = ring[j][0], yj = ring[j][1];
    const intersect = ((yi > lat) !== (yj > lat)) && (lon < ((xj - xi) * (lat - yi)) / ((yj - yi) || 1e-12) + xi);
    if (intersect) inside = !inside;
  }
  return inside;
}

function pointInGeoGeom(lon, lat, geom) {
  if (!geom) return false;
  if (geom.type === 'Polygon') {
    if (!pointInRing(lon, lat, geom.coordinates[0])) return false;
    for (let k = 1; k < geom.coordinates.length; k++) {
      if (pointInRing(lon, lat, geom.coordinates[k])) return false;
    }
    return true;
  }
  if (geom.type === 'MultiPolygon') {
    for (const poly of geom.coordinates) {
      if (!poly?.[0]) continue;
      if (pointInRing(lon, lat, poly[0])) {
        let inHole = false;
        for (let k = 1; k < poly.length; k++) {
          if (pointInRing(lon, lat, poly[k])) { inHole = true; break; }
        }
        if (!inHole) return true;
      }
    }
  }
  return false;
}

async function fetchCatalogBoundaries() {
  try {
    const resp = await fetch(apiUrl('api/v1/boundaries'));
    if (!resp.ok) return;
    const fc = await resp.json();
    const feats = Array.isArray(fc?.features) ? fc.features : [];
    const items = [];
    for (const f of feats) {
      const geom = f?.geometry;
      if (!geom || (geom.type !== 'Polygon' && geom.type !== 'MultiPolygon')) continue;
      const name = String(f.properties?.shapeName || f.properties?.name || '').trim();
      if (!name) continue;
      let minLon = Infinity, minLat = Infinity, maxLon = -Infinity, maxLat = -Infinity;
      const scan = (ring) => {
        for (const pt of ring) {
          if (pt[0] < minLon) minLon = pt[0];
          if (pt[0] > maxLon) maxLon = pt[0];
          if (pt[1] < minLat) minLat = pt[1];
          if (pt[1] > maxLat) maxLat = pt[1];
        }
      };
      if (geom.type === 'Polygon') scan(geom.coordinates[0]);
      else for (const poly of geom.coordinates) if (poly?.[0]) scan(poly[0]);
      if (!Number.isFinite(minLon)) continue;
      const area = (maxLon - minLon) * (maxLat - minLat);
      items.push({ name, geom, bbox: [minLon, minLat, maxLon, maxLat], area });
    }
    // Smallest enclosing boundary wins (e.g. Putrajaya < Kuala Lumpur < Selangor)
    items.sort((a, b) => a.area - b.area);
    state.catalogBoundaries = items;
    applyPendingFocusIfReady();
  } catch (_) {}
}

function findEnclosingCatalogBoundary(lon, lat) {
  for (const b of state.catalogBoundaries) {
    const [minLon, minLat, maxLon, maxLat] = b.bbox;
    if (lon < minLon || lon > maxLon || lat < minLat || lat > maxLat) continue;
    if (pointInGeoGeom(lon, lat, b.geom)) return b;
  }
  return null;
}

function updateHoverBoundary(lon, lat) {
  if (!map || !layersReady || state.profile !== 'public' || !state.catalogBoundaries.length) return;
  const hSrc = map.getSource('arus-hover-boundary');
  if (!hSrc) return;
  const hit = findEnclosingCatalogBoundary(lon, lat);
  // Highlight when hovering a DIFFERENT boundary while a focus is active, OR any state when zoomed out (< 9.5)
  const shouldHighlight = hit && (
    (state.boundaryGeoJson && hit.name.toLowerCase() !== String(state.activeBoundaryName || '').toLowerCase())
    || (!state.boundaryGeoJson && map.getZoom() < 9.5)
  );
  if (shouldHighlight) {
    if (state.hoveredBoundary?.name !== hit.name) {
      state.hoveredBoundary = hit;
      hSrc.setData({ type: 'Feature', geometry: hit.geom, properties: { name: hit.name } });
      map.getCanvas().style.cursor = 'pointer';
    }
  } else if (state.hoveredBoundary) {
    state.hoveredBoundary = null;
    hSrc.setData({ type: 'FeatureCollection', features: [] });
    map.getCanvas().style.cursor = '';
  }
}

async function activateCatalogBoundary(b, opts = {}) {
  if (!b || !b.geom) return;
  const fitView = opts.fitView !== false;
  state.hoveredBoundary = null;
  if (map && layersReady) {
    const hSrc = map.getSource('arus-hover-boundary');
    if (hSrc) hSrc.setData({ type: 'FeatureCollection', features: [] });
    map.getCanvas().style.cursor = '';
  }
  state.activeBoundaryName = b.name;
  // Sync preset dropdown if a matching preset exists
  const matchedPreset = PRESETS.find((p) =>
    p.name.toLowerCase() === b.name.toLowerCase()
    || (b.name === 'Malacca' && p.name.toLowerCase() === 'melaka'),
  );
  if (matchedPreset && $('preset-select')) $('preset-select').value = matchedPreset.id;
  clearLivePin();
  applyBoundaryGeometry(b.geom, false, { fitView });
  if (matchedPreset?.liveBasin) {
    await tryLoadPrecomputedLive(matchedPreset.liveBasin);
  }
  renderFloodCanvas(true);
  updateLiveMetrics();
}

function applyRegionBoundary() {
  if (!map || !layersReady || !state.boundaryGeoJson) return;
  const geom = state.boundaryGeoJson;
  // Compute tight bounding box for pan-out auto-clear & show Focus chip
  let bMinLon = Infinity, bMinLat = Infinity, bMaxLon = -Infinity, bMaxLat = -Infinity;
  const scanRing = (ring) => {
    for (const pt of ring) {
      if (pt[0] < bMinLon) bMinLon = pt[0];
      if (pt[0] > bMaxLon) bMaxLon = pt[0];
      if (pt[1] < bMinLat) bMinLat = pt[1];
      if (pt[1] > bMaxLat) bMaxLat = pt[1];
    }
  };
  if (geom.type === 'Polygon' && Array.isArray(geom.coordinates?.[0])) {
    scanRing(geom.coordinates[0]);
  } else if (geom.type === 'MultiPolygon' && Array.isArray(geom.coordinates)) {
    for (const poly of geom.coordinates) {
      if (Array.isArray(poly?.[0])) scanRing(poly[0]);
    }
  }
  if (Number.isFinite(bMinLon)) {
    state.boundaryBounds = [bMinLon, bMinLat, bMaxLon, bMaxLat];
  }
  const focusLabel = state.activeBoundaryName
    || $('preset-select')?.selectedOptions?.[0]?.textContent?.trim()
    || $('coord-search')?.value?.trim()
    || 'Selected Area';
  state.activeBoundaryName = focusLabel;
  if ($('focus-chip-text')) $('focus-chip-text').textContent = `Focus: ${focusLabel}`;
  $('focus-chip')?.classList.remove('hidden');
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

async function applyBoundaryGeometry(geom, runSim = true, opts = {}) {
  if (!geom || (geom.type !== 'Polygon' && geom.type !== 'MultiPolygon')) return false;
  state.boundaryVersion = (state.boundaryVersion || 0) + 1;
  state.boundaryGeoJson = geom;
  const [minLon, minLat, maxLon, maxLat] = computeGeomBbox(geom);
  const midLat = (minLat + maxLat) * 0.5;
  const midLon = (minLon + maxLon) * 0.5;
  const mPerDegLat = 111320.0;
  const mPerDegLon = Math.max(10000.0, 111320.0 * Math.cos((midLat * Math.PI) / 180));
  // Buffer the physics mesh by +28% (+14% per side) so cross-border upstream catchments
  // (e.g. Sg. Bernam headwaters in Perak, Sg. Langat in Negeri Sembilan) route naturally
  // into the selected administrative region while visual output stays clipped to the border.
  const spanXm = (maxLon - minLon) * mPerDegLon * 1.28;
  const spanYm = (maxLat - minLat) * mPerDegLat * 1.28;
  const aspect = Math.max(0.4, Math.min(2.5, spanXm / Math.max(1, spanYm)));
  const rows = Math.max(32, Math.min(72, Math.round(Math.sqrt(3600 / aspect))));
  const cols = Math.max(32, Math.min(72, Math.round(rows * aspect)));
  const dxM = Math.max(45.0, Math.min(2400.0, Math.max(spanXm / cols, spanYm / rows)));
  state.center = { lat: midLat, lon: midLon };
  state.boundaryGrid = { cols, rows, dxM };
  state.boundaryNeedsLandFit = dxM >= 300;
  state.cachedDem = null;
  if (map) { try { map.setTerrain(null); } catch (_) {} }
  clearLivePin();
  applyRegionBoundary();
  if (map && opts.fitView !== false) {
    map.stop();
    const vw = window.innerWidth || 1280;
    const vh = window.innerHeight || 800;
    map.fitBounds([[minLon, minLat], [maxLon, maxLat]], {
      padding: {
        top: Math.min(64, Math.round(vh * 0.12)),
        bottom: Math.min(78, Math.round(vh * 0.14)),
        left: Math.min(310, Math.round(vw * 0.24)),
        right: Math.min(235, Math.round(vw * 0.20)),
      },
      pitch: 0,
      bearing: 0,
      duration: 0,
    });
  }
  if (runSim) {
    await runSimulation(false);
  } else {
    rebuildBoundaryCellMask();
    renderFloodCanvas(true);
    updateLiveMetrics();
  }
  return true;
}

const NOMINATIM_CACHE_KEY = 'arus.nominatim.cache.v1';
const NOMINATIM_TTL_MS = 7 * 24 * 3600 * 1000;
const NOMINATIM_MAX_ITEM_BYTES = 32 * 1024;
const NOMINATIM_MAX_TOTAL_BYTES = 512 * 1024;
const nominatimMemCache = new Map();

function readNominatimCache(cacheKey) {
  const mem = nominatimMemCache.get(cacheKey);
  if (mem && Date.now() - mem.ts < NOMINATIM_TTL_MS) return mem.val;
  try {
    const store = JSON.parse(window.localStorage.getItem(NOMINATIM_CACHE_KEY) || '{}');
    const entry = store[cacheKey];
    if (entry && Date.now() - entry.ts < NOMINATIM_TTL_MS) {
      nominatimMemCache.set(cacheKey, entry);
      return entry.val;
    }
  } catch (_) {}
  return null;
}

function writeNominatimCache(cacheKey, val) {
  const entry = { ts: Date.now(), val };
  nominatimMemCache.set(cacheKey, entry);
  try {
    const encoded = JSON.stringify(entry);
    if (encoded.length > NOMINATIM_MAX_ITEM_BYTES) return;
    const store = JSON.parse(window.localStorage.getItem(NOMINATIM_CACHE_KEY) || '{}');
    store[cacheKey] = entry;
    let keys = Object.keys(store);
    while (JSON.stringify(store).length > NOMINATIM_MAX_TOTAL_BYTES && keys.length > 1) {
      keys.sort((a, b) => (store[a]?.ts || 0) - (store[b]?.ts || 0));
      delete store[keys.shift()];
    }
    window.localStorage.setItem(NOMINATIM_CACHE_KEY, JSON.stringify(store));
  } catch (_) {}
}

async function setCustomBoundary(lon, lat, radiusKm = 5.0, shape = 'hex', runSim = false) {
  const geom = buildRadiusPolygon(lon, lat, radiusKm, shape);
  return applyBoundaryGeometry(geom, runSim);
}

async function loadBoundaryByQuery(query, opts = {}) {
  if (!query) return false;
  // Prefer instant land-only server catalog boundaries (no maritime sea wedge, zero Nominatim delay)
  const qNorm = query.split(',')[0].trim().toLowerCase();
  const catHit = state.catalogBoundaries.find((b) => {
    const bn = b.name.toLowerCase();
    return bn === qNorm || (qNorm === 'melaka' && bn === 'malacca') || (qNorm === 'malacca' && bn === 'melaka');
  });
  if (catHit) {
    state.activeBoundaryName = catHit.name;
    return applyBoundaryGeometry(catHit.geom, opts.runSim !== false);
  }
  state.activeBoundaryName = query.split(',')[0].trim();
  const cacheKey = `poly:${query.toLowerCase().trim()}`;
  try {
    let hit = readNominatimCache(cacheKey);
    if (!hit) {
      const resp = await fetch(
        `https://nominatim.openstreetmap.org/search?format=json&polygon_geojson=1&polygon_threshold=0.005&limit=1&q=${encodeURIComponent(query)}`,
      );
      const hits = await resp.json();
      hit = hits?.[0] || null;
      if (hit) writeNominatimCache(cacheKey, hit);
    }
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

// Multi-hue cool-water depth ramp (no red): pale aqua -> cyan -> royal blue -> violet -> deep purple
function colorForDepth(h) {
  const cut = state.displayPrefs.depthCutoffM || 0.06;
  if (h < cut) return null;
  const fade = Math.min(1, (h - cut) / 0.04);
  const stops = [
    [cut,   165, 243, 252, 145], // #a5f3fc pale aqua (0.06 m shallow)
    [0.25,   34, 211, 238, 190], // #22d3ee vivid cyan (0.25 m ankle)
    [0.50,   37,  99, 235, 224], // #2563eb royal blue (0.50 m knee)
    [0.85,  124,  58, 237, 238], // #7c3aed vivid violet (0.85 m waist)
    [1.20,   76,  29, 149, 248], // #4c1d95 deep purple (1.20 m+ deep)
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
  if (Math.abs(d) < 0.015) return null;
  const mag = Math.min(1, Math.abs(d) / 0.30);
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

  const landMask = (
    !state.uploadedDem
    && state.cachedDem
    && state.cachedDem.landMask
    && state.cachedDem.maskW === W
    && state.cachedDem.maskH === H
  ) ? state.cachedDem.landMask : null;

  for (let py = 0; py < H; py++) {
    const fy = (1 - (py + 0.5) / H) * (rows - 1);
    const rNearest = Math.min(rows - 1, Math.max(0, Math.round(fy)));
    for (let px = 0; px < W; px++) {
      const fx = ((px + 0.5) / W) * (cols - 1);
      const cNearest = Math.min(cols - 1, Math.max(0, Math.round(fx)));
      const out = (py * W + px) * 4;

      if (!meta.active_mask[rNearest * cols + cNearest]) {
        continue;
      }
      if (landMask && !landMask[py * W + px]) {
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
    if (state.frameRasterCache.size >= 512) {
      const oldestKey = state.frameRasterCache.keys().next().value;
      if (oldestKey !== undefined) state.frameRasterCache.delete(oldestKey);
    }
    state.frameRasterCache.set(lastRasterKey, url);
  }
  if (map.getSource('arus-water-img')) {
    map.getSource('arus-water-img').updateImage({ url, coordinates: coords });
  } else {
    map.addSource('arus-water-img', { type: 'image', url, coordinates: coords });
    const anchor = map.getLayer('arus-river-halo')
      ? 'arus-river-halo'
      : (findFloodInsertBeforeId() || 'arus-particles-line');
    map.addLayer(
      {
        id: 'arus-water-smooth',
        type: 'raster',
        source: 'arus-water-img',
        paint: { 'raster-opacity': floodOpacityExpr(state.displayPrefs.floodOpacity), 'raster-resampling': 'linear', 'raster-fade-duration': 0 },
      },
      anchor,
    );
  }
}

function renderSecondaryLiveOverlays(forceRaster) {
  if (!map || !layersReady) return;
  const showSecondary = state.profile === 'public' && !state.boundaryGeoJson && state.displayPrefs.showFlood;
  const anchor = map.getLayer('arus-river-halo') ? 'arus-river-halo' : (findFloodInsertBeforeId() || (map.getLayer('arus-particles-line') ? 'arus-particles-line' : undefined));
  const vb = map.getBounds();
  const vMinLon = vb.getWest();
  const vMaxLon = vb.getEast();
  const vMinLat = vb.getSouth();
  const vMaxLat = vb.getNorth();
  for (const [basinId, ov] of state.liveOverlays.entries()) {
    const srcId = `arus-live-img-${basinId}`;
    const layerId = `arus-live-layer-${basinId}`;
    if (!showSecondary || basinId === state.liveBasinId) {
      if (map.getLayer(layerId)) map.setLayoutProperty(layerId, 'visibility', 'none');
      continue;
    }
    const meta = ov.grid.metadata;
    const [bMinLon, bMinLat, bMaxLon, bMaxLat] = meta.wgs84_bounds;
    // Skip expensive rasterization for basins completely outside current viewport
    if (bMaxLon < vMinLon || bMinLon > vMaxLon || bMaxLat < vMinLat || bMinLat > vMaxLat) {
      continue;
    }
    const env = ov.grid.envelope;
    const fIdx = Math.min(state.frameIdx, ov.frames.length - 1);
    const frame = ov.frames[fIdx];
    const key = `sec:${basinId}:${ov.grid.run_id || ''}:${fIdx}:${state.layer}:${state.envelopeMode}:${state.displayPrefs.depthCutoffM}`;
    if (forceRaster || ov.lastKey !== key || !map.getSource(srcId)) {
      ov.lastKey = key;
      let url = state.frameRasterCache.get(key);
      if (!url) {
        const W = rasterCanvas.width;
        const H = rasterCanvas.height;
        const img = rasterCtx.createImageData(W, H);
        const data = img.data;
        const cols = meta.cols;
        const rows = meta.rows;
        let speedArr = null;
        if (state.layer === 'velocity' || state.layer === 'hazard') {
          speedArr = new Float32Array(cols * rows);
          for (let i = 0; i < cols * rows; i++) speedArr[i] = Math.hypot(frame.vx[i], frame.vy[i]);
        }
        for (let py = 0; py < H; py++) {
          const fy = (1 - (py + 0.5) / H) * (rows - 1);
          const rN = Math.min(rows - 1, Math.max(0, Math.round(fy)));
          for (let px = 0; px < W; px++) {
            const fx = ((px + 0.5) / W) * (cols - 1);
            const cN = Math.min(cols - 1, Math.max(0, Math.round(fx)));
            if (!meta.active_mask[rN * cols + cN]) continue;
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
              const onset = env.onset_time_s[rN * cols + cN];
              const maxT = ov.frames.length ? (ov.frames[ov.frames.length - 1].time_s || 360) : 360;
              rgba = colorForArrival(onset, maxT);
            }
            if (rgba) {
              const out = (py * W + px) * 4;
              data[out] = rgba[0];
              data[out + 1] = rgba[1];
              data[out + 2] = rgba[2];
              data[out + 3] = Math.round(rgba[3]);
            }
          }
        }
        rasterCtx.putImageData(img, 0, 0);
        url = rasterCanvas.toDataURL('image/png');
        if (state.frameRasterCache.size >= 512) {
          const oldestKey = state.frameRasterCache.keys().next().value;
          if (oldestKey !== undefined) state.frameRasterCache.delete(oldestKey);
        }
        state.frameRasterCache.set(key, url);
      }
      const coords = [[bMinLon, bMaxLat], [bMaxLon, bMaxLat], [bMaxLon, bMinLat], [bMinLon, bMinLat]];
      if (map.getSource(srcId)) {
        try { map.getSource(srcId).updateImage({ url, coordinates: coords }); } catch (_) {}
      } else {
        try {
          map.addSource(srcId, { type: 'image', url, coordinates: coords });
          map.addLayer(
            {
              id: layerId,
              type: 'raster',
              source: srcId,
              paint: { 'raster-opacity': floodOpacityExpr(state.displayPrefs.floodOpacity), 'raster-resampling': 'linear', 'raster-fade-duration': 0 },
            },
            anchor,
          );
        } catch (_) {}
      }
    }
    if (map.getLayer(layerId) && map.getLayoutProperty(layerId, 'visibility') !== 'visible') {
      map.setLayoutProperty(layerId, 'visibility', 'visible');
    }
  }
}

function renderFloodCanvas(forceRaster) {
  if (!state.grid || !state.frames.length || !layersReady) return;
  renderSecondaryLiveOverlays(forceRaster);
  if (state.profile === 'public') refreshAlertsAndBeacons();
  const meta = state.grid.metadata;
  const env = state.grid.envelope;
  const cols = meta.cols;
  const rows = meta.rows;
  const [minLon, minLat, maxLon, maxLat] = meta.wgs84_bounds;
  const dLon = (maxLon - minLon) / cols;
  const dLat = (maxLat - minLat) / rows;
  const frame = state.frames[Math.min(state.frameIdx, state.frames.length - 1)];

  const rasterKey = `${state.runId}:${state.boundaryVersion || 0}:${state.frameIdx}:${state.layer}:${state.envelopeMode}:${state.displayPrefs.depthCutoffM}`;
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
    const activeIvs = state.interventions.length > 0
      ? state.interventions
      : (state.profile === 'public' ? (state.liveInterventions || []) : []);
    const ivFeatures = activeIvs.map((iv) => {
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
  updateHoverBoundary(lon, lat);
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

function addInterventionCell(col, row, delta_m, reason) {
  if (!state.grid) return;
  const { cols, rows } = state.grid.metadata;
  if (col < 0 || row < 0 || col >= cols || row >= rows) return;
  const existing = state.interventions.findIndex((iv) => iv.col === col && iv.row === row);
  if (existing >= 0) {
    state.interventions[existing] = { col, row, delta_m, reason };
  } else {
    state.interventions.push({ col, row, delta_m, reason });
  }
}

function rasterizeBundSegment(c0, r0, c1, r1, delta_m, reason) {
  let x0 = c0, y0 = r0;
  const dx = Math.abs(c1 - c0), dy = Math.abs(r1 - r0);
  const sx = c0 < c1 ? 1 : -1, sy = r0 < r1 ? 1 : -1;
  let err = dx - dy;
  while (true) {
    addInterventionCell(x0, y0, delta_m, reason);
    if (x0 + 1 < state.grid.metadata.cols && dx < dy) addInterventionCell(x0 + 1, y0, delta_m, reason);
    else if (y0 + 1 < state.grid.metadata.rows && dy <= dx) addInterventionCell(x0, y0 + 1, delta_m, reason);
    if (x0 === c1 && y0 === r1) break;
    const e2 = 2 * err;
    if (e2 > -dy) { err -= dy; x0 += sx; }
    if (e2 < dx) { err += dx; y0 += sy; }
  }
}

function addBundLine(points, heightM = 2.0, reason = 'riverbank flood bund') {
  if (!state.grid || !Array.isArray(points) || points.length === 0) return;
  const cells = points
    .map(([lat, lon]) => lngLatToCell(lon, lat))
    .filter(Boolean);
  if (cells.length === 0) return;
  const delta = Number.isFinite(heightM) ? heightM : 2.0;
  if (cells.length === 1) {
    addInterventionCell(cells[0].col, cells[0].row, delta, reason);
  } else {
    for (let i = 0; i < cells.length - 1; i++) {
      rasterizeBundSegment(cells[i].col, cells[i].row, cells[i + 1].col, cells[i + 1].row, delta, reason);
    }
  }
  state.lastBundCell = cells[cells.length - 1];
  state.staticVectorsDirty = true;
  updateInterventionBar();
  renderFloodCanvas(false);
}

function breachBund(lat, lon, radiusCells = 2) {
  if (!state.grid || state.interventions.length === 0) return;
  const hit = lngLatToCell(lon, lat);
  if (!hit) return;
  if (state.baselineInterventions.length === 0) {
    state.baselineInterventions = state.interventions.map((iv) => ({ ...iv }));
  }
  const r2 = radiusCells * radiusCells;
  state.interventions = state.interventions.filter((iv) => {
    const dc = iv.col - hit.col;
    const dr = iv.row - hit.row;
    return dc * dc + dr * dr > r2;
  });
  state.staticVectorsDirty = true;
  updateInterventionBar();
  renderFloodCanvas(false);
}

function handleMapClick(lon, lat) {
  if (state.pinDropMode) {
    setPinDropMode(false);
    dropLivePinAt(lon, lat);
    return;
  }
  if (state.profile === 'public' && state.hoveredBoundary) {
    activateCatalogBoundary(state.hoveredBoundary);
    return;
  }
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
    if ($('coord-search')) $('coord-search').value = `${lat.toFixed(4)}, ${lon.toFixed(4)}`;
    setCustomPresetLabel(`Custom · ${lat.toFixed(4)}, ${lon.toFixed(4)}`);
    if (map) map.flyTo({ center: [lon, lat], zoom: Math.max(map.getZoom(), 11.5), duration: 700 });
    updateSimDraftSummary();
    showToast('Basin center updated in draft — click Run Simulation when ready.');
    return;
  }
  if (state.profile === 'public') return;
  if (state.tool === 'floodpin') {
    const depthM = parseFloat($('fp-depth')?.value || '0.5') || 0.5;
    state.observedGauges.push({ lat, lon, depth_m: depthM, radius_cells: 3 });
    updateFloodPinBar();
    showToast(`Added observed ${fmtDepth(depthM, 1)} flood pin (${state.observedGauges.length} total). Click Run Simulation when ready.`);
    return;
  }
  const hit = lngLatToCell(lon, lat);
  if (!hit) {
    if (state.tool === 'bund' || state.tool === 'pond') {
      showToast('Run a baseline simulation first (or click inside the active basin) before placing bunds/ponds.');
    }
    return;
  }

  if (state.tool === 'bund') {
    const heightM = parseFloat($('bund-height')?.value || '2.0') || 2.0;
    const reason = `raised flood bund (+${heightM.toFixed(1)} m)`;
    if (state.lastBundCell) {
      rasterizeBundSegment(state.lastBundCell.col, state.lastBundCell.row, hit.col, hit.row, heightM, reason);
    } else {
      addInterventionCell(hit.col, hit.row, heightM, reason);
    }
    state.lastBundCell = { col: hit.col, row: hit.row };
    state.staticVectorsDirty = true;
    updateInterventionBar();
    renderFloodCanvas(false);
    return;
  }

  if (state.tool === 'pond') {
    const delta = -0.8;
    const reason = 'detention pond';
    for (let dr = 0; dr <= 1; dr++) {
      for (let dc = 0; dc <= 1; dc++) {
        addInterventionCell(hit.col + dc, hit.row + dr, delta, reason);
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

function computeRecessionSummary(idx, thresholdM, framesOverride) {
  const frames = framesOverride || state.frames;
  if (!frames || frames.length < 2) return { label: '—', receded: false, endDepth: 0 };
  const totalSec = Math.max(1, frames[frames.length - 1].time_s || 1);
  let peakIdx = -1;
  let peakVal = 0;
  for (let i = 0; i < frames.length; i++) {
    const d = frames[i].depth[idx] || 0;
    if (d > peakVal) {
      peakVal = d;
      peakIdx = i;
    }
  }
  const endDepth = frames[frames.length - 1].depth[idx] || 0;
  if (peakVal < thresholdM) return { label: 'Dry', receded: true, endDepth };
  for (let i = Math.max(0, peakIdx); i < frames.length; i++) {
    if ((frames[i].depth[idx] || 0) < thresholdM) {
      return {
        label: `Recedes ${fmtSimOffset(frames[i].time_s, totalSec)}`,
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

function lookupPointAcrossBasins(lon, lat) {
  const tryGrid = (grid, frames, basinId) => {
    const meta = grid?.metadata;
    if (!meta || !frames?.length) return null;
    const [minLon, minLat, maxLon, maxLat] = meta.wgs84_bounds;
    if (lon < minLon || lon > maxLon || lat < minLat || lat > maxLat) return null;
    const col = Math.min(meta.cols - 1, Math.max(0, Math.floor(((lon - minLon) / (maxLon - minLon)) * meta.cols)));
    const row = Math.min(meta.rows - 1, Math.max(0, Math.floor(((lat - minLat) / (maxLat - minLat)) * meta.rows)));
    const idx = row * meta.cols + col;
    return { grid, frames, basinId, col, row, idx, active: Boolean(meta.active_mask[idx]) };
  };
  let fallback = null;
  if (state.profile === 'public' && state.liveOverlays.size > 0) {
    for (const [basinId, ov] of state.liveOverlays.entries()) {
      const hit = tryGrid(ov.grid, ov.frames, basinId);
      if (hit && hit.active) return hit;
      if (hit && !fallback) fallback = hit;
    }
  }
  const primary = tryGrid(state.grid, state.frames, state.liveBasinId);
  if (primary && primary.active) return primary;
  return fallback || primary;
}

function dropLivePinAt(lon, lat) {
  if (window.innerWidth <= 768) return;
  state.verdictTarget = { lon, lat };
  renderSiteVerdict(false);
}

function clearLivePin() {
  setPinDropMode(false);
  state.verdictTarget = null;
  if ($('site-verdict')) $('site-verdict').classList.add('hidden');
  if (state.verdictMarker) {
    state.verdictMarker.remove();
    state.verdictMarker = null;
  }
}

function renderSiteVerdict(isFrameTick = false) {
  const card = $('site-verdict');
  if (!card || !state.verdictTarget) return;
  const { lat, lon } = state.verdictTarget;
  const hit = lookupPointAcrossBasins(lon, lat);
  card.classList.remove('hidden');

  const placePinMarker = (color) => {
    if (!map || typeof maplibregl === 'undefined') return;
    if (isFrameTick && state.verdictMarker) return;
    if (state.verdictMarker) state.verdictMarker.remove();
    const el = document.createElement('div');
    el.className = 'arus-dropped-pin';
    el.innerHTML = pushpinSvg(color, 28, 40);
    const m = new maplibregl.Marker({ element: el, anchor: 'bottom', draggable: true }).setLngLat([lon, lat]).addTo(map);
    let lastDragTs = 0;
    m.on('drag', () => {
      const now = performance.now();
      if (now - lastDragTs < 60) return;
      lastDragTs = now;
      const p = m.getLngLat();
      state.verdictTarget = { lon: p.lng, lat: p.lat };
      renderSiteVerdict(true);
    });
    m.on('dragend', () => {
      const p = m.getLngLat();
      state.verdictTarget = { lon: p.lng, lat: p.lat };
      renderSiteVerdict(false);
    });
    state.verdictMarker = m;
  };

  if (!hit || !hit.active) {
    if (!isFrameTick) $('sv-title').textContent = `${lat.toFixed(4)}°, ${lon.toFixed(4)}°`;
    $('sv-badge').className = 'lm-badge low';
    $('sv-badge').textContent = 'NO DATA';
    $('sv-summary').textContent = 'This coordinate lies outside active live flood coverage.';
    $('sv-stats').innerHTML = '';
    placePinMarker('#64748b');
    return;
  }

  const { grid, frames, idx } = hit;
  const env = grid.envelope;
  const ffeM = parseFloat($('sv-ffe-select')?.value || '0') || 0;
  const fIdx = Math.min(state.frameIdx, frames.length - 1);
  const rawNowM = frames[fIdx]?.depth?.[idx] || 0;
  const rawPeakM = env.max_depth_m[idx] || 0;
  const cut = state.displayPrefs.depthCutoffM || 0.08;
  const isNowWet = rawNowM >= cut;
  const isFlooded = rawPeakM >= cut;
  const netM = isFlooded ? Math.max(0, rawPeakM - ffeM) : 0;
  const totalSec = Math.max(1, frames[frames.length - 1].time_s || 1);
  const recInfo = computeRecessionSummary(idx, env.threshold_m || 0.15, frames);

  let lvl = 'low';
  let label = 'DRY';
  let msg = `Below ${Math.round(cut * 100)} cm flood cutoff for your ${fmtDepth(ffeM, 1)} floor. Safe for parked cars and normal access.`;
  if (netM >= 0.30) {
    lvl = 'danger';
    label = 'FLOOD EXPECTED';
    msg = `Reaches ${fmtDepth(netM, 2)} above floor (${fmtDepth(rawPeakM, 2)} ground). Move cars to high ground, do not drive through floodwater, and prepare to evacuate. ${recInfo.receded ? recInfo.label : 'Water persists through forecast end'}.`;
  } else if (netM >= 0.15) {
    lvl = 'moderate';
    label = 'WATCH · MOVE CARS';
    msg = `Reaches ${fmtDepth(netM, 2)} above floor (${fmtDepth(rawPeakM, 2)} ground — car floor height). Move parked vehicles and monitor official alerts. ${recInfo.receded ? recInfo.label : 'Standing water persists'}.`;
  } else if (netM >= cut) {
    lvl = 'moderate';
    label = 'WATCH';
    msg = `Shallow ponding up to ${fmtDepth(netM, 2)} above floor (${fmtDepth(rawPeakM, 2)} ground). Avoid low driveways and monitor official alerts.`;
  }

  $('sv-badge').className = `lm-badge ${lvl}`;
  $('sv-badge').textContent = label;
  $('sv-summary').textContent = msg;
  if (!isFrameTick) {
    $('sv-title').textContent = `${lat.toFixed(4)}°, ${lon.toFixed(4)}°`;
    const revKey = `rev:${lat.toFixed(4)},${lon.toFixed(4)}`;
    const cachedPlace = readNominatimCache(revKey);
    if (cachedPlace) {
      $('sv-title').textContent = cachedPlace;
      $('sv-title').title = `${lat.toFixed(4)}°, ${lon.toFixed(4)}°`;
    } else {
      fetch(`https://nominatim.openstreetmap.org/reverse?format=json&zoom=16&lat=${lat.toFixed(5)}&lon=${lon.toFixed(5)}`)
        .then((r) => (r.ok ? r.json() : null))
        .then((data) => {
          const a = data?.address || {};
          const primary = a.road || a.neighbourhood || a.suburb || a.village || a.town || a.city;
          const secondary = a.suburb || a.town || a.city || a.state;
          const place = primary && secondary && primary !== secondary ? `${primary}, ${secondary}` : (primary || secondary);
          if (place) {
            writeNominatimCache(revKey, place);
            if (state.verdictTarget && Math.abs(state.verdictTarget.lat - lat) < 1e-5 && Math.abs(state.verdictTarget.lon - lon) < 1e-5) {
              $('sv-title').textContent = place;
              $('sv-title').title = `${lat.toFixed(4)}°, ${lon.toFixed(4)}°`;
            }
          }
        })
        .catch(() => {});
    }
  }
  const color = lvl === 'danger' ? '#e11d48' : lvl === 'moderate' ? '#d97706' : '#0284c7';
  placePinMarker(color);
  const nowDisp = isNowWet ? fmtDepth(rawNowM, 2) : (rawNowM > 0.005 ? fmtDepth(rawNowM, 2) : '0.00 m');
  const peakDisp = isFlooded ? fmtDepth(rawPeakM, 2) : `< ${Math.round(cut * 100)} cm`;
  const padBtn = $('btn-sv-raise');
  if (padBtn) padBtn.style.display = state.profile === 'public' ? 'none' : '';
  $('sv-stats').innerHTML = `
    <div class="mini-stat"><div class="mini-k">Current Depth</div><div class="mini-v">${nowDisp}</div></div>
    <div class="mini-stat"><div class="mini-k">Peak (5d)</div><div class="mini-v">${peakDisp}</div></div>
    <div class="mini-stat"><div class="mini-k">Above Floor</div><div class="mini-v">${fmtDepth(netM, 2)}</div></div>
    <div class="mini-stat"><div class="mini-k">Recession</div><div class="mini-v">${isFlooded ? recInfo.label : 'Dry'}</div></div>
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

  const [minLon, minLat, maxLon, maxLat] = meta.wgs84_bounds;
  const hLon = minLon + ((col + 0.5) / meta.cols) * (maxLon - minLon);
  const hLat = minLat + ((row + 0.5) / meta.rows) * (maxLat - minLat);
  $('inspector-title').textContent = `Hydrograph · ${hLat.toFixed(4)}°, ${hLon.toFixed(4)}°`;
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
  const useSubgridPool = dxM >= 150;
  for (const s of samples) {
    const data = tileData.get(s.tkey);
    if (!data) { raw.push(null); continue; }
    // Keep open ocean / narrow sea straits (center z <= 0.05 m) strictly masked as nodata;
    // never let neighboring coastal land pixels promote a sea cell into active land.
    const cOff = (s.py * 256 + s.px) * 4;
    const centerZ = (data[cOff] * 256 + data[cOff + 1] + data[cOff + 2] / 256) - 32768;
    if (!(Number.isFinite(centerZ) && centerZ > 0.05)) {
      raw.push(null);
      continue;
    }
    let z = centerZ;
    if (useSubgridPool) {
      // 3x3 sub-grid lower-quartile (p25) pooling preserves narrow inland flood valleys
      // (e.g. 300-500 m kampung depressions) on coarse district/statewide meshes.
      const vals = [];
      for (let dy = -1; dy <= 1; dy++) {
        const py = Math.min(255, Math.max(0, s.py + dy));
        for (let dx = -1; dx <= 1; dx++) {
          const px = Math.min(255, Math.max(0, s.px + dx));
          const off = (py * 256 + px) * 4;
          const v = (data[off] * 256 + data[off + 1] + data[off + 2] / 256) - 32768;
          if (Number.isFinite(v) && v > 0.05) vals.push(v);
        }
      }
      if (vals.length >= 3) {
        vals.sort((a, b) => a - b);
        z = vals[Math.floor(vals.length * 0.25)];
      }
    }
    raw.push(z);
    validCount++;
    if (z < minZ) minZ = z;
    if (z > maxZ) maxZ = z;
  }
  if (validCount < cols * rows * 0.15 || (maxZ - minZ) < 0.2) return null;
  const elevations = smoothScreeningDem(raw, cols, rows);
  // Build a high-resolution shoreline land mask at rasterCanvas resolution from the
  // already-fetched Terrarium tiles so coarse coastal cells never bleed onto sea channels.
  const maskW = rasterCanvas.width;
  const maskH = rasterCanvas.height;
  const landMask = new Uint8Array(maskW * maskH);
  for (let py = 0; py < maskH; py++) {
    const lat = maxLat - ((py + 0.5) / maskH) * (maxLat - minLat);
    for (let px = 0; px < maskW; px++) {
      const lon = minLon + ((px + 0.5) / maskW) * (maxLon - minLon);
      const tp = lonLatToTilePixel(lon, lat, zoom);
      const d = tileData.get(`${tp.tx}/${tp.ty}`);
      if (!d) { landMask[py * maskW + px] = 1; continue; }
      const off = (tp.py * 256 + tp.px) * 4;
      const v = (d[off] * 256 + d[off + 1] + d[off + 2] / 256) - 32768;
      landMask[py * maskW + px] = (Number.isFinite(v) && v > 0.05) ? 1 : 0;
    }
  }
  state.cachedDem = { key, elevations, landMask, maskW, maskH };
  // One-shot land-clipped framing for coastal regional boundaries (removes empty sea wedge)
  if (state.boundaryNeedsLandFit && state.boundaryGeoJson && map && !state.fixedView) {
    state.boundaryNeedsLandFit = false;
    let lMinLon = Infinity, lMaxLon = -Infinity, lMinLat = Infinity, lMaxLat = -Infinity;
    for (let r = 0; r < rows; r++) {
      const lat = minLat + ((r + 0.5) / rows) * (maxLat - minLat);
      for (let c = 0; c < cols; c++) {
        if (elevations[r * cols + c] === null) continue;
        const lon = minLon + ((c + 0.5) / cols) * (maxLon - minLon);
        if (!pointInGeom(lon, lat, state.boundaryGeoJson)) continue;
        if (lon < lMinLon) lMinLon = lon;
        if (lon > lMaxLon) lMaxLon = lon;
        if (lat < lMinLat) lMinLat = lat;
        if (lat > lMaxLat) lMaxLat = lat;
      }
    }
    if (Number.isFinite(lMinLon) && lMaxLon > lMinLon && lMaxLat > lMinLat) {
      const vw = window.innerWidth || 1280;
      const vh = window.innerHeight || 800;
      map.stop();
      map.fitBounds([[lMinLon, lMinLat], [lMaxLon, lMaxLat]], {
        padding: {
          top: Math.min(64, Math.round(vh * 0.12)),
          bottom: Math.min(78, Math.round(vh * 0.14)),
          left: Math.min(310, Math.round(vw * 0.24)),
          right: Math.min(235, Math.round(vw * 0.20)),
        },
        pitch: 0,
        bearing: 0,
        duration: 0,
      });
    }
  }
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
  const horizonHoursMap = { '6h': 6, '1d': 24, '2d': 48, '3d': 72, '5d': 120 };
  const targetHours = horizonHoursMap[sc.horizon] || 48;
  let hourlyRain = (state.hourlyRainMm || []).slice(0, targetHours).map((v) => v * mult);
  let spatialGrid = state.spatialRainGrid ? {
    ...state.spatialRainGrid,
    hourly_mm: (state.spatialRainGrid.hourly_mm || []).map((row) => row.slice(0, targetHours).map((v) => v * mult)),
    et0_mm: (state.spatialRainGrid.et0_mm || []).map((row) => row.slice(0, targetHours)),
  } : null;
  if (spatialGrid && spatialGrid.hourly_mm?.[0]?.length) {
    state.spanHours = spatialGrid.hourly_mm[0].length;
  }
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

  const nRainHours = Math.max(
    hourlyRain.length,
    spatialGrid?.hourly_mm?.[0]?.length || 0,
  );
  if (nRainHours > 0) {
    // Keep time compression <= 220x (safely under backend 240x CFL/compression guard)
    const minDurForHours = Math.ceil((nRainHours * 3600) / 220);
    durationS = Math.max(durationS, minDurForHours);
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
    interventions: state.interventions.length > 0 ? state.interventions : (sc.startDry ? [] : (state.liveInterventions || [])),
    baseline_interventions: state.baselineInterventions,
    ascii_dem: state.uploadedDem,
    terrain_elevations_m: sampled,
    terrain_provenance: sampled ? 'BrowserTerrariumScreening' : null,
    antecedent_rain_mm: sc.startDry ? 0 : (state.antecedentMm || 0),
    hourly_rain_mm: hourlyRain,
    spatial_rain_grid: spatialGrid,
    hourly_tide_m: sc.tideMode === 'off' ? [] : (state.hourlyTideM || []),
    drainage_mm_per_hr: drainHr,
    storm_drain_score: Number.isFinite(sc.stormDrainScore) ? Math.min(1, Math.max(0, sc.stormDrainScore)) : 0.5,
    storm_drain_grid: Array.isArray(state.stormDrainGrid) && state.stormDrainGrid.length === cols * rows ? state.stormDrainGrid : null,
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

function applyLoadedSimulation(grid, frames, runId, engineLabel, framesBuf = null) {
  state.grid = grid;
  state.frames = frames;
  if (framesBuf) state.lastFramesBuf = framesBuf;
  state.runId = runId;
  state.activeEngine = engineLabel || 'Server · F64';
  if (!state.userOverrideCutoff && grid?.metadata?.dx_m) {
    state.displayPrefs.depthCutoffM = grid.metadata.dx_m >= 300 ? 0.15 : 0.08;
  }
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
  setLayer(state.layer);
}

async function loadAllPrecomputedLiveBasins(primaryBasinId) {
  try {
    const resp = await fetch(apiUrl('api/v1/live/basins'));
    if (!resp.ok) return;
    const data = await resp.json();
    const basins = Array.isArray(data?.basins) ? data.basins : [];
    const others = basins.filter((id) => id && id !== primaryBasinId && !state.liveOverlays.has(id));
    await Promise.all(others.map(async (id) => {
      try {
        const [gResp, bResp] = await Promise.all([
          fetch(apiUrl(`api/v1/live/${encodeURIComponent(id)}/grid`)),
          fetch(apiUrl(`api/v1/live/${encodeURIComponent(id)}/frames.bin`)),
        ]);
        if (!gResp.ok || !bResp.ok) return;
        const grid = await gResp.json();
        const frames = decodeFramesBin(await bResp.arrayBuffer());
        if (frames.length) {
          state.liveOverlays.set(id, { grid, frames, lastKey: '' });
        }
      } catch (_) {}
    }));
    renderSecondaryLiveOverlays(true);
    refreshAlertsAndBeacons();
    updateLiveMetrics();
  } catch (_) {}
}

async function tryLoadPrecomputedLive(basinId) {
  if (!basinId) return false;
  try {
    const [gridResp, binResp] = await Promise.all([
      fetch(apiUrl(`api/v1/live/${encodeURIComponent(basinId)}/grid`)),
      fetch(apiUrl(`api/v1/live/${encodeURIComponent(basinId)}/frames.bin`)),
    ]);
    if (!gridResp.ok || !binResp.ok) return false;
    const grid = await gridResp.json();
    const buf = await binResp.arrayBuffer();
    const frames = decodeFramesBin(buf);
    if (!frames.length) return false;
    state.liveBasinId = basinId;
    state.liveOverlays.set(basinId, { grid, frames, lastKey: '' });
    // Pre-computed Live caches span 5 days (120 hours) from the latest forecast cycle
    state.spanHours = 120;
    const cycleMs = Date.parse(state.serverContext?.context?.cycle_iso || '');
    if (Number.isFinite(cycleMs)) state.startEpochMs = cycleMs;
    renderForecastSegments();
    applyLoadedSimulation(grid, frames, grid.run_id || `live-${basinId}`, 'Live · Cached', buf);
    loadAllPrecomputedLiveBasins(basinId);
    return true;
  } catch (_) {
    return false;
  }
}

async function loadInitialLiveOrSim() {
  const qs = new URLSearchParams(window.location.search);
  const isCustomParamSim = qs.get('presentation') === '1' || qs.get('fixed') === '1' || qs.has('rain');
  const ctxBasin = state.serverContext?.context?.basin_id;
  if (!isCustomParamSim && ctxBasin && await tryLoadPrecomputedLive(ctxBasin)) {
    return;
  }
  // In Simulate tab without pre-cached data, wait for user to click Run Simulation
  if (state.profile === 'researcher' && !isCustomParamSim) return;
  await runSimulation(false);
}

async function loadRun(runId) {
  const [gridResp, binResp] = await Promise.all([
    fetch(apiUrl(`api/v1/runs/${runId}/grid`)),
    fetch(apiUrl(`api/v1/runs/${runId}/frames.bin`)),
  ]);
  if (!gridResp.ok || !binResp.ok) throw new Error('Failed to load simulation frames');
  const grid = await gridResp.json();
  const buf = await binResp.arrayBuffer();
  const frames = decodeFramesBin(buf);
  applyLoadedSimulation(grid, frames, runId, 'Server · F64', buf);
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
      state.simWorker = new Worker(apiUrl('assets/sim-worker.js'), { type: 'module' });
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
        resolve({ engine: msg.engine || 'WASM · F32', grid, frames, framesBuffer: msg.framesBuffer });
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

let simProgressTimer = null;
let simProgressStartMs = 0;

function updateSimDraftSummary() {
  const el = $('sim-draft-summary');
  if (!el) return;
  const sc = state.simConfig;
  const modeStr = sc.mode === 'stress'
    ? `${sc.stressMmDay}mm/d × ${sc.stressDays}d Stress`
    : (sc.mode === 'physics'
      ? `${sc.rainfallMmHr}mm/hr · ${sc.durationS}s`
      : `${sc.horizon || '2d'} Forecast`);
  const initStr = sc.startDry ? 'Dry t=0' : 'Live t=0';
  const drainPct = Math.round((Number.isFinite(sc.stormDrainScore) ? sc.stormDrainScore : 0.5) * 100);
  const demTag = state.uploadedDem ? ' · Custom .ASC' : '';
  el.textContent = `${modeStr} · ${initStr} · Drain ${drainPct}%${demTag}`;
}

function updateSimResultMeta() {
  const el = $('sim-result-meta');
  if (!el) return;
  const m = state.grid?.metadata;
  const gridStr = m ? `${m.cols}×${m.rows} · ${Math.round(m.dx_m)}m` : '—';
  const eng = state.activeEngine || 'Solved';
  const massOk = state.grid?.budget_assessment?.passed !== false ? 'Mass OK' : 'Mass Warn';
  el.textContent = `${eng} · ${gridStr} · ${massOk}`;
}

function setSimPhase(phase) {
  const next = (phase === 'solving' || phase === 'results') ? phase : 'draft';
  state.simPhase = next;
  document.documentElement.dataset.simPhase = next;
  const badge = $('sim-phase-badge');
  if (badge) {
    badge.className = `sim-phase-badge ${next === 'results' ? 'results' : 'draft'}`;
    badge.textContent = next === 'results' ? 'Results (Read-Only)' : (next === 'solving' ? 'Solving…' : 'Draft Setup');
  }
  $('sim-phase-draft')?.classList.toggle('hidden', next === 'results');
  $('sim-phase-results')?.classList.toggle('hidden', next !== 'results');
  if (next === 'draft') {
    updateSimDraftSummary();
    if (state.profile === 'researcher' && (state.tool === 'inspect' || state.tool === 'xsection')) {
      setTool('bund');
    }
    if ($('inspector')) $('inspector').classList.add('hidden');
    $('sim-dir-drawer')?.classList.add('hidden');
  } else if (next === 'results') {
    updateSimResultMeta();
    if (state.profile === 'researcher' && !['inspect', 'xsection'].includes(state.tool)) {
      setTool('inspect');
    }
  }
}

function setSimProgress(text, stageIdx = -1) {
  const overlay = $('sim-progress');
  if (!overlay) return;
  if (!text) {
    overlay.classList.add('hidden');
    if (simProgressTimer) {
      clearInterval(simProgressTimer);
      simProgressTimer = null;
    }
    return;
  }
  if ($('sim-progress-text')) $('sim-progress-text').textContent = text;
  if (overlay.classList.contains('hidden')) {
    simProgressStartMs = performance.now();
    if ($('sim-elapsed')) $('sim-elapsed').textContent = '00:00.0';
    if (simProgressTimer) clearInterval(simProgressTimer);
    simProgressTimer = setInterval(() => {
      const sec = (performance.now() - simProgressStartMs) / 1000;
      const m = Math.floor(sec / 60);
      const s = sec - m * 60;
      if ($('sim-elapsed')) {
        $('sim-elapsed').textContent = `${String(m).padStart(2, '0')}:${s < 10 ? '0' : ''}${s.toFixed(1)}`;
      }
    }, 100);
  }
  if (stageIdx >= 0) {
    for (let i = 0; i < 4; i++) {
      const stepEl = $(`sim-step-${i}`);
      if (!stepEl) continue;
      stepEl.classList.toggle('done', i < stageIdx);
      stepEl.classList.toggle('active', i === stageIdx);
    }
  }
  overlay.classList.remove('hidden');
}

async function runSimulation(compareMode) {
  if (state.serverConfig?.features?.allow_simulation === false) {
    return;
  }
  if (state.activeAbort) {
    try { state.activeAbort.abort(); } catch (_) {}
  }
  const abort = new AbortController();
  state.activeAbort = abort;
  state.lastCompareMode = Boolean(compareMode);
  const wasPlaying = state.playing;
  state.playing = false;
  if ($('btn-play')) $('btn-play').textContent = '▶';
  if ($('fb-play')) $('fb-play').textContent = '▶';
  if (state.profile === 'researcher') setSimPhase('solving');

  const btn = compareMode ? $('btn-compare') : $('btn-run');
  const orig = btn ? btn.innerHTML : '';
  if (btn) {
    btn.disabled = true;
    btn.innerHTML = '<span class="spinner"></span>Solving…';
  }
  setSimProgress('Sampling terrain DEM…', 0);
  let solvedOk = false;
  try {
    const payload = await buildRequestPayload();
    if (abort.signal.aborted) return;
    setSimProgress('Applying rainfall & tide forcing…', 1);
    setSimProgress(compareMode ? 'Solving Baseline vs Intervention…' : 'Solving 2D shallow water…', 2);
    if (compareMode) {
      // Cap interactive compare window to 24h (360s, <=240x) so Baseline vs Intervention solves in <2s
      if (payload.spatial_rain_grid && Array.isArray(payload.spatial_rain_grid.hourly_mm)) {
        const hrs = payload.spatial_rain_grid.hourly_mm[0]?.length || 0;
        if (hrs > 24 && payload.duration_s > 600) {
          payload.spatial_rain_grid = {
            ...payload.spatial_rain_grid,
            hourly_mm: payload.spatial_rain_grid.hourly_mm.map((row) => row.slice(0, 24)),
          };
          payload.duration_s = 360;
        }
      }
      if (Array.isArray(payload.hourly_rain_mm) && payload.hourly_rain_mm.length > 24 && payload.duration_s > 600) {
        payload.hourly_rain_mm = payload.hourly_rain_mm.slice(0, 24);
        payload.duration_s = 360;
      }
      const resp = await fetch(apiUrl('api/v1/compare'), {
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
      state.staticVectorsDirty = true;
      setLayer('diff');
      $('inspector').classList.remove('hidden');
      $('inspector-svg').classList.add('hidden');
      const isPres = document.documentElement.dataset.presentation === 'true';
      if ($('btn-inspector-pin')) $('btn-inspector-pin').classList.toggle('hidden', isPres);
      const isBreach = state.baselineInterventions.length > 0;
      const breachedCells = Math.max(0, state.baselineInterventions.length - state.interventions.length);
      $('inspector-title').textContent = isBreach ? 'Bund Breach Δh Impact' : 'Intervention Δh Impact';
      const earthworkRow = isBreach
        ? `<div class="mini-stat"><div class="mini-k">Breach Gap</div><div class="mini-v bad">${breachedCells} cells removed</div></div>`
        : `<div class="mini-stat"><div class="mini-k">Earthwork</div><div class="mini-v">${Math.round(state.diffData.earthwork.fill_volume_m3).toLocaleString()} m³ fill</div></div>`;
      $('inspector-stats').innerHTML = `
        <div class="mini-stat"><div class="mini-k">${isBreach ? 'River Drawdown' : 'Protected'}</div><div class="mini-v good">${state.diffData.improved_cells} cells (−${fmtDepth(state.diffData.max_reduction_m, 2)})</div></div>
        <div class="mini-stat"><div class="mini-k">Worsened</div><div class="mini-v bad">${state.diffData.worsened_cells} cells (+${fmtDepth(state.diffData.max_worsening_m, 2)})</div></div>
        ${earthworkRow}
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
        applyLoadedSimulation(browserRes.grid, browserRes.frames, browserRes.grid.run_id, browserRes.engine, browserRes.framesBuffer);
        if (!state.fixedView && !state.boundaryGrid) {
          const [minLon, minLat, maxLon, maxLat] = browserRes.grid.metadata.wgs84_bounds;
          map.flyTo({
            center: [(minLon + maxLon) * 0.5, (minLat + maxLat) * 0.5],
            zoom: 14.4,
            pitch: state.is3D ? 48 : 0,
            duration: 800,
          });
        }
      } else {
        const allowAsyncJobs = state.serverConfig?.features?.allow_async_jobs !== false;
        const useAsyncJob = allowAsyncJobs && (payload.duration_s > 900 || payload.cols * payload.rows > 8192);
        let summary = null;
        if (!useAsyncJob) {
          const resp = await fetch(apiUrl('api/v1/simulations'), {
            method: 'POST',
            headers: { 'content-type': 'application/json' },
            body: JSON.stringify(payload),
            signal: abort.signal,
          });
          if (resp.status === 408 && allowAsyncJobs) {
            setSimProgress('Long run queued as async basin job…');
          } else if (!resp.ok) {
            throw new Error(await parseErrorText(resp));
          } else {
            summary = await resp.json();
          }
        }
        if (!summary && allowAsyncJobs) {
          const jobResp = await fetch(apiUrl('api/v1/jobs'), {
            method: 'POST',
            headers: { 'content-type': 'application/json' },
            body: JSON.stringify(payload),
            signal: abort.signal,
          });
          if (!jobResp.ok) throw new Error(await parseErrorText(jobResp));
          const job = await jobResp.json();
          const jobId = job.job_id;
          const onCancelJob = () => {
            fetch(apiUrl(`api/v1/jobs/${jobId}/cancel`), { method: 'POST' }).catch(() => {});
          };
          abort.signal.addEventListener('abort', onCancelJob, { once: true });
          try {
            while (true) {
              if (abort.signal.aborted) {
                const err = new Error('Aborted');
                err.name = 'AbortError';
                throw err;
              }
              const pollResp = await fetch(apiUrl(`api/v1/jobs/${jobId}`), { signal: abort.signal });
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
        setSimProgress('Loading frames & rendering…', 3);
        await loadRun(summary.run_id);
        if (!state.fixedView && !state.boundaryGrid) {
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
    solvedOk = true;
  } catch (err) {
    if (err && err.name === 'AbortError') {
      showToast('Simulation cancelled — draft edits preserved.');
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
    if (state.profile === 'researcher') {
      if (solvedOk) {
        setSimPhase('results');
        await autoSaveCompletedRun();
      } else {
        setSimPhase('draft');
      }
    } else if (wasPlaying && solvedOk) {
      state.playing = true;
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
    rainPart = `${meanRate.toFixed(1)} mm/hr NWP`;
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
  const isPres = document.documentElement.dataset.presentation === 'true';
  $('telemetry').textContent = isPres
    ? `${c.lat.toFixed(2)}°, ${c.lng.toFixed(2)}° · z${z.toFixed(1)} · ${rainPart}${windPart}`
    : `${c.lat.toFixed(2)}°, ${c.lng.toFixed(2)}° · ${rainPart}${windPart}`;
}

function updateHonestyPill() {
  const pill = $('honesty-pill');
  const txt = $('honesty-text');
  if (!state.grid) return;
  const ok = state.grid.budget_assessment.passed;
  const res = Math.abs(state.grid.water_budget.residual_m3);
  const prov = state.grid.metadata.terrain_provenance;
  const isPres = document.documentElement.dataset.presentation === 'true';
  const demTag = isPres
    ? (prov === 'BrowserTerrariumScreening' ? 'Terrarium 30m' : prov === 'UploadedAsciiDem' ? 'Uploaded .ASC' : 'Synthetic DEM')
    : (prov === 'BrowserTerrariumScreening' ? '30m DEM' : prov === 'UploadedAsciiDem' ? '.ASC DEM' : 'Synthetic');
  pill.classList.toggle('fail', !ok);
  pill.title = `Engine: ${state.activeEngine} · Mass residual: ${res.toExponential(1)} m³ (screening only)`;
  const massTag = ok ? `Mass OK` : `Mass FAIL`;
  txt.textContent = isPres
    ? `${demTag} screening · Mass ${ok ? 'OK' : 'FAIL'}`
    : `${demTag} · ${massTag}`;
  updateFrameTelemetry();
}

function fmtMMSS(sec) {
  const s = Math.max(0, Math.round(sec));
  const m = Math.floor(s / 60);
  const rem = s % 60;
  return `${String(m).padStart(2, '0')}:${String(rem).padStart(2, '0')}`;
}

function fmtCalendarTime(epochMs, tzLabel) {
  const p = getTimePartsInTz(epochMs);
  const label = tzLabel || p.tzShort;
  return `${p.year}-${p.month}-${p.day} ${String(p.h24).padStart(2, '0')}:${String(p.mi).padStart(2, '0')}:${String(p.sec).padStart(2, '0')} ${label}`;
}

const DAYS_SHORT = ['Sun', 'Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat'];

function fmtCalloutTime(epochMs, tzLabel) {
  const p = getTimePartsInTz(epochMs);
  const label = tzLabel || p.tzShort;
  if (state.displayPrefs.clock === '24h') {
    return `${p.weekday} ${p.dayNum} · ${String(p.h24).padStart(2, '0')}:${String(p.mi).padStart(2, '0')} ${label}`;
  }
  const ampm = p.h24 >= 12 ? 'PM' : 'AM';
  const h12 = p.h24 % 12 || 12;
  const minPart = p.mi === 0 ? '' : `:${String(p.mi).padStart(2, '0')}`;
  return `${p.weekday} ${p.dayNum} · ${h12}${minPart} ${ampm} ${label}`;
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
      const p = getTimePartsInTz(startMs + i * 86_400_000);
      segs.push(`<div class="fb-seg" data-seg="${i}" data-day="${p.dayKey}">${p.weekday} ${p.dayNum}</div>`);
    }
  } else {
    const slots = Math.max(4, Math.min(8, Math.round(state.spanHours / 3) || 4));
    for (let i = 0; i < slots; i++) {
      const p = getTimePartsInTz(startMs + (spanMs * i) / slots);
      const hh = String(p.h24).padStart(2, '0');
      const mi = String(p.mi).padStart(2, '0');
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
  const show = isPres ? true : Boolean(state.displayPrefs.showMetrics);
  card.classList.toggle('hidden', !show);
  if (!show) return;

  const cut = state.displayPrefs.depthCutoffM || 0.06;
  const idx = Math.min(state.frameIdx, state.frames.length - 1);
  const usePeakEnv = Boolean(state.envelopeMode && state.grid.envelope?.max_depth_m);
  if (!state.boundaryCellMask || state.boundaryCellMask.length !== state.grid.metadata.cols * state.grid.metadata.rows) {
    rebuildBoundaryCellMask();
  }
  const mask = state.boundaryCellMask;
  let totalCells = 0;
  let floodedCells = 0;
  let stagnantCells = 0;
  let sumDepth = 0;
  let peakDepth = 0;
  let stagnantKm2 = 0;
  let viewportLabel = null;

  // In Live mode without a locked boundary, compute stats adaptively across the current viewport
  if (state.profile === 'public' && !state.boundaryGeoJson && map && state.liveOverlays.size > 0) {
    const vb = map.getBounds();
    const vMinLon = vb.getWest();
    const vMaxLon = vb.getEast();
    const vMinLat = vb.getSouth();
    const vMaxLat = vb.getNorth();
    const hitStates = new Set();
    for (const [basinId, ov] of state.liveOverlays.entries()) {
      const m = ov.grid?.metadata;
      const e = ov.grid?.envelope;
      if (!m || !ov.frames?.length) continue;
      const [bMinLon, bMinLat, bMaxLon, bMaxLat] = m.wgs84_bounds;
      if (bMaxLon < vMinLon || bMinLon > vMaxLon || bMaxLat < vMinLat || bMinLat > vMaxLat) continue;
      const cols = m.cols;
      const rows = m.rows;
      const fIdx = Math.min(state.frameIdx, ov.frames.length - 1);
      const dArr = (state.envelopeMode && e?.max_depth_m) ? e.max_depth_m : ov.frames[fIdx].depth;
      const aMask = m.active_mask;
      const cKm2 = (m.dx_m * m.dx_m) / 1_000_000;
      let basinHits = 0;
      for (let r = 0; r < rows; r++) {
        const lat = bMinLat + ((r + 0.5) / rows) * (bMaxLat - bMinLat);
        if (lat < vMinLat || lat > vMaxLat) continue;
        for (let c = 0; c < cols; c++) {
          const lon = bMinLon + ((c + 0.5) / cols) * (bMaxLon - bMinLon);
          if (lon < vMinLon || lon > vMaxLon) continue;
          const cellIdx = r * cols + c;
          if (!aMask[cellIdx]) continue;
          basinHits++;
          totalCells++;
          const h = dArr[cellIdx];
          if (h > 0) sumDepth += h;
          if (h >= cut) {
            floodedCells++;
            stagnantCells++;
            stagnantKm2 += cKm2;
          }
          if (h > peakDepth) peakDepth = h;
        }
      }
      if (basinHits > 0) hitStates.add(formatStateName(basinId));
    }
    // If zoomed tighter than 1 grid cell, sample the enclosing cell at map center so stats never drop to 0
    if (totalCells === 0) {
      const ctr = map.getCenter();
      for (const [basinId, ov] of state.liveOverlays.entries()) {
        const m = ov.grid?.metadata;
        const e = ov.grid?.envelope;
        if (!m || !ov.frames?.length) continue;
        const [bMinLon, bMinLat, bMaxLon, bMaxLat] = m.wgs84_bounds;
        if (ctr.lng < bMinLon || ctr.lng > bMaxLon || ctr.lat < bMinLat || ctr.lat > bMaxLat) continue;
        const c = Math.min(m.cols - 1, Math.max(0, Math.floor(((ctr.lng - bMinLon) / (bMaxLon - bMinLon)) * m.cols)));
        const r = Math.min(m.rows - 1, Math.max(0, Math.floor(((ctr.lat - bMinLat) / (bMaxLat - bMinLat)) * m.rows)));
        const cellIdx = r * m.cols + c;
        if (!m.active_mask[cellIdx]) continue;
        const fIdx = Math.min(state.frameIdx, ov.frames.length - 1);
        const dArr = (state.envelopeMode && e?.max_depth_m) ? e.max_depth_m : ov.frames[fIdx].depth;
        const h = dArr[cellIdx];
        totalCells = 1;
        sumDepth = h > 0 ? h : 0;
        peakDepth = h;
        if (h >= cut) {
          floodedCells = 1;
          stagnantCells = 1;
          stagnantKm2 = (m.dx_m * m.dx_m) / 1_000_000;
        }
        hitStates.add(formatStateName(basinId));
        break;
      }
    }
    const statesArr = Array.from(hitStates);
    if (statesArr.length === 0) {
      viewportLabel = 'Current View';
    } else if (statesArr.length === 1) {
      viewportLabel = map.getZoom() >= 9.8 ? `Local View · ${statesArr[0]}` : statesArr[0];
    } else if (statesArr.length <= 2) {
      viewportLabel = statesArr.join(' & ');
    } else {
      viewportLabel = `Malaysia (${statesArr.length} states in view)`;
    }
  } else {
    const meta = state.grid.metadata;
    const frame = state.frames[idx];
    const depth = usePeakEnv ? state.grid.envelope.max_depth_m : frame.depth;
    const [bMinLon, bMinLat, bMaxLon, bMaxLat] = meta.wgs84_bounds;
    const cols = meta.cols;
    const rows = meta.rows;
    // Intersect focused boundary with current map viewport so zooming in updates KPIs live
    const useViewportClip = state.profile === 'public' && Boolean(map);
    const vb = useViewportClip ? map.getBounds() : null;
    const vMinLon = vb ? vb.getWest() : -180;
    const vMaxLon = vb ? vb.getEast() : 180;
    const vMinLat = vb ? vb.getSouth() : -90;
    const vMaxLat = vb ? vb.getNorth() : 90;
    for (let r = 0; r < rows; r++) {
      const lat = bMinLat + ((r + 0.5) / rows) * (bMaxLat - bMinLat);
      if (useViewportClip && (lat < vMinLat || lat > vMaxLat)) continue;
      for (let c = 0; c < cols; c++) {
        const i = r * cols + c;
        if (!mask[i]) continue;
        if (useViewportClip) {
          const lon = bMinLon + ((c + 0.5) / cols) * (bMaxLon - bMinLon);
          if (lon < vMinLon || lon > vMaxLon) continue;
        }
        totalCells++;
        const h = depth[i];
        if (h > 0) sumDepth += h;
        if (h >= cut) {
          floodedCells++;
          stagnantCells++;
        }
        if (h > peakDepth) peakDepth = h;
      }
    }
    // If zoomed tighter than 1 grid cell inside focused boundary, sample the center cell
    if (totalCells === 0 && useViewportClip) {
      const ctr = map.getCenter();
      if (ctr.lng >= bMinLon && ctr.lng <= bMaxLon && ctr.lat >= bMinLat && ctr.lat <= bMaxLat) {
        const c = Math.min(cols - 1, Math.max(0, Math.floor(((ctr.lng - bMinLon) / (bMaxLon - bMinLon)) * cols)));
        const r = Math.min(rows - 1, Math.max(0, Math.floor(((ctr.lat - bMinLat) / (bMaxLat - bMinLat)) * rows)));
        const i = r * cols + c;
        if (mask[i]) {
          const h = depth[i];
          totalCells = 1;
          sumDepth = h > 0 ? h : 0;
          peakDepth = h;
          if (h >= cut) { floodedCells = 1; stagnantCells = 1; }
        }
      }
    }
    const cellKm2 = (meta.dx_m * meta.dx_m) / 1_000_000;
    stagnantKm2 = stagnantCells * cellKm2;
  }
  const avgDepth = totalCells > 0 ? sumDepth / totalCells : 0;
  const floodedPct = totalCells > 0 ? (floodedCells / totalCells) * 100 : 0;

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
  if ($('mob-sum-dot')) $('mob-sum-dot').className = `mob-dot ${lvl}`;
  if ($('mob-sum-text')) {
    $('mob-sum-text').textContent = `${label} · ${fmtDepth(peakDepth, 2)} ▾`;
  }
  const trendEl = $('lm-trend');
  if (trendEl) {
    if (usePeakEnv) {
      trendEl.className = 'lm-trend';
      trendEl.textContent = 'Peak Max';
    } else if (idx === 0) {
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
      const presetName = sel && sel.selectedOptions[0] ? sel.selectedOptions[0].textContent : 'Local Flood Screening';
      const locName = viewportLabel || presetName;
      $('po-title').textContent = locName;
      const risk = $('po-risk');
      risk.className = `po-risk ${lvl}`;
      risk.textContent = label;
      const sc = state.simConfig;
      const hasLiveGrid = Boolean(state.spatialRainGrid) || state.activeEngine === 'Live · Cached';
      const modeDesc = hasLiveGrid
        ? 'live NWP regional forecast'
        : (sc.mode === 'stress'
          ? `hypothetical ${sc.stressMmDay} mm/day × ${sc.stressDays}d stress-test`
          : `hypothetical ${fmtRain(sc.rainfallMmHr * (sc.rainMult || 1), 0)} scenario — not live forecast`);
      const actionHint = peakDepth >= 0.30
        ? ' >30 cm: move vehicles and do not drive through floodwater.'
        : peakDepth >= 0.15
          ? ' 15–30 cm: move parked cars from low streets.'
          : ' Click any spot on the map to check your floor height.';
      $('po-summary').textContent =
        `Peak depth ${fmtDepth(peakDepth, 2)} across ${floodedPct.toFixed(0)}% of area (${modeDesc}).${actionHint}`;
      $('po-hotspots').innerHTML = '';
      if (state.serverConfig?.disclaimer) {
        const staleTag = state.serverContext?.is_stale ? ' [Feed stale >12h]' : '';
        $('po-foot').textContent = `${state.serverConfig.disclaimer}${staleTag}`;
      }
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
        const cut = state.displayPrefs.depthCutoffM || 0.06;
        tone = hCur >= 0.35 ? 'danger' : hCur >= cut ? 'warn' : '';
        statusText = hCur >= cut ? `~${hCur.toFixed(1)} m est.` : `Dry (<${Math.round(cut * 100)} cm)`;
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

function fmtMytDisplay(epochMs, includeYear = true) {
  const tz = getActiveTimeZone();
  const p = getTimePartsInTz(epochMs, tz);
  const d = new Date(epochMs);
  let datePart;
  try {
    datePart = d.toLocaleDateString('en-GB', {
      day: '2-digit',
      month: 'short',
      ...(includeYear ? { year: 'numeric' } : {}),
      timeZone: tz,
    });
  } catch (_) {
    datePart = `${p.day} ${p.month}${includeYear ? ` ${p.year}` : ''}`;
  }
  const timePart = state.displayPrefs.clock === '24h'
    ? `${String(p.h24).padStart(2, '0')}:${String(p.mi).padStart(2, '0')}`
    : `${p.h24 % 12 || 12}:${String(p.mi).padStart(2, '0')} ${p.h24 >= 12 ? 'PM' : 'AM'}`;
  return `${datePart}, ${timePart} ${p.tzShort}`;
}

function fmtMytSlug(epochMs) {
  const d = new Date(epochMs);
  const parts = new Intl.DateTimeFormat('en-GB', {
    year: 'numeric', month: '2-digit', day: '2-digit',
    hour: '2-digit', minute: '2-digit', hour12: false,
    timeZone: 'Asia/Kuala_Lumpur',
  }).formatToParts(d);
  const get = (t) => parts.find((p) => p.type === t)?.value || '00';
  return `${get('year')}${get('month')}${get('day')}-${get('hour')}${get('minute')}myt`;
}

function getCurrentFrameEpochMs() {
  const idx = Math.min(state.frameIdx, Math.max(0, state.frames.length - 1));
  const cur = state.frames[idx]?.time_s || 0;
  const tot = Math.max(1, state.frames[state.frames.length - 1]?.time_s || 1);
  const baseMs = state.startEpochMs
    || (state.serverContext?.context?.cycle_iso ? Date.parse(state.serverContext.context.cycle_iso) : Date.now());
  if (state.spanHours > 0) {
    return baseMs + (cur / tot) * state.spanHours * 3600 * 1000;
  }
  return baseMs + cur * 1000;
}

function updateTimeReadout() {
  if (!state.frames.length) return;
  const idx = Math.min(state.frameIdx, state.frames.length - 1);
  const cur = state.frames[idx].time_s;
  const tot = state.frames[state.frames.length - 1].time_s || 0;
  const frameMs = getCurrentFrameEpochMs();
  if (state.spanHours > 0) {
    $('time-readout').textContent = fmtMytDisplay(frameMs, false);
  } else {
    $('time-readout').textContent = `${fmtMytDisplay(frameMs, false)} · ${fmtMMSS(cur)} / ${fmtMMSS(tot)}`;
  }

  if (state.spanHours > 0) {
    const frac = state.frames.length > 1 ? idx / (state.frames.length - 1) : 0;
    const epochMs = state.startEpochMs + frac * state.spanHours * 3600_000;
    const pct = frac * 100;
    $('fb-fill').style.width = `${pct.toFixed(2)}%`;
    const callout = $('fb-callout');
    callout.textContent = fmtCalloutTime(epochMs);
    callout.style.left = `${Math.max(12, Math.min(88, pct)).toFixed(2)}%`;
    const segNodes = document.querySelectorAll('#fb-segments .fb-seg');
    if (segNodes.length > 0) {
      let activeIdx;
      if (state.spanHours >= 24) {
        const curDayKey = getTimePartsInTz(epochMs).dayKey;
        let matched = -1;
        segNodes.forEach((n, i) => {
          if (n.dataset.day === curDayKey) matched = i;
        });
        activeIdx = matched >= 0
          ? matched
          : Math.min(segNodes.length - 1, Math.floor(frac * segNodes.length));
      } else {
        activeIdx = Math.min(segNodes.length - 1, Math.floor(frac * segNodes.length));
      }
      segNodes.forEach((n, i) => n.classList.toggle('active', i === activeIdx));
    }
  }
  updateFrameTelemetry();
  updateLiveMetrics();
  if (state.verdictTarget) renderSiteVerdict(true);
}

function updateInterventionBar() {
  const count = state.interventions.length;
  const dx = state.grid?.metadata?.dx_m || 30;
  const cellArea = dx * dx;
  let fillM3 = 0;
  let cutM3 = 0;
  for (const iv of state.interventions) {
    if (iv.delta_m > 0) fillM3 += iv.delta_m * cellArea;
    else if (iv.delta_m < 0) cutM3 += Math.abs(iv.delta_m) * cellArea;
  }
  const volParts = [];
  if (fillM3 > 0) volParts.push(`+${Math.round(fillM3).toLocaleString()} m³ fill`);
  if (cutM3 > 0) volParts.push(`-${Math.round(cutM3).toLocaleString()} m³ cut`);
  $('iv-count').textContent = volParts.length ? `${count} cells (${volParts.join(', ')})` : `${count} cells`;
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
  if (layer === 'diff' && !state.diffData) layer = 'depth';
  if (!['depth', 'velocity', 'hazard', 'arrival', 'diff'].includes(layer)) layer = 'depth';
  state.layer = layer;
  document.querySelectorAll('[data-layer]').forEach((el) => {
    el.classList.toggle('active', el.dataset.layer === layer);
  });
  const bar = $('legend-bar');
  const ticks = $('legend-ticks');
  const note = $('legend-note');
  const cut = state.displayPrefs.depthCutoffM || 0.06;
  if (layer === 'depth') {
    const span = Math.max(0.1, 1.20 - cut);
    const p25 = (((0.25 - cut) / span) * 100).toFixed(1);
    const p50 = (((0.50 - cut) / span) * 100).toFixed(1);
    const p85 = (((0.85 - cut) / span) * 100).toFixed(1);
    bar.style.background = `linear-gradient(90deg, #a5f3fc 0%, #22d3ee ${p25}%, #2563eb ${p50}%, #7c3aed ${p85}%, #4c1d95 100%)`;
    ticks.innerHTML = `<span>${fmtDepth(cut, 2)}</span><span>${fmtDepth(0.3, 1)}</span><span>${fmtDepth(0.6, 1)}</span><span>${fmtDepth(1.2, 1)}+</span>`;
    note.textContent = `Flood depth h (sheet runoff < ${Math.round(cut * 100)} cm hidden).`;
  } else if (layer === 'velocity') {
    bar.style.background = 'linear-gradient(90deg, #2dd4bf, #0284c7, #4f46e5)';
    ticks.innerHTML = `<span>${fmtSpeed(0.0, 1)}</span><span>${fmtSpeed(0.7, 1)}</span><span>${fmtSpeed(1.4, 1)}+</span>`;
    note.textContent = 'Depth-averaged velocity |v| with streamlines.';
  } else if (layer === 'hazard') {
    bar.style.background = 'linear-gradient(90deg, #38bdf8 0% 25%, #facc15 25% 50%, #f97316 50% 75%, #e11d48 75% 100%)';
    ticks.innerHTML = '<span>Low</span><span>Mod</span><span>Ext</span>';
    note.textContent = state.profile === 'public'
      ? 'Flood danger level (combines water depth and flow speed).'
      : 'DEFRA FD2320 hazard rating: h × (v + 0.5).';
  } else if (layer === 'arrival') {
    bar.style.background = 'linear-gradient(90deg, #e11d48, #f97316, #facc15, #38bdf8)';
    ticks.innerHTML = '<span>Early</span><span>Mid</span><span>Late</span>';
    note.textContent = 'Flood arrival time (onset ETA from storm start).';
  } else {
    bar.style.background = 'linear-gradient(90deg, #0d9488, #cbd5e1 50%, #e11d48)';
    ticks.innerHTML = `<span>−${fmtDepth(0.3, 1)} Safer</span><span>0</span><span>+${fmtDepth(0.3, 1)} Worse</span>`;
    note.textContent = 'Max depth change: Breached/Edited minus Baseline.';
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
  grid.textContent = 'Loading catalog…';
  const fmtTag = (t) => String(t || '').replace(/_/g, ' ').replace(/\b\w/g, (c) => c.toUpperCase());
  const safeHref = (u) => {
    try {
      const parsed = new URL(String(u));
      return (parsed.protocol === 'https:' || parsed.protocol === 'http:') ? parsed.href : null;
    } catch (_) { return null; }
  };
  try {
    const resp = await fetch(apiUrl('api/v1/catalog/sources'));
    if (!resp.ok) throw new Error('Catalog not configured');
    const data = await resp.json();
    grid.textContent = '';
    const ctx = state.serverContext?.context;
    if (ctx) {
      const banner = document.createElement('div');
      banner.className = 'src-card';
      banner.style.cssText = 'grid-column:1/-1;border-color:var(--accent);background:var(--hud-well)';
      const bTitle = document.createElement('strong');
      const basinTitle = String(ctx.basin_id || '').replace(/(^|[-_])\w/g, (m) => m.toUpperCase()).replace(/[-_]/g, ' ');
      bTitle.textContent = `Active Live Run · Basin: ${basinTitle}${state.serverContext.is_stale ? ' (STALE >12h)' : ''}`;
      const bSub = document.createElement('div');
      bSub.style.color = 'var(--ink-2)';
      bSub.textContent = `Forecast cycle: ${fmtMytDisplay(Date.parse(ctx.cycle_iso), true)} · Terrain: Terrarium 30m DEM`;
      banner.append(bTitle, bSub);
      grid.appendChild(banner);
    }
    for (const s of (data.sources || [])) {
      const card = document.createElement('div');
      card.className = 'src-card';
      const title = document.createElement('strong');
      title.textContent = s.title || s.id || 'Source';
      const badges = document.createElement('div');
      const b1 = document.createElement('span');
      b1.className = `src-badge ${s.access === 'public' ? 'pub' : ''}`;
      b1.textContent = fmtTag(s.access);
      const b2 = document.createElement('span');
      b2.className = 'src-badge';
      b2.textContent = fmtTag(s.data_class);
      badges.append(b1, document.createTextNode(' '), b2);
      if (s.license?.noncommercial_only) {
        const b3 = document.createElement('span');
        b3.className = 'src-badge';
        b3.textContent = 'Noncommercial';
        badges.append(document.createTextNode(' '), b3);
      }
      const meta = document.createElement('div');
      meta.style.color = 'var(--ink-3)';
      meta.textContent = `${s.provider || ''} · ${s.license?.name || ''}`;
      card.append(title, badges, meta);
      const docUrl = safeHref(s.documentation_urls?.[0]);
      if (docUrl) {
        const link = document.createElement('a');
        link.href = docUrl;
        link.target = '_blank';
        link.rel = 'noopener noreferrer';
        link.style.cssText = 'font-size:11px;color:var(--accent);text-decoration:none;margin-top:2px;display:inline-block';
        link.textContent = 'Official docs ↗';
        card.appendChild(link);
      }
      grid.appendChild(card);
    }
  } catch (e) {
    grid.textContent = String(e.message || e);
  }
}

function setCustomPresetLabel(label) {
  const sel = $('preset-select');
  if (!sel) return;
  const clean = String(label || '').replace(/^★\s*/, '').trim();
  const matched = PRESETS.find((p) => p.name.toLowerCase() === clean.toLowerCase());
  if (matched) {
    sel.querySelector('option[value="__custom"]')?.remove();
    sel.value = matched.id;
    return;
  }
  let opt = sel.querySelector('option[value="__custom"]');
  if (!opt) {
    opt = document.createElement('option');
    opt.value = '__custom';
    sel.insertBefore(opt, sel.firstChild);
  }
  opt.textContent = clean;
  sel.value = '__custom';
}

function wireUi() {
  const sel = $('preset-select');
  sel.innerHTML = PRESETS.map((p) => `<option value="${p.id}">${p.name}</option>`).join('');
  sel.addEventListener('change', async () => {
    const p = PRESETS.find((x) => x.id === sel.value);
    if (!p) return;
    if ($('coord-search')) $('coord-search').value = '';
    clearLivePin();
    state.uploadedDem = null;
    state.cachedDem = null;
    state.interventions = [];
    updateInterventionBar();
    if (p.boundaryQuery) {
      await loadBoundaryByQuery(p.boundaryQuery, { runSim: false });
      if (p.liveBasin && await tryLoadPrecomputedLive(p.liveBasin)) return;
      if (state.profile !== 'researcher') await runSimulation(false);
      return;
    }
    clearBoundaryFocus();
    state.center = { lon: p.lon, lat: p.lat };
    map.flyTo({
      center: [p.lon, p.lat],
      zoom: p.zoom,
      pitch: Number.isFinite(p.pitch) ? p.pitch : 0,
      bearing: Number.isFinite(p.bearing) ? p.bearing : 0,
      duration: 900,
    });
    if (p.liveBasin && await tryLoadPrecomputedLive(p.liveBasin)) return;
    if (state.profile !== 'researcher') runSimulation(false);
  });

  $('btn-save-default')?.addEventListener('click', () => {
    if (!map) return;
    const c = map.getCenter();
    const z = map.getZoom();
    const label = $('coord-search')?.value?.trim()
      || $('preset-select')?.selectedOptions?.[0]?.textContent?.trim()
      || `${c.lat.toFixed(4)}, ${c.lng.toFixed(4)}`;
    saveUserPrefs({
      last_center: { lat: Number(c.lat.toFixed(5)), lon: Number(c.lng.toFixed(5)), zoom: Number(z.toFixed(2)), label },
    });
    showToast(`Saved "${label}" as your personal default view.`);
  });

  const searchInput = $('coord-search');
  const searchDropdown = $('search-results');
  let searchTimer = null;
  let searchSeq = 0;
  let searchHits = [];
  let searchActiveIdx = -1;

  const hideSearchDropdown = () => {
    if (searchDropdown) searchDropdown.classList.add('hidden');
    searchActiveIdx = -1;
  };

  const selectSearchHit = async (hit) => {
    if (!hit) return;
    hideSearchDropdown();
    document.body.classList.remove('mob-menu-open');
    const shouldSim = state.profile !== 'public' && state.serverConfig?.features?.allow_simulation !== false;
    const label = hit.name || hit.display_name?.split(',')[0]?.trim() || searchInput.value.trim();
    if (searchInput) searchInput.value = label;
    const geo = hit.geojson;
    const isAdminPoly = (hit.class === 'boundary' || hit.type === 'administrative')
      && geo && (geo.type === 'Polygon' || geo.type === 'MultiPolygon');
    if (isAdminPoly) {
      state.activeBoundaryName = label;
      setCustomPresetLabel(label);
      await applyBoundaryGeometry(geo, shouldSim);
      if (state.profile === 'public') {
        const matched = PRESETS.find((p) => p.liveBasin && (
          p.name.toLowerCase() === label.toLowerCase()
          || (label.toLowerCase() === 'malacca' && p.name.toLowerCase() === 'melaka')
        ));
        if (matched?.liveBasin) await tryLoadPrecomputedLive(matched.liveBasin);
      }
      return;
    }
    const lat = parseFloat(hit.lat);
    const lon = parseFloat(hit.lon);
    if (!Number.isFinite(lat) || !Number.isFinite(lon)) return;
    clearBoundaryFocus();
    state.center = { lat, lon };
    state.cachedDem = null;
    setCustomPresetLabel(label);
    map.flyTo({ center: [lon, lat], zoom: 14.2, duration: 900 });
    if (shouldSim) runSimulation(false);
  };

  const renderSearchDropdown = (hits, statusMsg = '') => {
    if (!searchDropdown) return;
    searchDropdown.replaceChildren();
    searchHits = hits;
    searchActiveIdx = -1;
    if (statusMsg) {
      const msg = document.createElement('div');
      msg.className = 'search-result-empty';
      msg.textContent = statusMsg;
      searchDropdown.appendChild(msg);
      searchDropdown.classList.remove('hidden');
      return;
    }
    if (!hits.length) {
      const empty = document.createElement('div');
      empty.className = 'search-result-empty';
      empty.textContent = 'No matching places found.';
      searchDropdown.appendChild(empty);
      searchDropdown.classList.remove('hidden');
      return;
    }
    hits.forEach((hit, i) => {
      const btn = document.createElement('button');
      btn.type = 'button';
      btn.className = 'search-result-item';
      btn.dataset.idx = String(i);
      const primary = hit.name || hit.display_name?.split(',')[0]?.trim() || 'Location';
      const rest = (hit.display_name || '')
        .split(',')
        .map((s) => s.trim())
        .filter((s, idx) => idx > 0 && s)
        .slice(0, 3)
        .join(', ');
      const nameEl = document.createElement('div');
      nameEl.className = 'search-result-name';
      nameEl.textContent = primary;
      btn.appendChild(nameEl);
      if (rest) {
        const subEl = document.createElement('div');
        subEl.className = 'search-result-sub';
        subEl.textContent = rest;
        btn.appendChild(subEl);
      }
      btn.addEventListener('click', (ev) => {
        ev.preventDefault();
        selectSearchHit(hit);
      });
      searchDropdown.appendChild(btn);
    });
    searchDropdown.classList.remove('hidden');
  };

  const fetchSearchSuggestions = async (raw) => {
    const q = raw.trim();
    if (q.length < 3) {
      hideSearchDropdown();
      return;
    }
    const seq = ++searchSeq;
    const sKey = `search6:${q.toLowerCase()}`;
    let hits = readNominatimCache(sKey);
    if (!Array.isArray(hits)) {
      renderSearchDropdown([], 'Searching…');
      try {
        let url = `https://nominatim.openstreetmap.org/search?format=json&addressdetails=1&polygon_geojson=1&polygon_threshold=0.005&limit=6&q=${encodeURIComponent(q)}`;
        const ab = state.serverConfig?.alert_bounds;
        if (Array.isArray(ab) && ab.length === 4) {
          url += `&viewbox=${ab[0]},${ab[3]},${ab[2]},${ab[1]}`;
        }
        const resp = await fetch(url);
        if (!resp.ok) throw new Error(`HTTP ${resp.status}`);
        const data = await resp.json();
        hits = Array.isArray(data) ? data : [];
        if (hits.length) writeNominatimCache(sKey, hits);
      } catch (_) {
        if (seq === searchSeq) renderSearchDropdown([], 'Search temporarily unavailable.');
        return;
      }
    }
    if (seq !== searchSeq) return;
    renderSearchDropdown(hits);
  };

  searchInput?.addEventListener('input', (e) => {
    const raw = e.target.value.trim();
    if (searchTimer) clearTimeout(searchTimer);
    const parts = raw.split(',').map((v) => parseFloat(v.trim()));
    if (parts.length === 2 && Number.isFinite(parts[0]) && Number.isFinite(parts[1])) {
      hideSearchDropdown();
      return;
    }
    if (raw.length < 3) {
      hideSearchDropdown();
      return;
    }
    searchTimer = setTimeout(() => fetchSearchSuggestions(raw), 350);
  });

  searchInput?.addEventListener('keydown', async (e) => {
    if (e.key === 'Escape') {
      hideSearchDropdown();
      return;
    }
    if (e.key === 'ArrowDown' && searchHits.length > 0 && !searchDropdown?.classList.contains('hidden')) {
      e.preventDefault();
      searchActiveIdx = (searchActiveIdx + 1) % searchHits.length;
      searchDropdown.querySelectorAll('.search-result-item').forEach((el, i) => el.classList.toggle('active', i === searchActiveIdx));
      return;
    }
    if (e.key === 'ArrowUp' && searchHits.length > 0 && !searchDropdown?.classList.contains('hidden')) {
      e.preventDefault();
      searchActiveIdx = (searchActiveIdx - 1 + searchHits.length) % searchHits.length;
      searchDropdown.querySelectorAll('.search-result-item').forEach((el, i) => el.classList.toggle('active', i === searchActiveIdx));
      return;
    }
    if (e.key !== 'Enter') return;
    e.preventDefault();
    if (!searchDropdown?.classList.contains('hidden') && searchHits.length > 0) {
      selectSearchHit(searchHits[searchActiveIdx >= 0 ? searchActiveIdx : 0]);
      return;
    }
    const raw = e.target.value.trim();
    if (!raw) return;
    const parts = raw.split(',').map((v) => parseFloat(v.trim()));
    if (parts.length === 2 && Number.isFinite(parts[0]) && Number.isFinite(parts[1])) {
      hideSearchDropdown();
      clearBoundaryFocus();
      state.center = { lat: parts[0], lon: parts[1] };
      state.cachedDem = null;
      setCustomPresetLabel(`Custom · ${parts[0].toFixed(4)}, ${parts[1].toFixed(4)}`);
      map.flyTo({ center: [parts[1], parts[0]], zoom: 14.2, duration: 800 });
      if (state.profile !== 'public' && state.serverConfig?.features?.allow_simulation !== false) {
        runSimulation(false);
      }
      return;
    }
    if (searchTimer) clearTimeout(searchTimer);
    await fetchSearchSuggestions(raw);
  });

  document.addEventListener('click', (e) => {
    if (!e.target.closest('.search-input-wrap')) hideSearchDropdown();
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
    state.displayPrefs = {
      showFlood: true, showRivers: true, showBoundary: true, showMarkers: true,
      showParticles: true, is3D: true, showLabels: true, basemap: 'topo', layer: 'depth',
      depthCutoffM: 0.06, floodOpacity: 0.95, riverOpacity: 0.72,
      units: 'metric', clock: state.displayPrefs.clock || '12h',
      timezone: state.displayPrefs.timezone || 'local',
      showMetrics: true, showLegend: true,
    };
    saveUserPrefs({ displayPrefs: state.displayPrefs });
    populateLayersModal();
    applyDisplayPrefsToMap();
    $('layers-modal').classList.add('hidden');
  });
  $('opt-flood-opacity').addEventListener('input', (e) => { $('val-flood-opacity').textContent = `${e.target.value}%`; });
  $('opt-river-opacity').addEventListener('input', (e) => { $('val-river-opacity').textContent = `${e.target.value}%`; });

  $('btn-open-sim').addEventListener('click', openSimModal);
  $('btn-sim-close').addEventListener('click', () => $('sim-modal').classList.add('hidden'));
  $('btn-sim-cancel').addEventListener('click', () => $('sim-modal').classList.add('hidden'));
  if ($('opt-cutoff')) {
    $('opt-cutoff').addEventListener('input', (e) => {
      const cm = Math.round((parseFloat(e.target.value) || 0.08) * 100);
      if ($('val-cutoff')) $('val-cutoff').textContent = `${cm} cm`;
    });
  }
  $('btn-sim-apply').addEventListener('click', () => applySimModalDraft(false));
  $('btn-sim-apply-run')?.addEventListener('click', () => applySimModalDraft(true));
  $('btn-sim-fork')?.addEventListener('click', () => {
    state.draftParentRunId = state.activeRunId || null;
    state.draftParentSeq = state.activeRunSeq || null;
    setSimPhase('draft');
    const fromTxt = state.draftParentSeq ? ` (from #${state.draftParentSeq})` : '';
    showToast(`Cloned into new draft${fromTxt} — modify parameters or bunds, then click Run Simulation.`);
  });
  $('btn-sim-new')?.addEventListener('click', () => {
    startCleanSimDraft();
  });
  $('btn-dir-close')?.addEventListener('click', () => $('sim-dir-drawer')?.classList.add('hidden'));
  $('btn-dir-new')?.addEventListener('click', () => {
    $('sim-dir-drawer')?.classList.add('hidden');
    startCleanSimDraft();
  });
  $('btn-dir-save-draft')?.addEventListener('click', () => saveDraftConfigToDir());
  document.querySelectorAll('[data-dir-filter]').forEach((btn) => {
    btn.addEventListener('click', () => {
      state.simDirFilter = btn.dataset.dirFilter || 'all';
      document.querySelectorAll('[data-dir-filter]').forEach((b) => b.classList.toggle('active', b === btn));
      renderSimDirList();
    });
  });
  $('btn-mob-tools')?.addEventListener('click', () => {
    const opening = !document.body.classList.contains('mob-tools-open');
    document.body.classList.remove('mob-summary-open', 'mob-layers-open', 'mob-menu-open');
    document.body.classList.toggle('mob-tools-open', opening);
  });
  if ($('sim-storm-drain')) {
    $('sim-storm-drain').addEventListener('input', (e) => {
      updateStormDrainLabel(parseInt(e.target.value, 10) || 0);
    });
  }
  if ($('sim-storm-drain-file')) {
    $('sim-storm-drain-file').addEventListener('change', async (e) => {
      const file = e.target.files?.[0];
      if (!file) return;
      try {
        const parsed = JSON.parse(await file.text());
        const arr = Array.isArray(parsed) ? parsed : (Array.isArray(parsed?.values) ? parsed.values : null);
        if (!arr || arr.length < 16) {
          throw new Error('Expected JSON array of 0..1 drain capacity scores');
        }
        state.stormDrainGrid = arr.map((v) => Math.min(1, Math.max(0, Number(v) || 0)));
        updateStormDrainGridStatus();
      } catch (err) {
        showToast(`Drain grid error: ${err.message}`);
      }
      e.target.value = '';
    });
  }
  if ($('sim-storm-drain-clear')) {
    $('sim-storm-drain-clear').addEventListener('click', () => {
      state.stormDrainGrid = null;
      updateStormDrainGridStatus();
    });
  }
  $('btn-sim-reset').addEventListener('click', () => {
    state.simConfig = {
      mode: 'forecast', horizon: '5d', rainMult: 1.0, stressMmDay: 150, stressDays: 3, stressPadM: 0,
      rainfallMmHr: 280, durationS: 360, hyetograph: 'peak-storm', soil: 'impervious',
      boundary: 'transmissive', manningN: 0.035, windX: 0, startDry: false,
      enableEvap: true, enableDrain: true, tideMode: 'live', compute: 'auto', stormDrainScore: 0.5,
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
  $('btn-sv-close').addEventListener('click', clearLivePin);
  $('sv-ffe-select').addEventListener('change', () => renderSiteVerdict(false));
  $('btn-sv-raise').addEventListener('click', () => {
    if (!state.verdictTarget) return;
    state.stressSite = { lat: state.verdictTarget.lat, lon: state.verdictTarget.lon };
    state.simConfig.mode = 'stress';
    state.simConfig.stressPadM = 0.8;
    clearLivePin();
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
  const cycleSpeed = () => {
    state.speed = state.speed === 0.5 ? 1 : state.speed === 1 ? 2 : state.speed === 2 ? 5 : 0.5;
    if ($('btn-speed')) $('btn-speed').textContent = `${state.speed}x`;
    if ($('fb-speed')) $('fb-speed').textContent = `${state.speed}x`;
  };
  if ($('fb-speed')) $('fb-speed').addEventListener('click', cycleSpeed);
  if ($('fb-track')) {
    const scrubForecast = (ev) => {
      if (!state.frames.length) return;
      const rect = $('fb-track').getBoundingClientRect();
      const frac = Math.min(1, Math.max(0, (ev.clientX - rect.left) / Math.max(1, rect.width)));
      state.playing = false;
      if ($('btn-play')) $('btn-play').textContent = '▶';
      if ($('fb-play')) $('fb-play').textContent = '▶';
      const nextIdx = Math.round(frac * (state.frames.length - 1));
      if (nextIdx !== state.frameIdx) {
        state.frameIdx = nextIdx;
        $('scrubber').value = String(state.frameIdx);
        updateTimeReadout();
        renderFloodCanvas(true);
      }
    };
    let dragging = false;
    const startDrag = (e) => {
      e.preventDefault();
      dragging = true;
      try { e.currentTarget.setPointerCapture(e.pointerId); } catch (_) {}
      scrubForecast(e);
    };
    const moveDrag = (e) => {
      if (!dragging) return;
      e.preventDefault();
      scrubForecast(e);
    };
    const endDrag = () => { dragging = false; };
    for (const target of [$('fb-track'), $('fb-callout')].filter(Boolean)) {
      target.addEventListener('pointerdown', startDrag);
      target.addEventListener('pointermove', moveDrag);
      target.addEventListener('pointerup', endDrag);
      target.addEventListener('pointercancel', endDrag);
    }
    window.addEventListener('pointermove', moveDrag);
    window.addEventListener('pointerup', endDrag);
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

  ['sources-modal', 'layers-modal', 'sim-modal', 'saved-sims-modal', 'rec-preview-modal', 'app-settings-modal', 'about-modal'].forEach((id) => {
    const el = $(id);
    if (el) el.addEventListener('click', (e) => {
      if (e.target === el) {
        if (id === 'rec-preview-modal') closeRecPreviewModal();
        else el.classList.add('hidden');
      }
    });
  });
  $('btn-mobile-menu')?.addEventListener('click', (e) => {
    e.stopPropagation();
    const open = document.body.classList.toggle('mob-menu-open');
    if (open) {
      document.body.classList.remove('mob-summary-open', 'mob-layers-open', 'mob-tools-open');
      $('alerts-dropdown')?.classList.add('hidden');
    }
  });
  $('btn-mob-summary')?.addEventListener('click', (e) => {
    e.stopPropagation();
    const open = document.body.classList.toggle('mob-summary-open');
    if (open) {
      document.body.classList.remove('mob-menu-open', 'mob-layers-open', 'mob-tools-open');
      $('alerts-dropdown')?.classList.add('hidden');
    }
  });
  $('btn-mob-layers')?.addEventListener('click', (e) => {
    e.stopPropagation();
    const open = document.body.classList.toggle('mob-layers-open');
    if (open) {
      document.body.classList.remove('mob-menu-open', 'mob-summary-open', 'mob-tools-open');
      $('alerts-dropdown')?.classList.add('hidden');
    }
  });
  document.querySelectorAll('.top-actions button').forEach((b) => {
    b.addEventListener('click', () => document.body.classList.remove('mob-menu-open'));
  });
  $('btn-focus-clear')?.addEventListener('click', clearBoundaryFocus);
  $('btn-app-settings')?.addEventListener('click', () => openAppSettingsModal(false));
  $('btn-save-view')?.addEventListener('click', () => openAppSettingsModal(true));
  $('btn-app-settings-close')?.addEventListener('click', () => $('app-settings-modal')?.classList.add('hidden'));
  $('opt-timezone')?.addEventListener('change', (e) => {
    state.displayPrefs.timezone = e.target.value || 'local';
    saveUserPrefs({ displayPrefs: state.displayPrefs });
    renderForecastSegments();
    updateTimeReadout();
    showToast(`Timezone set to ${state.displayPrefs.timezone === 'local' ? `Browser Local (${getBrowserTimeZone()})` : state.displayPrefs.timezone}.`);
  });
  $('opt-settings-clock')?.addEventListener('change', (e) => {
    state.displayPrefs.clock = e.target.value === '24h' ? '24h' : '12h';
    if ($('opt-clock')) $('opt-clock').value = state.displayPrefs.clock;
    saveUserPrefs({ displayPrefs: state.displayPrefs });
    updateTimeReadout();
  });
  document.querySelectorAll('[data-settings-tab]').forEach((btn) => {
    btn.addEventListener('click', () => {
      const tab = btn.dataset.settingsTab;
      document.querySelectorAll('[data-settings-tab]').forEach((b) => b.classList.toggle('active', b === btn));
      $('settings-pane-locations')?.classList.toggle('hidden', tab !== 'locations');
      $('settings-pane-general')?.classList.toggle('hidden', tab !== 'general');
    });
  });
  $('loc-kind')?.addEventListener('change', () => {
    const isBoundary = $('loc-kind').value === 'boundary';
    $('loc-point-fields')?.classList.toggle('hidden', isBoundary);
    $('loc-boundary-fields')?.classList.toggle('hidden', !isBoundary);
  });
  $('btn-loc-use-view')?.addEventListener('click', () => {
    if (!map) return;
    const c = map.getCenter();
    if ($('loc-lat')) $('loc-lat').value = c.lat.toFixed(4);
    if ($('loc-lon')) $('loc-lon').value = c.lng.toFixed(4);
    if ($('loc-zoom')) $('loc-zoom').value = map.getZoom().toFixed(1);
    if ($('loc-pitch')) $('loc-pitch').value = String(Math.round(map.getPitch()));
    if ($('loc-bearing')) $('loc-bearing').value = String(Math.round(map.getBearing()));
  });
  $('btn-loc-add')?.addEventListener('click', addOrUpdateCustomLocation);
  $('btn-loc-new')?.addEventListener('click', () => {
    editingCustomLocId = null;
    if ($('loc-name')) $('loc-name').value = '';
    if ($('loc-query')) $('loc-query').value = '';
    if ($('loc-lat')) $('loc-lat').value = '';
    if ($('loc-lon')) $('loc-lon').value = '';
    if ($('loc-zoom')) $('loc-zoom').value = '11.5';
    if ($('loc-pitch')) $('loc-pitch').value = '0';
    if ($('loc-bearing')) $('loc-bearing').value = '0';
    setLocFormDisabled(false);
    $('loc-name')?.focus();
  });
  $('btn-settings-reset-default')?.addEventListener('click', () => {
    saveUserPrefs({ default_location_id: null, last_center: null });
    renderManageLocationsList();
    showToast('Reset default startup view to server default.');
  });
  $('btn-settings-clear-locs')?.addEventListener('click', () => {
    saveCustomLocations([]);
    rebuildPresetDropdown();
    renderManageLocationsList();
    showToast('Cleared all custom locations.');
  });
  $('btn-alerts')?.addEventListener('click', (e) => {
    e.stopPropagation();
    document.body.classList.remove('mob-menu-open', 'mob-summary-open', 'mob-layers-open');
    $('alerts-dropdown')?.classList.toggle('hidden');
  });
  document.addEventListener('click', (e) => {
    const dd = $('alerts-dropdown');
    if (dd && !dd.classList.contains('hidden') && !e.target.closest('.alerts-wrap')) {
      dd.classList.add('hidden');
    }
  });
  $('btn-rec-close')?.addEventListener('click', closeRecPreviewModal);
  $('btn-rec-dismiss')?.addEventListener('click', closeRecPreviewModal);
  $('btn-saved-sims')?.addEventListener('click', openSavedSimsModal);
  $('btn-saved-close')?.addEventListener('click', () => $('saved-sims-modal')?.classList.add('hidden'));
  $('btn-saved-done')?.addEventListener('click', () => $('saved-sims-modal')?.classList.add('hidden'));
  $('btn-save-config')?.addEventListener('click', () => saveCurrentToIdb('config'));
  $('btn-save-sim')?.addEventListener('click', () => saveCurrentToIdb('simulation'));
  refreshSavedCountBadge();

  window.addEventListener('keydown', (e) => {
    if (state.simPhase === 'solving') {
      if (e.key === 'Escape' && state.activeAbort) {
        state.activeAbort.abort();
      }
      e.preventDefault();
      return;
    }
    const tag = (e.target && e.target.tagName || '').toLowerCase();
    const typing = tag === 'input' || tag === 'select' || tag === 'textarea';
    if (e.key === 'Escape') {
      ['sources-modal', 'layers-modal', 'sim-modal', 'saved-sims-modal', 'app-settings-modal', 'about-modal'].forEach((id) => { const el = $(id); if (el) el.classList.add('hidden'); });
      $('sim-dir-drawer')?.classList.add('hidden');
      $('alerts-dropdown')?.classList.add('hidden');
      closeRecPreviewModal();
      setPinDropMode(false);
      if (state.pickingStressSite) {
        state.pickingStressSite = false;
        document.body.classList.remove('picking-site');
        showToast('Site pick cancelled.');
      } else if (state.tool !== 'inspect' && (state.profile !== 'researcher' || state.simPhase === 'results')) {
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
    $('btn-env').textContent = state.envelopeMode ? 'Peak' : 'Step';
    $('btn-env').classList.toggle('active', state.envelopeMode);
    updateLiveMetrics();
    renderFloodCanvas(true);
  });

  $('btn-particles').addEventListener('click', () => {
    state.particlesOn = !state.particlesOn;
    state.displayPrefs.showParticles = state.particlesOn;
    $('btn-particles').classList.toggle('active', state.particlesOn);
    if (map && layersReady && map.getLayer('arus-particles-line')) {
      map.setLayoutProperty('arus-particles-line', 'visibility', state.particlesOn && state.displayPrefs.showFlood ? 'visible' : 'none');
    }
    saveUserPrefs({ displayPrefs: state.displayPrefs });
    renderFloodCanvas(true);
  });

  $('btn-play').addEventListener('click', () => {
    state.playing = !state.playing;
    $('btn-play').textContent = state.playing ? '❚❚' : '▶';
  });

  $('btn-speed').addEventListener('click', () => {
    state.speed = state.speed === 0.5 ? 1 : state.speed === 1 ? 2 : state.speed === 2 ? 5 : 0.5;
    $('btn-speed').textContent = `${state.speed}x`;
    if ($('fb-speed')) $('fb-speed').textContent = `${state.speed}x`;
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
  $('btn-iv-clear').addEventListener('click', () => {
    state.interventions = [];
    state.baselineInterventions = [];
    state.lastBundCell = null;
    updateInterventionBar();
    renderFloodCanvas(false);
  });
  if ($('btn-iv-breach')) {
    $('btn-iv-breach').addEventListener('click', () => {
      if (state.interventions.length === 0) return;
      const mid = state.interventions[Math.floor(state.interventions.length / 2)];
      const [minLon, minLat, maxLon, maxLat] = state.grid.metadata.wgs84_bounds;
      const lon = minLon + ((mid.col + 0.5) / state.grid.metadata.cols) * (maxLon - minLon);
      const lat = minLat + ((mid.row + 0.5) / state.grid.metadata.rows) * (maxLat - minLat);
      breachBund(lat, lon, 2);
      showToast('Punched breach gap in bund. Click Compare vs Baseline to view Breach Impact Δh.');
    });
  }
  if ($('btn-fp-clear')) {
    $('btn-fp-clear').addEventListener('click', () => {
      state.observedGauges = [];
      updateFloodPinBar();
      showToast('Cleared observed flood pins.');
    });
  }
  $('btn-run').addEventListener('click', () => runSimulation(false));
  $('btn-storm-test')?.addEventListener('click', () => {
    state.simConfig.mode = 'forecast';
    state.simConfig.rainfallMmHr = 220;
    state.simConfig.durationS = 600;
    state.spatialRainGrid = null;
    state.spanHours = 0;
    runSimulation(state.interventions.length > 0);
  });
  $('btn-compare').addEventListener('click', () => runSimulation(true));
  $('btn-nwp').addEventListener('click', fetchLiveMultiModelNwp);
  $('btn-nwp-apply').addEventListener('click', () => {
    if (state.nwpPeakMmHr !== null) {
      const clamped = Math.min(400, Math.max(40, Math.round(state.nwpPeakMmHr / 10) * 10));
      state.simConfig.rainfallMmHr = clamped;
      updateSimDraftSummary();
      showToast(`Applied ${clamped} mm/hr NWP peak to draft — click Run Simulation when ready.`);
    }
  });

  $('btn-export-asc').addEventListener('click', () => {
    if (!state.grid) return;
    if (state.runId && !state.runId.startsWith('local-')) {
      window.open(apiUrl(`api/v1/runs/${state.runId}/export/max-depth.asc`), '_blank');
      return;
    }
    const m = state.grid.metadata;
    const maxD = state.grid.envelope?.max_depth_m || state.frames[state.frames.length - 1]?.depth || [];
    const lines = [
      `ncols ${m.cols}`,
      `nrows ${m.rows}`,
      `xllcorner ${m.wgs84_bounds[0].toFixed(6)}`,
      `yllcorner ${m.wgs84_bounds[1].toFixed(6)}`,
      `cellsize ${((m.wgs84_bounds[2] - m.wgs84_bounds[0]) / m.cols).toFixed(8)}`,
      'NODATA_value -9999',
    ];
    for (let r = m.rows - 1; r >= 0; r--) {
      const row = [];
      for (let c = 0; c < m.cols; c++) {
        const i = r * m.cols + c;
        row.push(m.active_mask[i] ? (maxD[i] || 0).toFixed(4) : '-9999');
      }
      lines.push(row.join(' '));
    }
    downloadBlob(new Blob([lines.join('\n')], { type: 'text/plain;charset=utf-8' }), `arus-${state.runId || 'max-depth'}.asc`);
  });
  $('btn-export-csv').addEventListener('click', () => {
    if (!state.grid || !state.frames.length) return;
    if (state.runId && !state.runId.startsWith('local-')) {
      window.open(apiUrl(`api/v1/runs/${state.runId}/export/outlooks.csv`), '_blank');
      return;
    }
    const m = state.grid.metadata;
    const cut = state.displayPrefs.depthCutoffM || 0.08;
    const cellKm2 = (m.dx_m * m.dx_m) / 1_000_000;
    const rows = ['time_s,flooded_cells,flooded_area_km2,avg_depth_m,peak_depth_m'];
    for (const f of state.frames) {
      let n = 0, flooded = 0, sum = 0, peak = 0;
      for (let i = 0; i < f.depth.length; i++) {
        if (!m.active_mask[i]) continue;
        n++;
        const h = f.depth[i] || 0;
        sum += h;
        if (h >= cut) flooded++;
        if (h > peak) peak = h;
      }
      rows.push(`${f.time_s.toFixed(1)},${flooded},${(flooded * cellKm2).toFixed(4)},${(n ? sum / n : 0).toFixed(4)},${peak.toFixed(4)}`);
    }
    downloadBlob(new Blob([rows.join('\n')], { type: 'text/csv;charset=utf-8' }), `arus-${state.runId || 'outlooks'}.csv`);
  });
  $('btn-snap').addEventListener('click', captureSnapshotPng);
  $('btn-rec').addEventListener('click', toggleVideoRecording);

  $('btn-sources').addEventListener('click', openSourcesModal);
  $('btn-sources-close').addEventListener('click', () => $('sources-modal').classList.add('hidden'));
  const dismissAbout = () => {
    if ($('about-modal')) $('about-modal').classList.add('hidden');
    try { window.localStorage.setItem('arus.about.ack.v1', '1'); } catch (_) {}
  };
  $('btn-about')?.addEventListener('click', () => $('about-modal')?.classList.remove('hidden'));
  $('btn-about-close')?.addEventListener('click', dismissAbout);
  $('btn-about-ok')?.addEventListener('click', dismissAbout);
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
    updateSimDraftSummary();
    showToast(`Loaded "${file.name}" into draft — click Run Simulation when ready.`);
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

let snapInFlight = false;
function captureSnapshotPng() {
  if (!map || snapInFlight) return;
  snapInFlight = true;
  const snapBtn = $('btn-snap');
  if (snapBtn) snapBtn.disabled = true;
  map.once('render', () => {
    try {
      const src = map.getCanvas();
      const out = document.createElement('canvas');
      out.width = src.width;
      out.height = src.height;
      const ctx = out.getContext('2d');
      ctx.fillStyle = '#f8fafc';
      ctx.fillRect(0, 0, out.width, out.height);
      ctx.drawImage(src, 0, 0);
      drawRecordingHudOverlay(ctx, out.width, out.height);

      const title = $('po-title')?.textContent?.trim()
        || $('preset-select')?.selectedOptions?.[0]?.textContent?.trim()
        || 'view';
      const frameMs = getCurrentFrameEpochMs();
      const slug = title.toLowerCase().replace(/[^a-z0-9]+/g, '-').replace(/^-|-$/g, '').slice(0, 32) || 'view';
      const stamp = fmtMytSlug(frameMs);
      const filename = `arus-${slug}-${stamp}.png`;
      out.toBlob((blob) => {
        snapInFlight = false;
        if (snapBtn) snapBtn.disabled = false;
        if (blob) {
          downloadBlob(blob, filename);
          showToast(`PNG image downloaded (${filename}).`);
        } else {
          showToast('PNG export failed.');
        }
      }, 'image/png');
    } catch (_) {
      snapInFlight = false;
      if (snapBtn) snapBtn.disabled = false;
      showToast('PNG export failed.');
    }
  });
  map.triggerRepaint();
}

let activeRecRepaintTimer = null;
let lastRecBlobUrl = null;

function openRecPreviewModal(blob, filename) {
  if (lastRecBlobUrl) {
    try { URL.revokeObjectURL(lastRecBlobUrl); } catch (_) {}
  }
  lastRecBlobUrl = URL.createObjectURL(blob);
  const vid = $('rec-preview-video');
  const dl = $('btn-rec-download');
  const meta = $('rec-preview-meta');
  if (vid) {
    vid.src = lastRecBlobUrl;
    vid.currentTime = 0;
    vid.play().catch(() => {});
  }
  if (dl) {
    dl.href = lastRecBlobUrl;
    dl.download = filename;
    dl.onclick = () => showToast(`Video downloaded (${filename}).`);
  }
  if (meta) {
    const mb = (blob.size / (1024 * 1024)).toFixed(2);
    meta.textContent = `${filename} · ${mb} MB · Click Download Video to save where you choose.`;
  }
  $('rec-preview-modal')?.classList.remove('hidden');
}

function closeRecPreviewModal() {
  const vid = $('rec-preview-video');
  if (vid) { try { vid.pause(); } catch (_) {} }
  $('rec-preview-modal')?.classList.add('hidden');
}

function drawRecordingHudOverlay(ctx, W, H) {
  const dpr = window.devicePixelRatio || 1;
  const cssW = W / dpr;
  const s = dpr * Math.max(0.68, Math.min(1.0, cssW / 960));
  const pad = Math.round(16 * s);
  const title = $('po-title')?.textContent?.trim()
    || $('preset-select')?.selectedOptions?.[0]?.textContent?.trim()
    || 'Flood Outlook';
  const frameMs = getCurrentFrameEpochMs();
  const stamp = fmtMytDisplay(frameMs, true);
  const avgTxt = $('lm-avg')?.textContent?.trim() || '0.00 m';
  const peakTxt = $('lm-peak')?.textContent?.trim() || '0.00 m';
  const pctTxt = $('lm-pct')?.textContent?.trim() || '0%';
  const areaTxt = $('lm-stagnant')?.textContent?.trim() || '0.0 km²';
  const riskTxt = $('lm-badge')?.textContent?.trim() || 'LOW';
  const trendTxt = $('lm-trend')?.textContent?.trim() || 'Current';
  const markerLeftPct = parseFloat($('lm-marker')?.style?.left || '0') || 0;
  const riskBg = riskTxt === 'DANGER' ? 'rgba(220, 38, 38, 0.16)'
    : riskTxt === 'HIGH' ? 'rgba(234, 88, 12, 0.16)'
    : riskTxt === 'MODERATE' ? 'rgba(217, 119, 6, 0.16)'
    : 'rgba(16, 185, 129, 0.16)';
  const riskFg = riskTxt === 'DANGER' ? '#b91c1c'
    : riskTxt === 'HIGH' ? '#c2410c'
    : riskTxt === 'MODERATE' ? '#b45309'
    : '#047857';

  ctx.save();
  const cardW = Math.round(286 * s);

  // 1. Top-left location + live MYT date/time card
  const headH = Math.round(58 * s);
  ctx.fillStyle = 'rgba(255, 255, 255, 0.93)';
  ctx.beginPath();
  ctx.roundRect(pad, pad, cardW, headH, Math.round(10 * s));
  ctx.fill();
  ctx.strokeStyle = 'rgba(15, 23, 42, 0.12)';
  ctx.lineWidth = Math.max(1, s);
  ctx.stroke();

  ctx.fillStyle = '#0f172a';
  ctx.font = `700 ${Math.round(14 * s)}px Inter, system-ui, sans-serif`;
  ctx.fillText(`ARUS · ${title}`.slice(0, 30), pad + Math.round(12 * s), pad + Math.round(23 * s));

  ctx.fillStyle = '#0284c7';
  ctx.font = `600 ${Math.round(12 * s)}px "JetBrains Mono", monospace`;
  ctx.fillText(stamp, pad + Math.round(12 * s), pad + Math.round(44 * s));

  // 2. Area Flood Index card (matching the live HUD gauge + 2x2 KPIs)
  const afiY = pad + headH + Math.round(10 * s);
  const afiH = Math.round(166 * s);
  ctx.fillStyle = 'rgba(255, 255, 255, 0.93)';
  ctx.beginPath();
  ctx.roundRect(pad, afiY, cardW, afiH, Math.round(10 * s));
  ctx.fill();
  ctx.strokeStyle = 'rgba(15, 23, 42, 0.12)';
  ctx.stroke();

  const ix = pad + Math.round(12 * s);
  const iw = cardW - Math.round(24 * s);
  ctx.fillStyle = '#0f172a';
  ctx.font = `700 ${Math.round(12.5 * s)}px Inter, system-ui, sans-serif`;
  ctx.fillText('Area Flood Index', ix, afiY + Math.round(22 * s));

  // Severity badge on right
  const bW = Math.round(54 * s);
  const bH = Math.round(18 * s);
  const bX = ix + iw - bW;
  const bY = afiY + Math.round(9 * s);
  ctx.fillStyle = riskBg;
  ctx.beginPath();
  ctx.roundRect(bX, bY, bW, bH, Math.round(5 * s));
  ctx.fill();
  ctx.fillStyle = riskFg;
  ctx.font = `700 ${Math.round(9.5 * s)}px Inter, system-ui, sans-serif`;
  ctx.textAlign = 'center';
  ctx.fillText(riskTxt, bX + bW / 2, bY + Math.round(12.5 * s));

  // Trend label left of badge
  ctx.fillStyle = trendTxt.includes('Rising') ? '#dc2626' : (trendTxt.includes('Receding') ? '#059669' : '#64748b');
  ctx.font = `600 ${Math.round(10.5 * s)}px Inter, system-ui, sans-serif`;
  ctx.textAlign = 'right';
  ctx.fillText(trendTxt, bX - Math.round(8 * s), afiY + Math.round(22 * s));
  ctx.textAlign = 'left';

  // Gauge gradient bar + live marker dot
  const gY = afiY + Math.round(38 * s);
  const gH = Math.round(7 * s);
  const gGrad = ctx.createLinearGradient(ix, 0, ix + iw, 0);
  gGrad.addColorStop(0, '#10b981');
  gGrad.addColorStop(0.35, '#eab308');
  gGrad.addColorStop(0.7, '#f97316');
  gGrad.addColorStop(1, '#ef4444');
  ctx.fillStyle = gGrad;
  ctx.beginPath();
  ctx.roundRect(ix, gY, iw, gH, Math.round(4 * s));
  ctx.fill();

  const mkX = ix + Math.max(0, Math.min(1, markerLeftPct / 100)) * iw;
  ctx.fillStyle = '#ffffff';
  ctx.beginPath();
  ctx.arc(mkX, gY + gH / 2, Math.round(5.5 * s), 0, Math.PI * 2);
  ctx.fill();
  ctx.strokeStyle = '#0f172a';
  ctx.lineWidth = Math.max(1.5, 2 * s);
  ctx.stroke();

  ctx.fillStyle = '#64748b';
  ctx.font = `500 ${Math.round(9.5 * s)}px Inter, system-ui, sans-serif`;
  ctx.fillText('Low', ix, gY + Math.round(20 * s));
  ctx.fillText('Mod', ix + Math.round(iw * 0.30), gY + Math.round(20 * s));
  ctx.fillText('High', ix + Math.round(iw * 0.62), gY + Math.round(20 * s));
  ctx.textAlign = 'right';
  ctx.fillText('Danger', ix + iw, gY + Math.round(20 * s));
  ctx.textAlign = 'left';

  // 2x2 KPI boxes (Avg Depth, Peak Depth, Flooded Area, Flooded Extent)
  const kpis = [
    ['Avg Depth', avgTxt],
    ['Peak Depth', peakTxt],
    ['Flooded Area', pctTxt],
    ['Flooded Extent', areaTxt],
  ];
  const cellGap = Math.round(7 * s);
  const cellW = Math.round((iw - cellGap) / 2);
  const cellH = Math.round(42 * s);
  const gridY = gY + Math.round(29 * s);
  ctx.lineWidth = Math.max(1, s);
  for (let i = 0; i < 4; i++) {
    const col = i % 2;
    const row = Math.floor(i / 2);
    const cx = ix + col * (cellW + cellGap);
    const cy = gridY + row * (cellH + cellGap);
    ctx.fillStyle = 'rgba(241, 245, 249, 0.85)';
    ctx.beginPath();
    ctx.roundRect(cx, cy, cellW, cellH, Math.round(7 * s));
    ctx.fill();
    ctx.strokeStyle = 'rgba(15, 23, 42, 0.08)';
    ctx.stroke();
    ctx.fillStyle = '#64748b';
    ctx.font = `500 ${Math.round(9.5 * s)}px Inter, system-ui, sans-serif`;
    ctx.fillText(kpis[i][0], cx + Math.round(8 * s), cy + Math.round(15 * s));
    ctx.fillStyle = '#0f172a';
    ctx.font = `700 ${Math.round(12.5 * s)}px "JetBrains Mono", monospace`;
    ctx.fillText(kpis[i][1], cx + Math.round(8 * s), cy + Math.round(33 * s));
  }

  // 3. Top-right depth legend colorbar card
  const legW = Math.round(224 * s);
  const legH = Math.round(64 * s);
  const legX = W - legW - pad;
  ctx.fillStyle = 'rgba(255, 255, 255, 0.93)';
  ctx.beginPath();
  ctx.roundRect(legX, pad, legW, legH, Math.round(10 * s));
  ctx.fill();
  ctx.strokeStyle = 'rgba(15, 23, 42, 0.12)';
  ctx.stroke();

  const barX = legX + Math.round(12 * s);
  const barY = pad + Math.round(12 * s);
  const barW = legW - Math.round(24 * s);
  const barH = Math.round(10 * s);
  const grad = ctx.createLinearGradient(barX, 0, barX + barW, 0);
  grad.addColorStop(0, '#a5f3fc');
  grad.addColorStop(0.25, '#22d3ee');
  grad.addColorStop(0.55, '#2563eb');
  grad.addColorStop(0.82, '#7c3aed');
  grad.addColorStop(1, '#4c1d95');
  ctx.fillStyle = grad;
  ctx.beginPath();
  ctx.roundRect(barX, barY, barW, barH, Math.round(4 * s));
  ctx.fill();

  const cutCm = Math.round((state.displayPrefs.depthCutoffM || 0.15) * 100);
  ctx.fillStyle = '#334155';
  ctx.font = `600 ${Math.round(10 * s)}px "JetBrains Mono", monospace`;
  ctx.fillText(`${(cutCm / 100).toFixed(2)}m`, barX, barY + Math.round(24 * s));
  ctx.fillText('0.3m', barX + Math.round(barW * 0.30), barY + Math.round(24 * s));
  ctx.fillText('0.6m', barX + Math.round(barW * 0.58), barY + Math.round(24 * s));
  ctx.fillText('1.2m+', barX + Math.round(barW * 0.83), barY + Math.round(24 * s));

  ctx.fillStyle = '#10b981';
  ctx.fillRect(barX, barY + Math.round(35 * s), Math.round(14 * s), Math.max(2, Math.round(3 * s)));
  ctx.fillStyle = '#475569';
  ctx.font = `500 ${Math.round(10 * s)}px Inter, system-ui, sans-serif`;
  ctx.fillText(`River channel (< ${cutCm} cm hidden)`, barX + Math.round(20 * s), barY + Math.round(40 * s));

  ctx.restore();
}

function toggleVideoRecording() {
  const btn = $('btn-rec');
  if (activeRecorder && activeRecorder.state === 'recording') {
    activeRecorder.stop();
    btn.classList.remove('recording');
    btn.textContent = 'Rec';
    return;
  }
  if (!map || typeof MediaRecorder === 'undefined') {
    showToast('Browser video recording is not supported in this browser.');
    return;
  }
  const mapCanvas = map.getCanvas();
  const recCanvas = document.createElement('canvas');
  recCanvas.width = mapCanvas.width || 1280;
  recCanvas.height = mapCanvas.height || 720;
  const recCtx = recCanvas.getContext('2d');
  const paintComposite = () => {
    if (!map) return;
    if (recCanvas.width !== mapCanvas.width || recCanvas.height !== mapCanvas.height) {
      recCanvas.width = mapCanvas.width || 1280;
      recCanvas.height = mapCanvas.height || 720;
    }
    // Runs synchronously inside MapLibre's 'render' event while the WebGL drawing buffer is valid
    recCtx.drawImage(mapCanvas, 0, 0, recCanvas.width, recCanvas.height);
    drawRecordingHudOverlay(recCtx, recCanvas.width, recCanvas.height);
  };
  map.on('render', paintComposite);
  map.triggerRepaint();
  const stream = recCanvas.captureStream(24);
  const candidates = [
    'video/mp4;codecs=avc1.42E01E',
    'video/mp4',
    'video/webm;codecs=vp9',
    'video/webm',
  ];
  const mime = candidates.find((m) => MediaRecorder.isTypeSupported(m)) || '';
  const isNativeMp4 = mime.startsWith('video/mp4');
  recordedChunks = [];
  try {
    activeRecorder = mime
      ? new MediaRecorder(stream, { mimeType: mime, videoBitsPerSecond: 4_500_000 })
      : new MediaRecorder(stream, { videoBitsPerSecond: 4_500_000 });
  } catch (err) {
    map.off('render', paintComposite);
    stream.getTracks().forEach((t) => t.stop());
    showToast(`Recording unavailable: ${err.message}`);
    return;
  }
  activeRecorder.ondataavailable = (e) => {
    if (e.data && e.data.size > 0) recordedChunks.push(e.data);
  };
  // Trigger MapLibre repaints at ~24 fps so 'render' composites valid WebGL frames + HUD
  activeRecRepaintTimer = setInterval(() => {
    if (map) map.triggerRepaint();
  }, 42);
  activeRecorder.onstop = async () => {
    if (map) map.off('render', paintComposite);
    if (activeRecRepaintTimer) { clearInterval(activeRecRepaintTimer); activeRecRepaintTimer = null; }
    stream.getTracks().forEach((t) => t.stop());
    const blob = new Blob(recordedChunks, { type: mime || 'video/webm' });
    recordedChunks = [];
    if (!blob.size) {
      showToast('Recording was empty; try recording for at least 2 seconds.');
      return;
    }
    const title = $('po-title')?.textContent?.trim() || 'recording';
    const slug = title.toLowerCase().replace(/[^a-z0-9]+/g, '-').replace(/^-|-$/g, '').slice(0, 28) || 'recording';
    const baseName = `arus-${slug}-${fmtMytSlug(getCurrentFrameEpochMs())}`;
    if (isNativeMp4 || state.serverConfig?.features?.allow_mp4_export === false) {
      const ext = isNativeMp4 ? 'mp4' : 'webm';
      openRecPreviewModal(blob, `${baseName}.${ext}`);
      return;
    }
    btn.disabled = true;
    btn.textContent = 'MP4…';
    try {
      const resp = await fetch(apiUrl('api/v1/export/mp4'), {
        method: 'POST',
        headers: { 'content-type': 'video/webm' },
        body: blob,
      });
      if (resp.ok) {
        const mp4Blob = await resp.blob();
        openRecPreviewModal(mp4Blob, `${baseName}.mp4`);
      } else {
        openRecPreviewModal(blob, `${baseName}.webm`);
      }
    } catch (_) {
      openRecPreviewModal(blob, `${baseName}.webm`);
    } finally {
      btn.disabled = false;
      btn.textContent = 'Rec';
    }
  };
  activeRecorder.start(250);
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
    if (typeof opts.storm_drain_score === 'number' && Number.isFinite(opts.storm_drain_score)) {
      state.simConfig.stormDrainScore = Math.min(1, Math.max(0, opts.storm_drain_score));
    }
    if (Array.isArray(opts.storm_drain_grid)) {
      state.stormDrainGrid = opts.storm_drain_grid.map(v => Math.min(1, Math.max(0, Number(v) || 0)));
    } else if (opts.storm_drain_grid === null) {
      state.stormDrainGrid = null;
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
    state.baselineInterventions = [];
    state.lastBundCell = null;
    state.staticVectorsDirty = true;
    updateInterventionBar();
    renderFloodCanvas(false);
  },
  addBundLine: (points, heightM, reason) => addBundLine(points, heightM, reason),
  breachBund: (lat, lon, radiusCells) => breachBund(lat, lon, radiusCells),
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
  const prev = state.profile;
  const simAllowed = state.serverConfig?.features?.allow_simulation !== false;
  state.profile = (profile === 'public' || !simAllowed) ? 'public' : 'researcher';
  document.documentElement.dataset.profile = state.profile;
  document.querySelectorAll('[data-profile]').forEach((el) => {
    el.classList.toggle('active', el.dataset.profile === state.profile);
  });
  if (state.profile === 'public') {
    if (state.tool !== 'inspect') setTool('inspect');
    if (state.layer === 'diff') setLayer('depth');
    if ($('inspector')) $('inspector').classList.add('hidden');
    $('sim-dir-drawer')?.classList.add('hidden');
    if (map) {
      try { map.setTerrain(null); } catch (_) {}
      map.setMaxZoom(15.5);
      if (map.getZoom() > 15.5) map.setZoom(15.5);
    }
    renderSecondaryLiveOverlays(true);
    refreshAlertsAndBeacons();
  } else {
    if (map) map.setMaxZoom(17.5);
    clearLivePin();
    renderSecondaryLiveOverlays(false);
    clearFloodBeacons();
    // Switching from Live -> Simulate brings t=0 initial state, paused, without auto-solving
    if (prev === 'public' && state.frames.length) {
      state.playing = false;
      state.envelopeMode = false;
      state.frameIdx = 0;
      if ($('btn-env')) {
        $('btn-env').textContent = 'Step';
        $('btn-env').classList.remove('active');
      }
      if ($('btn-play')) $('btn-play').textContent = '▶';
      if ($('fb-play')) $('fb-play').textContent = '▶';
      if ($('scrubber')) $('scrubber').value = '0';
      updateTimeReadout();
      renderFloodCanvas(true);
    }
    refreshSavedCountBadge();
    const hasCustomSim = Boolean(state.frames.length && state.activeEngine && state.activeEngine !== 'Live · Cached');
    setSimPhase(hasCustomSim ? 'results' : 'draft');
  }
  updateLiveMetrics();
  if (persist) saveUserPrefs({ profile: state.profile });
}

// ---------- Zoom-Aware Pulsing Flood Beacons & Top-Bar Alerts ----------
let activeBeaconMarkers = [];
let cachedFloodClusters = [];
let lastAlertsComputeKey = '';
let lastBeaconRenderKey = '';
let beaconTooltipEl = null;

function severityRank(sev) {
  return sev === 'danger' ? 3 : (sev === 'warning' ? 2 : (sev === 'advisory' ? 1 : 0));
}

function severityLabel(sev) {
  return sev === 'danger' ? 'Danger' : (sev === 'warning' ? 'Warning' : 'Advisory');
}

function depthToSeverity(depthM) {
  if (depthM >= 0.60) return 'danger';
  if (depthM >= 0.30) return 'warning';
  return 'advisory';
}

function formatStateName(basinId) {
  const base = String(basinId || 'region').replace(/-(west|east|north|south)$/i, '');
  return base.replace(/(^|[-_])\w/g, (m) => m.toUpperCase()).replace(/[-_]/g, ' ');
}

function ensureBeaconTooltip() {
  if (beaconTooltipEl) return beaconTooltipEl;
  beaconTooltipEl = document.createElement('div');
  beaconTooltipEl.className = 'beacon-tooltip hidden';
  document.body.appendChild(beaconTooltipEl);
  return beaconTooltipEl;
}

function hideBeaconTooltip() {
  if (beaconTooltipEl) beaconTooltipEl.classList.add('hidden');
}

function clearFloodBeacons() {
  for (const m of activeBeaconMarkers) {
    try { m.remove(); } catch (_) {}
  }
  activeBeaconMarkers = [];
  lastBeaconRenderKey = '';
  hideBeaconTooltip();
}

function computeAllLiveFloodClusters() {
  const sources = [];
  if (state.liveOverlays.size > 0) {
    for (const [basinId, ov] of state.liveOverlays.entries()) {
      sources.push({ basinId, grid: ov.grid, frames: ov.frames });
    }
  } else if (state.grid && state.frames.length) {
    sources.push({ basinId: state.liveBasinId || 'local', grid: state.grid, frames: state.frames });
  }

  const alertBounds = Array.isArray(state.serverConfig?.alert_bounds) && state.serverConfig.alert_bounds.length === 4
    ? state.serverConfig.alert_bounds
    : null;
  // Show Warning (>= 0.30 m) and Danger (>= 0.60 m) only to avoid Advisory noise
  const minDepth = 0.30;
  const clusters = [];

  for (const src of sources) {
    const meta = src.grid?.metadata;
    const env = src.grid?.envelope;
    if (!meta || !src.frames?.length) continue;
    const cols = meta.cols;
    const rows = meta.rows;
    const [minLon, minLat, maxLon, maxLat] = meta.wgs84_bounds;
    const cellAreaKm2 = (meta.dx_m * meta.dx_m) / 1e6;
    const fIdx = Math.min(state.frameIdx, src.frames.length - 1);
    const depths = state.envelopeMode && env?.max_depth_m ? env.max_depth_m : src.frames[fIdx].depth;
    const mask = meta.active_mask;
    const visited = new Uint8Array(cols * rows);
    const queue = new Int32Array(cols * rows);

    for (let r = 0; r < rows; r++) {
      for (let c = 0; c < cols; c++) {
        const start = r * cols + c;
        if (visited[start] || !mask[start] || depths[start] < minDepth) continue;
        let head = 0;
        let tail = 0;
        queue[tail++] = start;
        visited[start] = 1;
        let sumLat = 0;
        let sumLon = 0;
        let peakDepth = 0;
        let peakLat = 0;
        let peakLon = 0;
        let count = 0;

        while (head < tail) {
          const idx = queue[head++];
          const cr = Math.floor(idx / cols);
          const cc = idx - cr * cols;
          const h = depths[idx];
          const lat = minLat + ((cr + 0.5) / rows) * (maxLat - minLat);
          const lon = minLon + ((cc + 0.5) / cols) * (maxLon - minLon);
          sumLat += lat;
          sumLon += lon;
          count++;
          if (h > peakDepth) {
            peakDepth = h;
            peakLat = lat;
            peakLon = lon;
          }
          const neighbors = [
            cr > 0 ? idx - cols : -1,
            cr + 1 < rows ? idx + cols : -1,
            cc > 0 ? idx - 1 : -1,
            cc + 1 < cols ? idx + 1 : -1,
          ];
          for (const n of neighbors) {
            if (n >= 0 && !visited[n] && mask[n] && depths[n] >= minDepth) {
              visited[n] = 1;
              queue[tail++] = n;
            }
          }
        }

        const cLat = peakDepth > 0 ? peakLat : sumLat / count;
        const cLon = peakDepth > 0 ? peakLon : sumLon / count;
        if (alertBounds) {
          const [aMinLon, aMinLat, aMaxLon, aMaxLat] = alertBounds;
          if (cLon < aMinLon || cLon > aMaxLon || cLat < aMinLat || cLat > aMaxLat) continue;
        }
        const areaKm2 = count * cellAreaKm2;
        const placeKey = `spot:${cLat.toFixed(2)},${cLon.toFixed(2)}`;
        const cachedPlace = readNominatimCache(placeKey);
        clusters.push({
          basinId: src.basinId,
          stateName: formatStateName(src.basinId),
          placeKey,
          placeName: cachedPlace || null,
          lat: cLat,
          lon: cLon,
          peakDepthM: peakDepth,
          areaKm2,
          severity: depthToSeverity(peakDepth),
        });
      }
    }
  }

  clusters.sort((a, b) => (severityRank(b.severity) - severityRank(a.severity)) || (b.areaKm2 - a.areaKm2));
  return clusters;
}

function formatClusterLabel(item) {
  const cached = item.placeName || readNominatimCache(item.placeKey);
  if (cached) {
    item.placeName = cached;
    return cached;
  }
  return `${item.stateName} (${item.lat.toFixed(2)}°N, ${item.lon.toFixed(2)}°E)`;
}

let alertGeoQueueRunning = false;
async function resolveAlertPlaceNames(clusters) {
  if (alertGeoQueueRunning) return;
  const pending = clusters.filter((c) => !c.placeName && !readNominatimCache(c.placeKey)).slice(0, 12);
  if (!pending.length) return;
  alertGeoQueueRunning = true;
  try {
    for (const item of pending) {
      try {
        const resp = await fetch(
          `https://nominatim.openstreetmap.org/reverse?format=json&zoom=13&lat=${item.lat.toFixed(4)}&lon=${item.lon.toFixed(4)}`,
        );
        if (resp.ok) {
          const data = await resp.json();
          const a = data?.address || {};
          const local = a.village || a.suburb || a.town || a.city || a.county || a.state_district || a.municipality;
          const st = a.state || item.stateName;
          const name = local && st && local !== st ? `${local}, ${st}` : (local || st);
          if (name) {
            writeNominatimCache(item.placeKey, name);
            for (const c of cachedFloodClusters) {
              if (c.placeKey === item.placeKey) c.placeName = name;
            }
            renderAlertsButtonAndDropdown(cachedFloodClusters);
            updateBeaconsForZoom(true);
          }
        }
      } catch (_) {}
      await new Promise((r) => setTimeout(r, 1100));
    }
  } finally {
    alertGeoQueueRunning = false;
  }
}

function renderAlertsButtonAndDropdown(clusters) {
  const btn = $('btn-alerts');
  const badge = $('alerts-badge');
  const list = $('alerts-dd-list');
  const sub = $('alerts-dd-sub');
  if (!btn || !badge || !list) return;

  let maxSev = 'none';
  for (const c of clusters) {
    if (severityRank(c.severity) > severityRank(maxSev)) maxSev = c.severity;
  }
  btn.className = `btn btn-xs alert-btn ${maxSev}`;
  badge.textContent = String(clusters.length);
  btn.title = clusters.length
    ? `${clusters.length} Warning/Danger flood zone${clusters.length === 1 ? '' : 's'} (max: ${severityLabel(maxSev)})`
    : 'No active Warning or Danger flood alerts';
  if (sub) {
    sub.textContent = clusters.length
      ? `${clusters.length} Warning/Danger · ${state.envelopeMode ? '5-Day Peak' : 'Current'}`
      : 'All clear';
  }

  list.replaceChildren();
  if (!clusters.length) {
    const empty = document.createElement('div');
    empty.style.cssText = 'padding:12px;text-align:center;font-size:11.5px;color:var(--ink-3)';
    empty.textContent = 'No active Warning (≥ 30 cm) or Danger (≥ 60 cm) flood zones.';
    list.appendChild(empty);
    return;
  }

  for (const item of clusters.slice(0, 25)) {
    const row = document.createElement('button');
    row.type = 'button';
    row.className = 'alert-row';

    const top = document.createElement('div');
    top.className = 'alert-row-top';
    const loc = document.createElement('span');
    loc.textContent = formatClusterLabel(item);
    const pill = document.createElement('span');
    pill.className = `alert-pill ${item.severity}`;
    pill.textContent = severityLabel(item.severity);
    top.append(loc, pill);

    const meta = document.createElement('div');
    meta.className = 'alert-row-meta';
    meta.textContent = `Peak ${fmtDepth(item.peakDepthM, 2)} · Affected ${item.areaKm2.toFixed(1)} km²`;

    row.append(top, meta);
    row.addEventListener('click', () => {
      $('alerts-dropdown')?.classList.add('hidden');
      clearBoundaryFocus();
      if (map) {
        map.flyTo({ center: [item.lon, item.lat], zoom: 11.4, pitch: 0, bearing: 0, duration: 900 });
      }
    });
    list.appendChild(row);
  }
}

function groupBeaconsForZoom(clusters, zoom) {
  // Level 4: Disappear once zoomed into local raster detail
  if (zoom >= 10.2) return { tier: 'hidden', beacons: [] };

  // Level 1: State-wide grouping (count)
  if (zoom < 7.2) {
    const byState = new Map();
    for (const c of clusters) {
      let g = byState.get(c.stateName);
      if (!g) {
        g = { title: c.stateName, latSum: 0, lonSum: 0, count: 0, peakDepthM: 0, areaKm2: 0, severity: 'advisory', targetZoom: 8.1 };
        byState.set(c.stateName, g);
      }
      g.latSum += c.lat;
      g.lonSum += c.lon;
      g.count += 1;
      g.areaKm2 += c.areaKm2;
      if (c.peakDepthM > g.peakDepthM) g.peakDepthM = c.peakDepthM;
      if (severityRank(c.severity) > severityRank(g.severity)) g.severity = c.severity;
    }
    return {
      tier: 'state',
      beacons: Array.from(byState.values()).map((g) => ({
        title: `${g.title} (${g.count} ${g.count === 1 ? 'zone' : 'zones'})`,
        lat: g.latSum / g.count,
        lon: g.lonSum / g.count,
        count: g.count,
        singular: false,
        peakDepthM: g.peakDepthM,
        areaKm2: g.areaKm2,
        severity: g.severity,
        targetZoom: g.count > 1 ? 8.1 : 11.4,
      })),
    };
  }

  // Level 2: District-scale spatial clustering (~30 km radius)
  if (zoom < 8.8) {
    const radiusDeg = 0.28;
    const groups = [];
    for (const c of clusters) {
      let matched = null;
      for (const g of groups) {
        if (g.stateName === c.stateName && Math.hypot(g.lat - c.lat, g.lon - c.lon) <= radiusDeg) {
          matched = g;
          break;
        }
      }
      if (!matched) {
        groups.push({
          stateName: c.stateName,
          placeName: c.placeName,
          latSum: c.lat,
          lonSum: c.lon,
          lat: c.lat,
          lon: c.lon,
          count: 1,
          peakDepthM: c.peakDepthM,
          areaKm2: c.areaKm2,
          severity: c.severity,
        });
      } else {
        matched.latSum += c.lat;
        matched.lonSum += c.lon;
        matched.count += 1;
        matched.lat = matched.latSum / matched.count;
        matched.lon = matched.lonSum / matched.count;
        matched.areaKm2 += c.areaKm2;
        if (c.peakDepthM > matched.peakDepthM) matched.peakDepthM = c.peakDepthM;
        if (severityRank(c.severity) > severityRank(matched.severity)) matched.severity = c.severity;
      }
    }
    return {
      tier: 'district',
      beacons: groups.map((g) => ({
        title: g.count > 1
          ? `${g.stateName} (${g.count} zones)`
          : formatClusterLabel(g),
        lat: g.lat,
        lon: g.lon,
        count: g.count,
        singular: g.count === 1,
        peakDepthM: g.peakDepthM,
        areaKm2: g.areaKm2,
        severity: g.severity,
        targetZoom: g.count > 1 ? 9.4 : 11.4,
      })),
    };
  }

  // Level 3: Region / Local singular beacons only
  return {
    tier: 'region',
    beacons: clusters.map((c) => ({
      title: formatClusterLabel(c),
      lat: c.lat,
      lon: c.lon,
      count: 1,
      singular: true,
      peakDepthM: c.peakDepthM,
      areaKm2: c.areaKm2,
      severity: c.severity,
      targetZoom: 11.4,
    })),
  };
}

function updateBeaconsForZoom(force = false) {
  if (!map || state.profile !== 'public' || state.displayPrefs.showFlood === false) {
    clearFloodBeacons();
    return;
  }
  const zoom = map.getZoom();
  const { tier, beacons } = groupBeaconsForZoom(cachedFloodClusters, zoom);
  const renderKey = `${lastAlertsComputeKey}:${tier}:${beacons.length}`;
  if (!force && renderKey === lastBeaconRenderKey) return;
  clearFloodBeacons();
  lastBeaconRenderKey = renderKey;
  if (tier === 'hidden' || !beacons.length) return;

  const tip = ensureBeaconTooltip();
  for (const b of beacons) {
    const el = document.createElement('div');
    el.className = `flood-beacon ${b.severity}${b.singular ? ' singular' : ''}`;
    const ring = document.createElement('span');
    ring.className = 'flood-beacon-ring';
    const core = document.createElement('span');
    core.className = 'flood-beacon-core';
    core.textContent = b.singular ? '' : String(b.count);
    el.append(ring, core);

    el.addEventListener('mouseenter', (ev) => {
      tip.innerHTML = '';
      const strong = document.createElement('strong');
      strong.textContent = b.title;
      const line1 = document.createElement('div');
      line1.textContent = `${severityLabel(b.severity)} · Peak ${fmtDepth(b.peakDepthM, 2)} · ${b.areaKm2.toFixed(1)} km²`;
      const line2 = document.createElement('div');
      line2.className = 'bt-sub';
      line2.textContent = 'Click to zoom in';
      tip.append(strong, line1, line2);
      tip.style.left = `${Math.min(window.innerWidth - 255, ev.clientX + 14)}px`;
      tip.style.top = `${Math.max(12, ev.clientY - 12)}px`;
      tip.classList.remove('hidden');
    });
    el.addEventListener('mousemove', (ev) => {
      tip.style.left = `${Math.min(window.innerWidth - 255, ev.clientX + 14)}px`;
      tip.style.top = `${Math.max(12, ev.clientY - 12)}px`;
    });
    el.addEventListener('mouseleave', hideBeaconTooltip);
    el.addEventListener('click', (ev) => {
      ev.stopPropagation();
      hideBeaconTooltip();
      clearBoundaryFocus();
      map.flyTo({ center: [b.lon, b.lat], zoom: b.targetZoom, pitch: 0, bearing: 0, duration: 850 });
    });

    const marker = new maplibregl.Marker({ element: el, anchor: 'center' })
      .setLngLat([b.lon, b.lat])
      .addTo(map);
    activeBeaconMarkers.push(marker);
  }
}

const clusterMemoCache = new Map();
function refreshAlertsAndBeacons() {
  if (state.profile !== 'public') return;
  const key = `${state.liveOverlays.size}:${state.runId}:${state.frameIdx}:${state.envelopeMode}`;
  if (key !== lastAlertsComputeKey) {
    lastAlertsComputeKey = key;
    let clusters = clusterMemoCache.get(key);
    if (!clusters) {
      clusters = computeAllLiveFloodClusters();
      if (clusterMemoCache.size >= 128) {
        const oldest = clusterMemoCache.keys().next().value;
        if (oldest !== undefined) clusterMemoCache.delete(oldest);
      }
      clusterMemoCache.set(key, clusters);
    }
    cachedFloodClusters = clusters;
    renderAlertsButtonAndDropdown(cachedFloodClusters);
    updateBeaconsForZoom(true);
    resolveAlertPlaceNames(cachedFloodClusters);
  } else {
    updateBeaconsForZoom(false);
  }
}

let activeIncidentMarkers = [];
function renderIncidentMarkers() {
  for (const m of activeIncidentMarkers) {
    try { m.remove(); } catch (_) {}
  }
  activeIncidentMarkers = [];
  if (!map || state.displayPrefs.showMarkers === false) return;
  const markers = state.serverContext?.context?.incident_markers;
  if (!Array.isArray(markers) || !markers.length) return;
  const tip = ensureBeaconTooltip();
  for (const inc of markers) {
    if (!Number.isFinite(inc.lat) || !Number.isFinite(inc.lon)) continue;
    const el = document.createElement('div');
    el.className = 'incident-pin';
    el.textContent = `⚠ ${inc.title || 'Field Incident'}`;
    el.addEventListener('mouseenter', (ev) => {
      tip.innerHTML = '';
      const strong = document.createElement('strong');
      strong.textContent = inc.title || 'Field Incident';
      const line1 = document.createElement('div');
      line1.textContent = inc.note || 'Operator-curated field incident.';
      tip.append(strong, line1);
      tip.style.left = `${Math.min(window.innerWidth - 255, ev.clientX + 14)}px`;
      tip.style.top = `${Math.max(12, ev.clientY - 12)}px`;
      tip.classList.remove('hidden');
    });
    el.addEventListener('mouseleave', hideBeaconTooltip);
    el.addEventListener('click', (ev) => {
      ev.stopPropagation();
      hideBeaconTooltip();
      map.flyTo({ center: [inc.lon, inc.lat], zoom: 12.2, pitch: 0, bearing: 0, duration: 850 });
    });
    const m = new maplibregl.Marker({ element: el, anchor: 'center' })
      .setLngLat([inc.lon, inc.lat])
      .addTo(map);
    activeIncidentMarkers.push(m);
  }
}

// ---------- Settings Dialog & Manage Locations ----------
const CUSTOM_LOCS_KEY = 'arus.customLocations.v1';
let editingCustomLocId = null;

function loadCustomLocations() {
  try {
    const raw = localStorage.getItem(CUSTOM_LOCS_KEY);
    const parsed = raw ? JSON.parse(raw) : [];
    return Array.isArray(parsed) ? parsed : [];
  } catch (_) {
    return [];
  }
}

function saveCustomLocations(list) {
  try {
    localStorage.setItem(CUSTOM_LOCS_KEY, JSON.stringify(list));
  } catch (_) {}
}

function rebuildPresetDropdown() {
  const custom = loadCustomLocations().map((c) => ({ ...c, isServer: false }));
  PRESETS = [...SERVER_PRESETS, ...custom];
  const sel = $('preset-select');
  if (sel) {
    const prev = sel.value;
    sel.innerHTML = PRESETS.map((p) => `<option value="${p.id}">${p.name}</option>`).join('');
    const prefs = loadUserPrefs();
    const defId = prefs.default_location_id;
    const defLabel = String(prefs.last_center?.label || '').replace(/^★\s*/, '').trim().toLowerCase();
    const defMatch = PRESETS.find((p) => p.id === defId || (defLabel && p.name.toLowerCase() === defLabel));
    if (PRESETS.some((p) => p.id === prev)) sel.value = prev;
    else if (defMatch) sel.value = defMatch.id;
  }
}

function setLocFormDisabled(disabled) {
  ['loc-name', 'loc-kind', 'loc-lat', 'loc-lon', 'loc-zoom', 'loc-pitch', 'loc-bearing', 'loc-query', 'btn-loc-use-view'].forEach((id) => {
    const el = $(id);
    if (el) el.disabled = disabled;
  });
  const addBtn = $('btn-loc-add');
  if (addBtn) {
    addBtn.disabled = disabled;
    addBtn.textContent = disabled ? 'Server (Read-Only)' : (editingCustomLocId ? 'Save Changes' : '+ Add Location');
  }
}

function populateLocationForm(loc) {
  editingCustomLocId = loc.isServer ? null : loc.id;
  if ($('loc-name')) $('loc-name').value = loc.name || '';
  const isB = Boolean(loc.boundaryQuery);
  if ($('loc-kind')) {
    $('loc-kind').value = isB ? 'boundary' : 'point';
    $('loc-kind').dispatchEvent(new Event('change'));
  }
  if (isB) {
    if ($('loc-query')) $('loc-query').value = loc.boundaryQuery || '';
  } else {
    if ($('loc-lat')) $('loc-lat').value = String(loc.lat ?? '');
    if ($('loc-lon')) $('loc-lon').value = String(loc.lon ?? '');
    if ($('loc-zoom')) $('loc-zoom').value = String(loc.zoom ?? 11.5);
    if ($('loc-pitch')) $('loc-pitch').value = String(loc.pitch ?? 0);
    if ($('loc-bearing')) $('loc-bearing').value = String(loc.bearing ?? 0);
  }
  setLocFormDisabled(Boolean(loc.isServer));
}

function openAppSettingsModal(prefillCurrentView = false) {
  editingCustomLocId = null;
  setLocFormDisabled(false);
  if ($('btn-loc-add')) $('btn-loc-add').textContent = '+ Add Location';
  // Ensure Manage Locations tab is active
  document.querySelectorAll('[data-settings-tab]').forEach((b) => {
    b.classList.toggle('active', b.dataset.settingsTab === 'locations');
  });
  $('settings-pane-locations')?.classList.remove('hidden');
  $('settings-pane-general')?.classList.add('hidden');
  if (prefillCurrentView && map) {
    if ($('loc-kind')) {
      $('loc-kind').value = 'point';
      $('loc-kind').dispatchEvent(new Event('change'));
    }
    const c = map.getCenter();
    if ($('loc-lat')) $('loc-lat').value = c.lat.toFixed(4);
    if ($('loc-lon')) $('loc-lon').value = c.lng.toFixed(4);
    if ($('loc-zoom')) $('loc-zoom').value = map.getZoom().toFixed(1);
    if ($('loc-pitch')) $('loc-pitch').value = String(Math.round(map.getPitch()));
    if ($('loc-bearing')) $('loc-bearing').value = String(Math.round(map.getBearing()));
    const hint = $('coord-search')?.value?.trim() || '';
    if ($('loc-name')) {
      $('loc-name').value = hint;
      setTimeout(() => $('loc-name')?.focus(), 50);
    }
  }
  const tzSel = $('opt-timezone');
  if (tzSel) {
    const localOpt = tzSel.querySelector('option[value="local"]');
    if (localOpt) localOpt.textContent = `Browser Local (${getBrowserTimeZone()})`;
    tzSel.value = state.displayPrefs.timezone || 'local';
  }
  if ($('opt-settings-clock')) $('opt-settings-clock').value = state.displayPrefs.clock || '12h';
  rebuildPresetDropdown();
  renderManageLocationsList();
  $('app-settings-modal')?.classList.remove('hidden');
}

function addOrUpdateCustomLocation() {
  const name = $('loc-name')?.value?.trim();
  const kind = $('loc-kind')?.value || 'point';
  if (!name) {
    showToast('Enter a location name.');
    return;
  }
  const list = loadCustomLocations();
  let entry;
  if (kind === 'boundary') {
    const query = $('loc-query')?.value?.trim();
    if (!query) {
      showToast('Enter a place or administrative boundary to search.');
      return;
    }
    entry = {
      id: editingCustomLocId || `custom-${Date.now()}`,
      name,
      kind: 'boundary',
      boundaryQuery: query,
      lon: map ? Number(map.getCenter().lng.toFixed(4)) : 101.696,
      lat: map ? Number(map.getCenter().lat.toFixed(4)) : 3.149,
      zoom: 9.5,
      pitch: 0,
      bearing: 0,
    };
  } else {
    const lat = parseFloat($('loc-lat')?.value);
    const lon = parseFloat($('loc-lon')?.value);
    const zoom = parseFloat($('loc-zoom')?.value || '11.5');
    const pitch = parseFloat($('loc-pitch')?.value || '0');
    const bearing = parseFloat($('loc-bearing')?.value || '0');
    if (!Number.isFinite(lat) || !Number.isFinite(lon) || lat < -85 || lat > 85 || lon < -180 || lon > 180) {
      showToast('Enter valid latitude (-85..85) and longitude (-180..180), or click Use View.');
      return;
    }
    entry = {
      id: editingCustomLocId || `custom-${Date.now()}`,
      name,
      kind: 'point',
      lat: Number(lat.toFixed(5)),
      lon: Number(lon.toFixed(5)),
      zoom: Number(Math.min(18, Math.max(3, Number.isFinite(zoom) ? zoom : 11.5)).toFixed(2)),
      pitch: Number(Math.min(60, Math.max(0, Number.isFinite(pitch) ? pitch : 0)).toFixed(1)),
      bearing: Number(Math.min(180, Math.max(-180, Number.isFinite(bearing) ? bearing : 0)).toFixed(1)),
      boundaryQuery: null,
    };
  }
  const idx = list.findIndex((x) => x.id === entry.id);
  if (idx >= 0) list[idx] = entry;
  else list.push(entry);
  saveCustomLocations(list);
  editingCustomLocId = null;
  if ($('loc-name')) $('loc-name').value = '';
  if ($('loc-query')) $('loc-query').value = '';
  if ($('btn-loc-add')) $('btn-loc-add').textContent = '+ Add Location';
  rebuildPresetDropdown();
  renderManageLocationsList();
  showToast(`Saved location "${name}".`);
}

function setDefaultLocation(loc) {
  saveUserPrefs({
    default_location_id: loc.id,
    last_center: {
      lat: loc.lat,
      lon: loc.lon,
      zoom: loc.zoom || 10.2,
      pitch: Number.isFinite(loc.pitch) ? loc.pitch : 0,
      bearing: Number.isFinite(loc.bearing) ? loc.bearing : 0,
      label: loc.name,
      boundaryQuery: loc.boundaryQuery || null,
    },
  });
  renderManageLocationsList();
  showToast(`Set "${loc.name}" as your default startup view.`);
}

function renderManageLocationsList() {
  const listEl = $('loc-list');
  if (!listEl) return;
  const prefs = loadUserPrefs();
  const activeDefId = prefs.default_location_id || (SERVER_PRESETS[0]?.id || 'default');
  listEl.replaceChildren();

  for (const loc of PRESETS) {
    const row = document.createElement('div');
    row.className = 'saved-sim-row';

    const starBtn = document.createElement('button');
    starBtn.type = 'button';
    const isDef = loc.id === activeDefId;
    starBtn.className = `btn btn-xs loc-star-btn${isDef ? ' active' : ''}`;
    starBtn.textContent = isDef ? '★' : '☆';
    starBtn.title = isDef ? 'Default startup view' : 'Set as default startup view';
    starBtn.addEventListener('click', () => setDefaultLocation(loc));

    const info = document.createElement('div');
    info.className = 'saved-sim-info';
    info.style.cursor = 'pointer';
    info.title = loc.isServer ? 'Click to view details (read-only)' : 'Click to edit details';
    info.addEventListener('click', () => {
      populateLocationForm(loc);
    });

    const title = document.createElement('div');
    title.className = 'saved-sim-title';
    const badge = document.createElement('span');
    badge.className = `saved-sim-badge ${loc.isServer ? 'cfg' : 'sim'}`;
    badge.textContent = loc.isServer ? 'Server' : 'Custom';
    const nameSpan = document.createElement('span');
    nameSpan.textContent = loc.name;
    title.append(badge, nameSpan);

    const meta = document.createElement('div');
    meta.className = 'saved-sim-meta';
    meta.textContent = loc.boundaryQuery
      ? `Boundary: ${loc.boundaryQuery}`
      : `${Number(loc.lat).toFixed(3)}°, ${Number(loc.lon).toFixed(3)}° · z${Number(loc.zoom).toFixed(1)} · pitch ${Math.round(loc.pitch || 0)}° · bearing ${Math.round(loc.bearing || 0)}°`;
    info.append(title, meta);

    const actions = document.createElement('div');
    actions.className = 'saved-sim-actions';
    if (!loc.isServer) {
      const editBtn = document.createElement('button');
      editBtn.type = 'button';
      editBtn.className = 'btn btn-xs';
      editBtn.textContent = 'Edit';
      editBtn.addEventListener('click', (ev) => {
        ev.stopPropagation();
        populateLocationForm(loc);
      });
      const delBtn = document.createElement('button');
      delBtn.type = 'button';
      delBtn.className = 'btn btn-xs';
      delBtn.textContent = 'Delete';
      delBtn.addEventListener('click', (ev) => {
        ev.stopPropagation();
        const next = loadCustomLocations().filter((x) => x.id !== loc.id);
        saveCustomLocations(next);
        if (activeDefId === loc.id) saveUserPrefs({ default_location_id: null, last_center: null });
        if (editingCustomLocId === loc.id) {
          editingCustomLocId = null;
          setLocFormDisabled(false);
        }
        rebuildPresetDropdown();
        renderManageLocationsList();
      });
      actions.append(editBtn, delBtn);
    }

    row.append(starBtn, info, actions);
    listEl.appendChild(row);
  }
}

// ---------- IndexedDB Saved Configs & Completed Simulations ----------
const SAVED_DB_NAME = 'arus-saved-sims-v1';
const SAVED_STORE = 'items';

function openSavedDb() {
  return new Promise((resolve, reject) => {
    if (typeof indexedDB === 'undefined') {
      reject(new Error('IndexedDB unavailable in this browser'));
      return;
    }
    const req = indexedDB.open(SAVED_DB_NAME, 1);
    req.onupgradeneeded = () => {
      const db = req.result;
      if (!db.objectStoreNames.contains(SAVED_STORE)) {
        db.createObjectStore(SAVED_STORE, { keyPath: 'id' });
      }
    };
    req.onsuccess = () => resolve(req.result);
    req.onerror = () => reject(req.error || new Error('Failed to open IndexedDB'));
  });
}

async function idbAllSaved() {
  const db = await openSavedDb();
  return new Promise((resolve, reject) => {
    const tx = db.transaction(SAVED_STORE, 'readonly');
    const req = tx.objectStore(SAVED_STORE).getAll();
    req.onsuccess = () => {
      const items = (req.result || []).sort((a, b) => (b.createdAt || 0) - (a.createdAt || 0));
      resolve(items);
    };
    req.onerror = () => reject(req.error);
  });
}

async function idbPutSaved(item) {
  const db = await openSavedDb();
  return new Promise((resolve, reject) => {
    const tx = db.transaction(SAVED_STORE, 'readwrite');
    tx.objectStore(SAVED_STORE).put(item);
    tx.oncomplete = () => resolve();
    tx.onerror = () => reject(tx.error);
  });
}

async function idbGetSaved(id) {
  const db = await openSavedDb();
  return new Promise((resolve, reject) => {
    const tx = db.transaction(SAVED_STORE, 'readonly');
    const req = tx.objectStore(SAVED_STORE).get(id);
    req.onsuccess = () => resolve(req.result || null);
    req.onerror = () => reject(req.error);
  });
}

async function idbDeleteSaved(id) {
  const db = await openSavedDb();
  return new Promise((resolve, reject) => {
    const tx = db.transaction(SAVED_STORE, 'readwrite');
    tx.objectStore(SAVED_STORE).delete(id);
    tx.oncomplete = () => resolve();
    tx.onerror = () => reject(tx.error);
  });
}

const SIM_DIR_DB_NAME = 'arus-sim-dir-v2';
const SIM_DIR_META_STORE = 'runs_meta';
const SIM_DIR_BLOB_STORE = 'runs_blob';
const SIM_DIR_MAX_UNPINNED = 25;

function openSimDirDb() {
  return new Promise((resolve, reject) => {
    if (typeof indexedDB === 'undefined') {
      reject(new Error('IndexedDB unavailable in this browser'));
      return;
    }
    const req = indexedDB.open(SIM_DIR_DB_NAME, 1);
    req.onupgradeneeded = () => {
      const db = req.result;
      if (!db.objectStoreNames.contains(SIM_DIR_META_STORE)) {
        db.createObjectStore(SIM_DIR_META_STORE, { keyPath: 'id' });
      }
      if (!db.objectStoreNames.contains(SIM_DIR_BLOB_STORE)) {
        db.createObjectStore(SIM_DIR_BLOB_STORE, { keyPath: 'id' });
      }
    };
    req.onsuccess = () => resolve(req.result);
    req.onerror = () => reject(req.error || new Error('Failed to open Simulation Directory DB'));
  });
}

async function idbAllRunMeta() {
  const db = await openSimDirDb();
  return new Promise((resolve, reject) => {
    const tx = db.transaction(SIM_DIR_META_STORE, 'readonly');
    const req = tx.objectStore(SIM_DIR_META_STORE).getAll();
    req.onsuccess = () => {
      const items = (req.result || []).sort((a, b) => (b.createdAt || 0) - (a.createdAt || 0));
      resolve(items);
    };
    req.onerror = () => reject(req.error);
  });
}

async function idbPutRun(meta, blob = null) {
  const db = await openSimDirDb();
  return new Promise((resolve, reject) => {
    const stores = blob ? [SIM_DIR_META_STORE, SIM_DIR_BLOB_STORE] : [SIM_DIR_META_STORE];
    const tx = db.transaction(stores, 'readwrite');
    tx.objectStore(SIM_DIR_META_STORE).put(meta);
    if (blob) tx.objectStore(SIM_DIR_BLOB_STORE).put(blob);
    tx.oncomplete = () => resolve();
    tx.onerror = () => reject(tx.error);
    tx.onabort = () => reject(tx.error || new Error('Transaction aborted'));
  });
}

async function idbGetRunBlob(id) {
  const db = await openSimDirDb();
  return new Promise((resolve, reject) => {
    const tx = db.transaction(SIM_DIR_BLOB_STORE, 'readonly');
    const req = tx.objectStore(SIM_DIR_BLOB_STORE).get(id);
    req.onsuccess = () => resolve(req.result || null);
    req.onerror = () => reject(req.error);
  });
}

async function idbDeleteRun(id) {
  const db = await openSimDirDb();
  return new Promise((resolve, reject) => {
    const tx = db.transaction([SIM_DIR_META_STORE, SIM_DIR_BLOB_STORE], 'readwrite');
    tx.objectStore(SIM_DIR_META_STORE).delete(id);
    tx.objectStore(SIM_DIR_BLOB_STORE).delete(id);
    tx.oncomplete = () => resolve();
    tx.onerror = () => reject(tx.error);
    tx.onabort = () => reject(tx.error || new Error('Transaction aborted'));
  });
}

async function idbEnforceSimDirLru() {
  try {
    const all = await idbAllRunMeta();
    const unpinned = all.filter((x) => !x.pinned);
    if (unpinned.length <= SIM_DIR_MAX_UNPINNED) return;
    const toEvict = unpinned.slice(SIM_DIR_MAX_UNPINNED);
    for (const old of toEvict) {
      await idbDeleteRun(old.id);
    }
  } catch (_) {}
}

function startCleanSimDraft() {
  state.draftParentRunId = null;
  state.draftParentSeq = null;
  state.interventions = [];
  state.baselineInterventions = [];
  state.observedGauges = [];
  state.diffData = null;
  state.uploadedDem = null;
  state.lastBundCell = null;
  state.xsecPts = [];
  state.selectedCell = null;
  state.staticVectorsDirty = true;
  updateInterventionBar();
  updateFloodPinBar();
  renderFloodCanvas(false);
  setSimPhase('draft');
  showToast('Started a clean simulation draft.');
}

function computeRunSummaryMetrics(grid) {
  const meta = grid?.metadata;
  const env = grid?.envelope;
  if (!meta || !env?.max_depth_m) {
    return { peakDepthM: 0, floodedPct: 0, floodedKm2: 0, gridDims: '—' };
  }
  const cut = state.displayPrefs.depthCutoffM || 0.08;
  const cellKm2 = (meta.dx_m * meta.dx_m) / 1_000_000;
  let active = 0;
  let wet = 0;
  let peak = 0;
  for (let i = 0; i < env.max_depth_m.length; i++) {
    if (!meta.active_mask[i]) continue;
    active++;
    const h = env.max_depth_m[i] || 0;
    if (h > peak) peak = h;
    if (h >= cut) wet++;
  }
  return {
    peakDepthM: peak,
    floodedPct: active > 0 ? Math.round((wet / active) * 100) : 0,
    floodedKm2: wet * cellKm2,
    gridDims: `${meta.cols}×${meta.rows} · ${Math.round(meta.dx_m)}m`,
  };
}

async function autoSaveCompletedRun() {
  if (!state.grid || !state.lastFramesBuf || !state.frames.length) return;
  try {
    const existing = await idbAllRunMeta();
    const nextSeq = existing.reduce((mx, x) => Math.max(mx, Number(x.seq) || 0), 0) + 1;
    const locLabel = $('preset-select')?.selectedOptions?.[0]?.textContent?.trim()
      || `${state.center.lat.toFixed(2)}°, ${state.center.lon.toFixed(2)}°`;
    const id = `run-${Date.now()}-${Math.random().toString(36).slice(2, 7)}`;
    const bufCopy = state.lastFramesBuf.slice(0);
    const summary = computeRunSummaryMetrics(state.grid);
    const meta = {
      id,
      seq: nextSeq,
      kind: 'simulation',
      name: `${locLabel} · #${nextSeq}`,
      parentRunId: state.draftParentRunId || null,
      parentSeq: state.draftParentSeq || null,
      pinned: false,
      createdAt: Date.now(),
      sizeBytes: bufCopy.byteLength + JSON.stringify(state.grid).length,
      config: {
        center: { lat: state.center.lat, lon: state.center.lon },
        zoom: map ? map.getZoom() : state.initZoom,
        simConfig: { ...state.simConfig },
        interventions: state.interventions.map((x) => ({ ...x })),
        baselineInterventions: state.baselineInterventions.map((x) => ({ ...x })),
        antecedentMm: state.antecedentMm,
        observedGauges: state.observedGauges.map((x) => ({ ...x })),
        boundaryGeoJson: state.boundaryGeoJson || null,
        boundaryName: state.activeBoundaryName || null,
      },
      summary: {
        ...summary,
        engine: state.activeEngine || 'Solved',
        editCount: state.interventions.length,
        massOk: state.grid?.budget_assessment?.passed !== false,
      },
    };
    const blob = {
      id,
      grid: state.grid,
      framesBuf: bufCopy,
      runId: state.runId || id,
      engine: state.activeEngine || 'Solved',
    };
    await idbPutRun(meta, blob);
    await idbEnforceSimDirLru();
    state.activeRunId = id;
    state.activeRunSeq = nextSeq;
    state.draftParentRunId = null;
    state.draftParentSeq = null;
    await refreshSavedCountBadge();
    if (!$('sim-dir-drawer')?.classList.contains('hidden')) {
      await renderSimDirList();
    }
  } catch (_) {}
}

async function saveDraftConfigToDir() {
  try {
    const existing = await idbAllRunMeta();
    const nextSeq = existing.reduce((mx, x) => Math.max(mx, Number(x.seq) || 0), 0) + 1;
    const locLabel = $('preset-select')?.selectedOptions?.[0]?.textContent?.trim()
      || `${state.center.lat.toFixed(2)}°, ${state.center.lon.toFixed(2)}°`;
    const id = `cfg-${Date.now()}-${Math.random().toString(36).slice(2, 7)}`;
    const meta = {
      id,
      seq: nextSeq,
      kind: 'config',
      name: `${locLabel} Draft · #${nextSeq}`,
      parentRunId: state.draftParentRunId || null,
      parentSeq: state.draftParentSeq || null,
      pinned: true,
      createdAt: Date.now(),
      sizeBytes: 2048,
      config: {
        center: { lat: state.center.lat, lon: state.center.lon },
        zoom: map ? map.getZoom() : state.initZoom,
        simConfig: { ...state.simConfig },
        interventions: state.interventions.map((x) => ({ ...x })),
        baselineInterventions: state.baselineInterventions.map((x) => ({ ...x })),
        antecedentMm: state.antecedentMm,
        observedGauges: state.observedGauges.map((x) => ({ ...x })),
        boundaryGeoJson: state.boundaryGeoJson || null,
        boundaryName: state.activeBoundaryName || null,
      },
      summary: {
        peakDepthM: 0,
        floodedPct: 0,
        floodedKm2: 0,
        gridDims: 'Draft',
        engine: 'Config',
        editCount: state.interventions.length,
        massOk: true,
      },
    };
    await idbPutRun(meta, null);
    await refreshSavedCountBadge();
    await renderSimDirList();
    showToast(`Saved draft "${meta.name}".`);
  } catch (err) {
    showToast(`Save draft failed: ${err && err.message ? err.message : String(err)}`);
  }
}

async function openSimDirRun(meta) {
  if (!meta) return;
  try {
    const cfg = meta.config || {};
    if (cfg.simConfig) Object.assign(state.simConfig, cfg.simConfig);
    if (cfg.center) state.center = { lat: cfg.center.lat, lon: cfg.center.lon };
    state.interventions = Array.isArray(cfg.interventions) ? cfg.interventions.map((x) => ({ ...x })) : [];
    state.baselineInterventions = Array.isArray(cfg.baselineInterventions) ? cfg.baselineInterventions.map((x) => ({ ...x })) : [];
    if (typeof cfg.antecedentMm === 'number') state.antecedentMm = cfg.antecedentMm;
    if (Array.isArray(cfg.observedGauges)) state.observedGauges = cfg.observedGauges.map((x) => ({ ...x }));
    state.diffData = null;
    if (state.layer === 'diff') setLayer('depth');
    if (cfg.boundaryGeoJson) {
      state.activeBoundaryName = cfg.boundaryName || null;
      await applyBoundaryGeometry(cfg.boundaryGeoJson, false, { fitView: false });
    } else {
      clearBoundaryFocus();
    }
    updateInterventionBar();
    updateFloodPinBar();
    if (map && cfg.center) {
      map.flyTo({ center: [cfg.center.lon, cfg.center.lat], zoom: cfg.zoom || 12.2, duration: 600 });
    }
    if (meta.kind === 'simulation') {
      const blob = await idbGetRunBlob(meta.id);
      if (blob?.grid && blob?.framesBuf) {
        const frames = decodeFramesBin(blob.framesBuf);
        if (frames.length) {
          state.frameRasterCache.clear();
          state.playing = false;
          state.activeRunId = meta.id;
          state.activeRunSeq = meta.seq;
          applyLoadedSimulation(blob.grid, frames, blob.runId || meta.id, `${blob.engine || 'Saved'} · #${meta.seq}`, blob.framesBuf);
          setSimPhase('results');
          if (window.innerWidth < 1440) $('sim-dir-drawer')?.classList.add('hidden');
          await renderSimDirList();
          showToast(`Opened #${meta.seq} "${meta.name}" instantly.`);
          return;
        }
      }
    }
    state.activeRunId = null;
    state.activeRunSeq = null;
    state.staticVectorsDirty = true;
    renderFloodCanvas(false);
    setSimPhase('draft');
    if (window.innerWidth < 1440) $('sim-dir-drawer')?.classList.add('hidden');
    showToast(`Loaded draft "${meta.name}" — click Run Simulation when ready.`);
  } catch (err) {
    showToast(`Open failed: ${err && err.message ? err.message : String(err)}`);
  }
}

async function forkSimDirRun(meta) {
  if (!meta) return;
  const cfg = meta.config || {};
  if (cfg.simConfig) Object.assign(state.simConfig, cfg.simConfig);
  if (cfg.center) state.center = { lat: cfg.center.lat, lon: cfg.center.lon };
  state.interventions = Array.isArray(cfg.interventions) ? cfg.interventions.map((x) => ({ ...x })) : [];
  state.baselineInterventions = Array.isArray(cfg.baselineInterventions) ? cfg.baselineInterventions.map((x) => ({ ...x })) : [];
  state.observedGauges = Array.isArray(cfg.observedGauges) ? cfg.observedGauges.map((x) => ({ ...x })) : [];
  state.activeRunId = null;
  state.activeRunSeq = null;
  state.draftParentRunId = meta.id;
  state.draftParentSeq = meta.seq;
  state.diffData = null;
  state.xsecPts = [];
  state.selectedCell = null;
  state.lastBundCell = null;
  state.uploadedDem = null;
  if (state.layer === 'diff') setLayer('depth');
  if (cfg.boundaryGeoJson) {
    state.activeBoundaryName = cfg.boundaryName || null;
    await applyBoundaryGeometry(cfg.boundaryGeoJson, false, { fitView: false });
  } else {
    clearBoundaryFocus();
  }
  if (map && cfg.center) {
    map.flyTo({ center: [cfg.center.lon, cfg.center.lat], zoom: cfg.zoom || 12.2, duration: 600 });
  }
  state.staticVectorsDirty = true;
  updateInterventionBar();
  updateFloodPinBar();
  renderFloodCanvas(false);
  setSimPhase('draft');
  $('sim-dir-drawer')?.classList.add('hidden');
  showToast(`Forked #${meta.seq} into new draft — edit and click Run Simulation.`);
}

async function compareWithSimDirRun(otherMeta) {
  if (!otherMeta || otherMeta.kind !== 'simulation') return;
  if (!state.grid?.envelope?.max_depth_m || state.simPhase !== 'results') {
    showToast('Open a completed run first, then click Compare on another run.');
    return;
  }
  const otherBlob = await idbGetRunBlob(otherMeta.id);
  const curMeta = state.grid.metadata;
  const othMeta = otherBlob?.grid?.metadata;
  const curPeak = state.grid.envelope.max_depth_m;
  const othPeak = otherBlob?.grid?.envelope?.max_depth_m;
  if (!othMeta || !othPeak) {
    showToast('Selected run has no stored depth envelope.');
    return;
  }
  const sameGrid = curMeta.cols === othMeta.cols && curMeta.rows === othMeta.rows
    && curMeta.wgs84_bounds.every((v, i) => Math.abs(v - othMeta.wgs84_bounds[i]) < 1e-4);
  if (!sameGrid) {
    showToast('Cannot diff runs with different basin extents — fork a run to compare on the same grid.');
    return;
  }
  const delta = new Float32Array(curPeak.length);
  let improved = 0;
  let worsened = 0;
  let maxRed = 0;
  let maxWors = 0;
  for (let i = 0; i < curPeak.length; i++) {
    if (!curMeta.active_mask[i]) continue;
    const d = (curPeak[i] || 0) - (othPeak[i] || 0);
    delta[i] = d;
    if (d <= -0.015) {
      improved++;
      if (-d > maxRed) maxRed = -d;
    } else if (d >= 0.015) {
      worsened++;
      if (d > maxWors) maxWors = d;
    }
  }
  state.diffData = {
    delta_max_depth_m: delta,
    improved_cells: improved,
    max_reduction_m: maxRed,
    worsened_cells: worsened,
    max_worsening_m: maxWors,
    conservation_valid: true,
    earthwork: { fill_volume_m3: 0 },
  };
  state.frameRasterCache.clear();
  setLayer('diff');
  if ($('inspector')) {
    $('inspector').classList.remove('hidden');
    $('inspector-svg')?.classList.add('hidden');
    $('inspector-title').textContent = `Δh Diff: #${state.activeRunSeq || '?'} vs #${otherMeta.seq}`;
    $('inspector-stats').innerHTML = `
      <div class="mini-stat"><div class="mini-k">Protected</div><div class="mini-v good">${improved} cells (−${fmtDepth(maxRed, 2)})</div></div>
      <div class="mini-stat"><div class="mini-k">Worsened</div><div class="mini-v bad">${worsened} cells (+${fmtDepth(maxWors, 2)})</div></div>
    `;
  }
  if (window.innerWidth < 1440) $('sim-dir-drawer')?.classList.add('hidden');
  showToast(`Client-side Δh Diff computed: #${state.activeRunSeq || '?'} vs #${otherMeta.seq}.`);
}

async function renderSimDirList() {
  const listEl = $('sim-dir-list');
  const sumEl = $('sim-dir-summary');
  if (!listEl) return;
  try {
    const all = await idbAllRunMeta();
    const filter = state.simDirFilter || 'all';
    const items = filter === 'pinned' ? all.filter((x) => x.pinned) : all;
    const totalBytes = all.reduce((acc, x) => acc + (x.sizeBytes || 0), 0);
    if (sumEl) {
      sumEl.textContent = `${all.length} run${all.length === 1 ? '' : 's'} (${(totalBytes / 1_048_576).toFixed(1)} MB) · Auto-saved locally`;
    }
    listEl.replaceChildren();
    if (!items.length) {
      const empty = document.createElement('div');
      empty.style.cssText = 'font-size:12px;color:var(--ink-3);padding:12px 4px;line-height:1.45';
      empty.textContent = filter === 'pinned'
        ? 'No pinned runs yet. Star any run to protect it from auto-cleanup.'
        : 'No simulations yet. Configure a draft and click Run Simulation — completed runs auto-save here.';
      listEl.appendChild(empty);
      return;
    }
    for (const meta of items) {
      const card = document.createElement('div');
      card.className = `sim-dir-card${meta.id === state.activeRunId ? ' active' : ''}`;
      card.addEventListener('click', () => openSimDirRun(meta));

      const head = document.createElement('div');
      head.className = 'sim-dir-head';
      const tw = document.createElement('div');
      tw.className = 'sim-dir-title-wrap';
      const seq = document.createElement('span');
      seq.className = `sim-dir-seq${meta.kind === 'config' ? ' cfg' : ''}`;
      seq.textContent = meta.kind === 'config' ? `CFG #${meta.seq}` : `#${meta.seq}`;
      const nm = document.createElement('span');
      nm.className = 'sim-dir-name';
      nm.textContent = meta.name || `Run #${meta.seq}`;
      nm.title = nm.textContent;
      tw.append(seq, nm);

      const pinBtn = document.createElement('button');
      pinBtn.type = 'button';
      pinBtn.className = `btn btn-xs loc-star-btn${meta.pinned ? ' active' : ''}`;
      pinBtn.textContent = meta.pinned ? '★' : '☆';
      pinBtn.title = meta.pinned ? 'Unpin run' : 'Pin run (keep forever)';
      pinBtn.addEventListener('click', async (ev) => {
        ev.stopPropagation();
        meta.pinned = !meta.pinned;
        await idbPutRun(meta, null);
        await renderSimDirList();
      });
      head.append(tw, pinBtn);
      card.appendChild(head);

      if (meta.parentSeq) {
        const fk = document.createElement('div');
        fk.className = 'sim-dir-fork';
        fk.textContent = `↳ forked from #${meta.parentSeq}`;
        card.appendChild(fk);
      }

      const chips = document.createElement('div');
      chips.className = 'sim-dir-chips';
      const s = meta.summary || {};
      if (meta.kind === 'simulation') {
        const c1 = document.createElement('span');
        c1.className = 'sim-dir-chip';
        c1.textContent = `Peak ${fmtDepth(s.peakDepthM || 0, 2)}`;
        const c2 = document.createElement('span');
        c2.className = 'sim-dir-chip';
        c2.textContent = fmtArea(s.floodedKm2 || 0, 1);
        chips.append(c1, c2);
      }
      if (s.editCount > 0) {
        const c3 = document.createElement('span');
        c3.className = 'sim-dir-chip';
        c3.textContent = `${s.editCount} bund cells`;
        chips.appendChild(c3);
      }
      const cTime = document.createElement('span');
      cTime.className = 'sim-dir-chip';
      cTime.textContent = new Date(meta.createdAt || Date.now()).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });
      chips.appendChild(cTime);
      card.appendChild(chips);

      const acts = document.createElement('div');
      acts.className = 'sim-dir-actions';
      const forkBtn = document.createElement('button');
      forkBtn.type = 'button';
      forkBtn.className = 'btn btn-xs';
      forkBtn.textContent = 'Fork';
      forkBtn.addEventListener('click', (ev) => {
        ev.stopPropagation();
        forkSimDirRun(meta);
      });
      acts.appendChild(forkBtn);

      if (meta.kind === 'simulation' && meta.id !== state.activeRunId && state.simPhase === 'results') {
        const cmpBtn = document.createElement('button');
        cmpBtn.type = 'button';
        cmpBtn.className = 'btn btn-xs';
        cmpBtn.textContent = 'Compare Δh';
        cmpBtn.addEventListener('click', (ev) => {
          ev.stopPropagation();
          compareWithSimDirRun(meta);
        });
        acts.appendChild(cmpBtn);
      }

      const renBtn = document.createElement('button');
      renBtn.type = 'button';
      renBtn.className = 'btn btn-xs';
      renBtn.textContent = 'Rename';
      renBtn.addEventListener('click', async (ev) => {
        ev.stopPropagation();
        const nextName = window.prompt('Rename simulation:', meta.name || '');
        if (nextName && nextName.trim()) {
          meta.name = nextName.trim();
          await idbPutRun(meta, null);
          await renderSimDirList();
        }
      });
      acts.appendChild(renBtn);

      const delBtn = document.createElement('button');
      delBtn.type = 'button';
      delBtn.className = 'btn btn-xs';
      delBtn.textContent = 'Delete';
      delBtn.addEventListener('click', async (ev) => {
        ev.stopPropagation();
        await idbDeleteRun(meta.id);
        if (state.activeRunId === meta.id) {
          state.activeRunId = null;
          state.activeRunSeq = null;
        }
        await refreshSavedCountBadge();
        await renderSimDirList();
      });
      acts.appendChild(delBtn);

      card.appendChild(acts);
      listEl.appendChild(card);
    }
  } catch (err) {
    listEl.textContent = `Could not read local runs: ${err && err.message ? err.message : String(err)}`;
  }
}

async function refreshSavedCountBadge() {
  try {
    const items = await idbAllRunMeta();
    const btn = $('btn-saved-sims');
    if (btn) btn.textContent = `Runs (${items.length})`;
  } catch (_) {}
}

async function openSavedSimsModal() {
  const dir = $('sim-dir-drawer');
  if (!dir) return;
  const opening = dir.classList.contains('hidden');
  dir.classList.toggle('hidden', !opening);
  if (opening) await renderSimDirList();
}

async function saveCurrentToIdb(kind) {
  try {
    const c = map ? map.getCenter() : { lat: state.center.lat, lng: state.center.lon };
    const z = map ? map.getZoom() : state.initZoom;
    const rawName = $('save-sim-name')?.value?.trim();
    const locLabel = $('coord-search')?.value?.trim()
      || $('preset-select')?.selectedOptions?.[0]?.textContent?.trim()
      || `${c.lat.toFixed(3)}, ${c.lng.toFixed(3)}`;
    const horizon = state.simConfig.horizon || '5d';
    const name = rawName || `${locLabel} · ${horizon} (${kind === 'simulation' ? 'Result' : 'Config'})`;

    const item = {
      id: `sim-${Date.now()}-${Math.random().toString(36).slice(2, 7)}`,
      kind,
      name,
      createdAt: Date.now(),
      config: {
        center: { lat: Number(c.lat.toFixed(5)), lon: Number(c.lng.toFixed(5)) },
        zoom: Number(z.toFixed(2)),
        simConfig: { ...state.simConfig },
        interventions: JSON.parse(JSON.stringify(state.interventions || [])),
        baselineInterventions: JSON.parse(JSON.stringify(state.baselineInterventions || [])),
        antecedentMm: state.antecedentMm || 0,
        observedGauges: JSON.parse(JSON.stringify(state.observedGauges || [])),
      },
      sizeBytes: 0,
    };

    if (kind === 'simulation') {
      if (!state.grid || !state.lastFramesBuf || !state.frames.length) {
        showToast('No simulation result loaded yet. Click Run Simulation first.');
        return;
      }
      const bufCopy = state.lastFramesBuf.slice(0);
      item.result = {
        grid: JSON.parse(JSON.stringify(state.grid)),
        framesBuf: bufCopy,
        runId: state.runId || item.id,
        engine: state.activeEngine || 'Saved',
      };
      item.sizeBytes = bufCopy.byteLength + JSON.stringify(item.result.grid).length;
    }

    await idbPutSaved(item);
    if ($('save-sim-name')) $('save-sim-name').value = '';
    await refreshSavedCountBadge();
    await renderSavedSimsList();
    showToast(kind === 'simulation' ? `Saved simulation "${name}" with full result.` : `Saved config "${name}".`);
  } catch (err) {
    showToast(`Save failed: ${err && err.message ? err.message : String(err)}`);
  }
}

async function openSavedItem(id) {
  try {
    const item = await idbGetSaved(id);
    if (!item) {
      showToast('Saved item not found.');
      return;
    }
    const cfg = item.config || {};
    if (cfg.simConfig) Object.assign(state.simConfig, cfg.simConfig);
    if (cfg.center) state.center = { lat: cfg.center.lat, lon: cfg.center.lon };
    state.interventions = Array.isArray(cfg.interventions) ? cfg.interventions : [];
    state.baselineInterventions = Array.isArray(cfg.baselineInterventions) ? cfg.baselineInterventions : [];
    if (typeof cfg.antecedentMm === 'number') state.antecedentMm = cfg.antecedentMm;
    if (Array.isArray(cfg.observedGauges)) state.observedGauges = cfg.observedGauges;
    updateInterventionBar();
    if (map && cfg.center) {
      map.flyTo({ center: [cfg.center.lon, cfg.center.lat], zoom: cfg.zoom || 11.4, duration: 700 });
    }
    $('saved-sims-modal')?.classList.add('hidden');
    if (item.kind === 'simulation' && item.result?.grid && item.result?.framesBuf) {
      const frames = decodeFramesBin(item.result.framesBuf);
      if (frames.length) {
        state.frameRasterCache.clear();
        state.playing = false;
        applyLoadedSimulation(item.result.grid, frames, item.result.runId || item.id, 'Saved · Instant', item.result.framesBuf);
        if (state.profile === 'researcher') setSimPhase('results');
        showToast(`Loaded "${item.name}" instantly (no re-solve needed).`);
        return;
      }
    }
    if (state.profile === 'researcher') setSimPhase('draft');
    showToast(`Loaded config "${item.name}". Click Run Simulation when ready.`);
  } catch (err) {
    showToast(`Failed to open saved item: ${err && err.message ? err.message : String(err)}`);
  }
}

async function deleteSavedItem(id) {
  try {
    await idbDeleteSaved(id);
    await refreshSavedCountBadge();
    await renderSavedSimsList();
  } catch (err) {
    showToast(`Delete failed: ${err && err.message ? err.message : String(err)}`);
  }
}

async function renderSavedSimsList() {
  const list = $('saved-sims-list');
  const summary = $('saved-sims-summary');
  if (!list) return;
  try {
    const items = await idbAllSaved();
    if (summary) {
      const totalKb = Math.round(items.reduce((acc, x) => acc + (x.sizeBytes || 0), 0) / 1024);
      summary.textContent = `${items.length} saved item${items.length === 1 ? '' : 's'}${totalKb > 0 ? ` · ${(totalKb / 1024).toFixed(1)} MB` : ''}`;
    }
    list.replaceChildren();
    if (!items.length) {
      const empty = document.createElement('div');
      empty.style.cssText = 'padding:14px;text-align:center;font-size:12px;color:var(--ink-3);background:var(--hud-well);border-radius:8px';
      empty.textContent = 'No saved configs or simulations yet. Save your current setup or completed run above.';
      list.appendChild(empty);
      return;
    }
    for (const item of items) {
      const row = document.createElement('div');
      row.className = 'saved-sim-row';

      const info = document.createElement('div');
      info.className = 'saved-sim-info';

      const title = document.createElement('div');
      title.className = 'saved-sim-title';
      const badge = document.createElement('span');
      badge.className = `saved-sim-badge ${item.kind === 'simulation' ? 'sim' : 'cfg'}`;
      badge.textContent = item.kind === 'simulation' ? 'Result Saved' : 'Config';
      const nameSpan = document.createElement('span');
      nameSpan.textContent = item.name || 'Untitled';
      title.appendChild(badge);
      title.appendChild(nameSpan);

      const sc = item.config?.simConfig || {};
      const editsCount = (item.config?.interventions || []).length;
      const when = item.createdAt ? new Date(item.createdAt).toLocaleString([], { month: 'short', day: 'numeric', hour: '2-digit', minute: '2-digit' }) : '';
      const sizeStr = item.sizeBytes ? ` · ${(item.sizeBytes / (1024 * 1024)).toFixed(2)} MB` : '';
      const meta = document.createElement('div');
      meta.className = 'saved-sim-meta';
      meta.textContent = `${sc.horizon || '5d'} · ${sc.startDry ? 'Dry start' : 'Live t=0'} · ${editsCount} edits · ${when}${sizeStr}`;

      info.appendChild(title);
      info.appendChild(meta);

      const actions = document.createElement('div');
      actions.className = 'saved-sim-actions';
      const openBtn = document.createElement('button');
      openBtn.className = 'btn btn-xs primary';
      openBtn.textContent = 'Open';
      openBtn.addEventListener('click', () => openSavedItem(item.id));
      const delBtn = document.createElement('button');
      delBtn.className = 'btn btn-xs';
      delBtn.textContent = 'Delete';
      delBtn.addEventListener('click', () => deleteSavedItem(item.id));
      actions.appendChild(openBtn);
      actions.appendChild(delBtn);

      row.appendChild(info);
      row.appendChild(actions);
      list.appendChild(row);
    }
  } catch (err) {
    list.textContent = `Could not read browser storage: ${err && err.message ? err.message : String(err)}`;
  }
}

function buildShareUrl() {
  const qs = new URLSearchParams();
  const c = map ? map.getCenter() : { lat: state.center.lat, lng: state.center.lon };
  qs.set('tab', state.profile === 'researcher' ? 'simulate' : 'live');
  qs.set('lat', c.lat.toFixed(4));
  qs.set('lon', c.lng.toFixed(4));
  if (map) qs.set('zoom', map.getZoom().toFixed(2));
  qs.set('basemap', state.basemap);
  qs.set('layer', state.layer);
  if (state.boundaryGeoJson && state.activeBoundaryName) {
    qs.set('focus', state.activeBoundaryName);
  }
  // Live tab shares ONLY the clean map view; Simulate tab shares the full scenario
  if (state.profile === 'researcher') {
    const sc = state.simConfig;
    qs.set('horizon', sc.horizon || '2d');
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
  }
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
  if ($('val-cutoff')) {
    const cm = Math.round((p.depthCutoffM || 0.08) * 100);
    $('val-cutoff').textContent = state.userOverrideCutoff ? `${cm} cm` : `${cm} cm (Auto)`;
  }
  const fPct = Math.round((p.floodOpacity || 0.95) * 100);
  $('opt-flood-opacity').value = String(fPct);
  $('val-flood-opacity').textContent = `${fPct}%`;
  const rPct = Math.round((p.riverOpacity || 0.72) * 100);
  $('opt-river-opacity').value = String(rPct);
  $('val-river-opacity').textContent = `${rPct}%`;
  $('opt-units').value = p.units;
  if ($('opt-clock')) $('opt-clock').value = p.clock;
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
        map.setTerrain(state.is3D && state.profile !== 'public' && map.getSource('arus-terrarium-dem')
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
    const floodOp = floodOpacityExpr(p.floodOpacity);
    if (map.getLayer('arus-water-smooth')) map.setPaintProperty('arus-water-smooth', 'raster-opacity', floodOp);
    for (const basinId of state.liveOverlays.keys()) {
      const lid = `arus-live-layer-${basinId}`;
      if (map.getLayer(lid)) map.setPaintProperty(lid, 'raster-opacity', floodOp);
    }
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
    depthCutoffM: parseFloat($('opt-cutoff').value) || 0.08,
    floodOpacity: (parseInt($('opt-flood-opacity').value, 10) || 95) / 100,
    riverOpacity: (parseInt($('opt-river-opacity').value, 10) || 72) / 100,
    units: $('opt-units').value,
    clock: $('opt-clock')?.value || state.displayPrefs.clock || '12h',
    timezone: state.displayPrefs.timezone || 'local',
    showMetrics: $('opt-show-metrics').checked,
    showLegend: $('opt-show-legend').checked,
  };
  state.userOverrideCutoff = true;
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

function updateStormDrainLabel(pct) {
  const el = $('sim-storm-drain-val');
  if (!el) return;
  const tag = pct <= 10 ? 'Clogged / None' : pct <= 35 ? 'Poor' : pct <= 65 ? 'Average' : pct <= 85 ? 'Good' : 'Modern Trunk';
  el.textContent = `${pct}% · ${tag}`;
}

function updateStormDrainGridStatus() {
  const st = $('sim-storm-drain-status');
  const clr = $('sim-storm-drain-clear');
  if (!st) return;
  if (Array.isArray(state.stormDrainGrid) && state.stormDrainGrid.length > 0) {
    st.textContent = `Custom grid (${state.stormDrainGrid.length} cells)`;
    if (clr) clr.classList.remove('hidden');
  } else {
    st.textContent = 'Baseline default';
    if (clr) clr.classList.add('hidden');
  }
}

function populateSimModal() {
  const sc = state.simConfig;
  switchSimTab(sc.mode || 'forecast');
  $('sim-horizon').value = sc.horizon || '2d';
  if ($('sim-start-mode')) $('sim-start-mode').value = sc.startDry ? 'dry' : 'live';
    const multStr = String(Number(sc.rainMult));
    $('sim-rain-mult').value = multStr;
  $('sim-stress-mm-day').value = String(sc.stressMmDay);
  $('sim-stress-days').value = String(sc.stressDays);
  $('sim-stress-pad').value = String(sc.stressPadM);
  const site = state.stressSite || state.center;
  $('sim-stress-coord').textContent = `${site.lat.toFixed(4)}°, ${site.lon.toFixed(4)}°`;
  if ($('sim-start-dry')) $('sim-start-dry').checked = sc.startDry;
  $('sim-enable-evap').checked = sc.enableEvap;
  $('sim-enable-drain').checked = sc.enableDrain;
  const sdPct = Math.round((Number.isFinite(sc.stormDrainScore) ? sc.stormDrainScore : 0.5) * 100);
  if ($('sim-storm-drain')) {
    $('sim-storm-drain').value = String(sdPct);
    updateStormDrainLabel(sdPct);
  }
  updateStormDrainGridStatus();
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

function applySimModalDraft(shouldRun = false) {
  const sc = state.simConfig;
  sc.horizon = $('sim-horizon').value || '2d';
  sc.rainMult = parseFloat($('sim-rain-mult').value) || 1.0;
  sc.stressMmDay = parseFloat($('sim-stress-mm-day').value) || 150;
  sc.stressDays = parseInt($('sim-stress-days').value, 10) || 3;
  sc.stressPadM = parseFloat($('sim-stress-pad').value) || 0;
  sc.startDry = $('sim-start-mode') ? $('sim-start-mode').value === 'dry' : Boolean($('sim-start-dry')?.checked);
  if ($('sim-start-dry')) $('sim-start-dry').checked = sc.startDry;
  sc.enableEvap = $('sim-enable-evap').checked;
  sc.enableDrain = $('sim-enable-drain').checked;
  if ($('sim-storm-drain')) {
    sc.stormDrainScore = Math.min(1, Math.max(0, (parseFloat($('sim-storm-drain').value) || 0) / 100));
  }
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
  updateSimDraftSummary();

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
  if (shouldRun) {
    runSimulation(runCompare);
  } else {
    showToast('Parameters applied to draft — click Run Simulation when ready.');
  }
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
  const basinTitle = String(c.basin_id || '').replace(/(^|[-_])\w/g, (m) => m.toUpperCase()).replace(/[-_]/g, ' ');
  const parts = [];
  parts.push(`<div><strong>Basin:</strong> ${basinTitle} · <strong>Cycle:</strong> ${c.cycle_iso}</div>`);
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
    const resp = await fetch(apiUrl('api/v1/context'));
    if (!resp.ok) return;
    const data = await resp.json();
    state.serverContext = data;
    const ctx = data.context;
    if (ctx && Array.isArray(ctx.wgs84_bounds) && ctx.wgs84_bounds.length === 4) {
      const [minLon, minLat, maxLon, maxLat] = ctx.wgs84_bounds;
      const inBounds = state.center.lon >= minLon && state.center.lon <= maxLon
        && state.center.lat >= minLat && state.center.lat <= maxLat;
      if (inBounds) {
        if (Number.isFinite(ctx.antecedent_rain_mm) && ctx.antecedent_rain_mm > 0) {
          state.antecedentMm = ctx.antecedent_rain_mm;
        }
        if (Array.isArray(ctx.observed_gauges) && ctx.observed_gauges.length > 0 && state.observedGauges.length === 0) {
          state.observedGauges = ctx.observed_gauges;
        }
        if (Array.isArray(ctx.interventions) && ctx.interventions.length > 0) {
          state.liveInterventions = ctx.interventions;
        }
        if (ctx.spatial_rain_grid) {
          state.spatialRainGrid = ctx.spatial_rain_grid;
          if (!qs.has('rain')) state.simConfig.rainfallMmHr = 0;
          const nHrs = ctx.spatial_rain_grid.hourly_mm?.[0]?.length || 0;
          if (nHrs > 0 && !qs.has('span_hours')) {
            state.spanHours = nHrs;
            const parsedStart = Date.parse(ctx.cycle_iso);
            if (Number.isFinite(parsedStart)) state.startEpochMs = parsedStart;
            state.tzLabel = 'MYT';
          }
        }
        if (Array.isArray(ctx.hourly_tide_m) && ctx.hourly_tide_m.length > 0) state.hourlyTideM = ctx.hourly_tide_m;
      }
    }
    if (ctx && Array.isArray(ctx.interventions) && ctx.interventions.length > 0) {
      state.liveInterventions = ctx.interventions;
      state.staticVectorsDirty = true;
    }
    renderServerContextBox();
    renderIncidentMarkers();
    if (state.grid) renderFloodCanvas(false);
  } catch (_) { /* optional server context */ }
}

function applyUserPrefs(qs) {
  if (qs.get('presentation') === '1' || qs.get('presentation') === 'true') return;
  if (qs.get('fixed') === '1' || qs.get('fixed') === 'true') return;
  const prefs = loadUserPrefs();
  if (prefs.last_center && !qs.has('lat') && !qs.has('lon')) {
    const lc = prefs.last_center;
    if (Number.isFinite(lc.lat) && Number.isFinite(lc.lon)) {
      state.center = { lat: lc.lat, lon: lc.lon };
      if (Number.isFinite(lc.zoom) && !qs.has('zoom')) state.initZoom = lc.zoom;
      if (Number.isFinite(lc.pitch) && !qs.has('pitch')) state.initPitch = lc.pitch;
      if (Number.isFinite(lc.bearing) && !qs.has('bearing')) state.initBearing = lc.bearing;
      if (lc.label) setCustomPresetLabel(lc.label);
    }
  }
  if ($('btn-play')) $('btn-play').textContent = state.playing ? '❚❚' : '▶';
  if ($('btn-env')) {
    $('btn-env').textContent = state.envelopeMode ? 'Peak' : 'Step';
    $('btn-env').classList.toggle('active', state.envelopeMode);
  }
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
    const resp = await fetch(apiUrl('api/v1/config'));
    if (!resp.ok) return;
    const cfg = await resp.json();
    state.serverConfig = cfg;
    fetchCatalogBoundaries();
    if (cfg.instance_title) document.title = cfg.instance_title;
    if (Array.isArray(cfg.presets) && cfg.presets.length > 0) {
      SERVER_PRESETS = cfg.presets.map((p) => ({
        id: p.id,
        name: p.name,
        lon: p.lon,
        lat: p.lat,
        zoom: p.zoom,
        pitch: Number.isFinite(p.pitch) ? p.pitch : 0,
        bearing: Number.isFinite(p.bearing) ? p.bearing : 0,
        boundaryQuery: p.boundary_query || null,
        liveBasin: p.live_basin || null,
        isServer: true,
      }));
      rebuildPresetDropdown();
    }
    const defaultProfile = cfg.default_ui_profile || (cfg.mode === 'hosted' ? 'public' : 'researcher');
    setUiProfile(defaultProfile, false);
    const f = cfg.features || {};
    const hide = (el) => { if (el) el.style.display = 'none'; };
    if (f.allow_simulation === false) {
      hide($('profile-tabs'));
      hide($('btn-open-sim'));
      hide($('btn-saved-sims'));
      setUiProfile('public', false);
    }
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
    if (f.allow_mp4_export === false && typeof MediaRecorder === 'undefined') {
      hide($('btn-rec'));
    }
    if (f.allow_direct_nwp_fetch === false) {
      hide($('nwp-section'));
    }
    if (f.allow_gis_export === false) {
      hide($('btn-export-asc'));
      hide($('btn-export-csv'));
    }
    if (cfg.mode === 'hosted' && !qs.get('presentation') && !qs.get('fixed')) {
      try {
        if (!window.localStorage.getItem('arus.about.ack.v1') && $('about-modal')) {
          $('about-modal').classList.remove('hidden');
        }
      } catch (_) {}
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

// Privacy-first client error feedback (zero PII/cookies, max 5 reports per session)
const reportedErrors = new Set();
function reportClientError(kind, message, source = '') {
  if (reportedErrors.size >= 5) return;
  const msg = String(message || '').slice(0, 500);
  const key = `${kind}:${msg.slice(0, 120)}`;
  if (!msg || reportedErrors.has(key)) return;
  reportedErrors.add(key);
  try {
    fetch(apiUrl('api/v1/telemetry/error'), {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      keepalive: true,
      body: JSON.stringify({
        kind,
        message: msg,
        source: String(source || '').slice(0, 160),
        profile: state.profile,
        engine: state.activeEngine,
      }),
    }).catch(() => {});
  } catch (_) {}
}
window.addEventListener('error', (e) => {
  reportClientError('error', e.message || 'Script error', `${e.filename || ''}:${e.lineno || 0}`);
});
window.addEventListener('unhandledrejection', (e) => {
  reportClientError('unhandledrejection', e.reason?.message || String(e.reason || 'Promise rejected'));
});

function applyStartupParams() {
  const qs = new URLSearchParams(window.location.search);
  const tab = qs.get('tab');
  if (tab === 'simulate') setUiProfile('researcher', false);
  else if (tab === 'live') setUiProfile('public', false);
  const horizon = qs.get('horizon');
  if (horizon && ['6h', '1d', '2d', '3d', '5d'].includes(horizon)) state.simConfig.horizon = horizon;
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
    state.displayPrefs.basemap = basemap;
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
