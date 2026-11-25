//! Hydrological DEM conditioning, D8 flow accumulation, river channel carving,
//! pre-state channel wetting, depression detection, and spatial bilinear sampling.

use std::cmp::Ordering as CmpOrdering;
use std::collections::BinaryHeap;

use arus_domain::{GridSpec, SpatialReference};
use arus_geo::TerrainGrid;

use super::ObservedGaugeSeed;

pub(super) fn compute_wgs84_bounds(
    center_lon: f64,
    center_lat: f64,
    cols: usize,
    rows: usize,
    dx_m: f64,
) -> [f64; 4] {
    let width_m = cols as f64 * dx_m;
    let height_m = rows as f64 * dx_m;
    let m_per_deg_lat = 111_320.0;
    let m_per_deg_lon = (111_320.0 * center_lat.to_radians().cos()).max(10_000.0);
    let half_dlat = (height_m * 0.5) / m_per_deg_lat;
    let half_dlon = (width_m * 0.5) / m_per_deg_lon;
    [
        center_lon - half_dlon,
        center_lat - half_dlat,
        center_lon + half_dlon,
        center_lat + half_dlat,
    ]
}

/// Builds a georeferenced synthetic sloping river-valley basin centered at `(center_lon, center_lat)`
/// with a winding central channel depression and corner nodata cells so coverage masks are visible.
pub(super) fn build_synthetic_valley_terrain(
    center_lon: f64,
    center_lat: f64,
    cols: usize,
    rows: usize,
    dx_m: f64,
) -> Result<(TerrainGrid, [f64; 4]), String> {
    let bounds = compute_wgs84_bounds(center_lon, center_lat, cols, rows, dx_m);

    let mut cells = Vec::with_capacity(cols * rows);
    for r in 0..rows {
        let ny = (r as f64 + 0.5) / (rows as f64);
        for c in 0..cols {
            let nx = (c as f64 + 0.5) / (cols as f64);
            if c >= cols.saturating_sub(2) && r >= rows.saturating_sub(2) {
                cells.push(None);
                continue;
            }
            let channel_y = 0.5 + 0.18 * (nx * std::f64::consts::TAU * 1.15).sin();
            let dist = (ny - channel_y).abs();
            let bank = (dist * dist * 5.2).min(2.6);
            let slope = (1.0 - nx) * 1.35;
            let pocket_d2 = (nx - 0.52).powi(2) + (ny - 0.36).powi(2);
            let pocket = -0.55 * (-pocket_d2 / 0.022).exp();
            let elev = (1.4 + slope + bank + pocket).max(0.1);
            cells.push(Some(elev));
        }
    }

    let grid = TerrainGrid::new(
        GridSpec {
            columns: cols,
            rows,
            dx_m,
            dy_m: dx_m,
        },
        SpatialReference {
            projected_crs: format!("local-metric-tangent@{center_lat:.4},{center_lon:.4}"),
            vertical_datum: "local-synthetic-datum".to_owned(),
            origin_m: [0.0, 0.0],
        },
        "SyntheticDemoBasin",
        cells,
    )
    .map_err(|e| e.to_string())?;
    Ok((grid, bounds))
}

#[derive(Clone, Copy)]
struct MinF64 {
    val: f64,
    idx: usize,
}

impl PartialEq for MinF64 {
    fn eq(&self, other: &Self) -> bool {
        self.val.to_bits() == other.val.to_bits() && self.idx == other.idx
    }
}
impl Eq for MinF64 {}
impl PartialOrd for MinF64 {
    fn partial_cmp(&self, other: &Self) -> Option<CmpOrdering> {
        Some(self.cmp(other))
    }
}
impl Ord for MinF64 {
    fn cmp(&self, other: &Self) -> CmpOrdering {
        other
            .val
            .total_cmp(&self.val)
            .then_with(|| other.idx.cmp(&self.idx))
    }
}

/// Barnes Priority-Flood depression filling with 2-pass 3x3 canopy/noise smoothing so 30 m
/// Terrarium DEM noise pits do not trap runoff into artificial multi-metre polka-dot wells.
pub(super) fn condition_hydrological_dem(elevations: &mut [Option<f64>], cols: usize, rows: usize) {
    let n = cols * rows;
    if elevations.len() != n || cols < 3 || rows < 3 {
        return;
    }
    // 2-pass gentle 3x3 smoothing removes 30 m SRTM/Terrarium canopy & building noise spikes.
    let dirs: [(isize, isize); 8] = [
        (-1, 0),
        (1, 0),
        (0, -1),
        (0, 1),
        (-1, -1),
        (-1, 1),
        (1, -1),
        (1, 1),
    ];
    for _ in 0..2 {
        let prev = elevations.to_vec();
        for r in 0..rows {
            for c in 0..cols {
                let idx = r * cols + c;
                let Some(z0) = prev[idx] else { continue };
                let mut sum = 0.0;
                let mut cnt = 0.0;
                for (dr, dc) in dirs {
                    let nr = r as isize + dr;
                    let nc = c as isize + dc;
                    if nr >= 0
                        && nc >= 0
                        && nr < rows as isize
                        && nc < cols as isize
                        && let Some(nz) = prev[(nr as usize) * cols + (nc as usize)]
                    {
                        sum += nz;
                        cnt += 1.0;
                    }
                }
                if cnt > 0.0 {
                    elevations[idx] = Some(z0 * 0.5 + (sum / cnt) * 0.5);
                }
            }
        }
    }

    let mut visited = vec![false; n];
    let mut heap: BinaryHeap<MinF64> = BinaryHeap::with_capacity(n);

    for r in 0..rows {
        for c in 0..cols {
            let idx = r * cols + c;
            let Some(z) = elevations[idx] else { continue };
            let is_outer = r == 0 || r + 1 == rows || c == 0 || c + 1 == cols;
            let touches_sea = !is_outer
                && [(-1isize, 0isize), (1, 0), (0, -1), (0, 1)]
                    .into_iter()
                    .any(|(dr, dc)| {
                        let nr = (r as isize + dr) as usize;
                        let nc = (c as isize + dc) as usize;
                        elevations[nr * cols + nc].is_none()
                    });
            if is_outer || touches_sea {
                visited[idx] = true;
                heap.push(MinF64 { val: z, idx });
            }
        }
    }

    let eps = 0.015;

    while let Some(cur) = heap.pop() {
        let r = (cur.idx / cols) as isize;
        let c = (cur.idx % cols) as isize;
        for (dr, dc) in dirs {
            let nr = r + dr;
            let nc = c + dc;
            if nr < 0 || nc < 0 || nr >= rows as isize || nc >= cols as isize {
                continue;
            }
            let nidx = (nr as usize) * cols + (nc as usize);
            if visited[nidx] {
                continue;
            }
            visited[nidx] = true;
            if let Some(nz) = elevations[nidx] {
                let filled = if nz < cur.val + eps {
                    cur.val + eps
                } else {
                    nz
                };
                elevations[nidx] = Some(filled);
                heap.push(MinF64 {
                    val: filled,
                    idx: nidx,
                });
            }
        }
    }
}

/// Computes D8 contributing drainage area (in cell count) on a hydrologically conditioned DEM.
pub(super) fn compute_d8_flow_accum(
    elevations: &[Option<f64>],
    cols: usize,
    rows: usize,
) -> Vec<u32> {
    let n = cols * rows;
    let mut order: Vec<(f64, usize)> = elevations
        .iter()
        .enumerate()
        .filter_map(|(i, z)| z.map(|v| (v, i)))
        .collect();
    order.sort_by(|a, b| b.0.total_cmp(&a.0));

    let mut accum = vec![1u32; n];
    let dirs: [(isize, isize); 8] = [
        (-1, 0),
        (1, 0),
        (0, -1),
        (0, 1),
        (-1, -1),
        (-1, 1),
        (1, -1),
        (1, 1),
    ];
    for (z, idx) in order {
        let r = (idx / cols) as isize;
        let c = (idx % cols) as isize;
        let mut best_slope = 0.0;
        let mut best_n = None;
        for (dr, dc) in dirs {
            let nr = r + dr;
            let nc = c + dc;
            if nr < 0 || nc < 0 || nr >= rows as isize || nc >= cols as isize {
                continue;
            }
            let nidx = (nr as usize) * cols + (nc as usize);
            if let Some(nz) = elevations[nidx] {
                let dist = if dr != 0 && dc != 0 {
                    std::f64::consts::SQRT_2
                } else {
                    1.0
                };
                let slope = (z - nz) / dist;
                if slope > best_slope {
                    best_slope = slope;
                    best_n = Some(nidx);
                }
            }
        }
        if let Some(target) = best_n {
            accum[target] = accum[target].saturating_add(accum[idx]);
        }
    }
    accum
}

/// Builds a channel-only / valley-only pre-state depth array at `t = 0`.
pub(super) fn build_channel_prestate(
    elevations: &[Option<f64>],
    cols: usize,
    rows: usize,
    bounds: [f64; 4],
    antecedent_rain_mm: f64,
    gauges: &[ObservedGaugeSeed],
) -> Vec<f64> {
    let n = cols * rows;
    let mut depths = vec![0.0f64; n];
    let accum = compute_d8_flow_accum(elevations, cols, rows);

    let mut valid_z: Vec<f64> = elevations.iter().filter_map(|&z| z).collect();
    valid_z.sort_by(f64::total_cmp);
    let low_z_cutoff = if valid_z.is_empty() {
        10.0
    } else {
        valid_z[(valid_z.len() * 35) / 100]
    };
    let channel_accum_min = ((n as f64).sqrt() * 0.45).round().max(8.0) as u32;

    if antecedent_rain_mm > 0.0 {
        let scale = (antecedent_rain_mm / 100.0).clamp(0.15, 1.5);
        let depressions = detect_real_depressions(elevations, cols, rows);
        for idx in 0..n {
            let Some(z) = elevations[idx] else { continue };
            let r = idx / cols;
            let c = idx % cols;
            let touches_sea = [(-1isize, 0isize), (1, 0), (0, -1), (0, 1)]
                .into_iter()
                .any(|(dr, dc)| {
                    let nr = r as isize + dr;
                    let nc = c as isize + dc;
                    nr >= 0
                        && nc >= 0
                        && nr < rows as isize
                        && nc < cols as isize
                        && elevations[(nr as usize) * cols + (nc as usize)].is_none()
                });
            if touches_sea {
                continue;
            }
            let a = accum[idx];
            if a >= channel_accum_min && z <= low_z_cutoff + 12.0 {
                let channel_factor = ((a as f64) / (channel_accum_min as f64)).ln_1p().min(2.2);
                depths[idx] = (0.08 * scale * channel_factor).min(0.28);
            } else if antecedent_rain_mm >= 50.0 && depressions[idx] && z <= low_z_cutoff + 12.0 {
                // Real lowland topographic depressions retain standing water after heavy multi-day rain
                depths[idx] = depths[idx].max((0.10 * scale).min(0.22));
            }
        }
    }

    let [min_lon, min_lat, max_lon, max_lat] = bounds;
    let lon_span = (max_lon - min_lon).max(1e-6);
    let lat_span = (max_lat - min_lat).max(1e-6);
    for g in gauges {
        if !g.depth_m.is_finite() || g.depth_m <= 0.0 {
            continue;
        }
        let fx = ((g.lon - min_lon) / lon_span) * (cols as f64);
        let fy = ((g.lat - min_lat) / lat_span) * (rows as f64);
        if fx < -2.0 || fx > (cols as f64 + 2.0) || fy < -2.0 || fy > (rows as f64 + 2.0) {
            continue;
        }
        let gc = (fx.floor() as isize).clamp(0, cols as isize - 1);
        let gr = (fy.floor() as isize).clamp(0, rows as isize - 1);
        let rad = (g.radius_cells.clamp(1, 6)) as isize;

        let mut local_min_z = f64::INFINITY;
        for dr in -rad..=rad {
            for dc in -rad..=rad {
                let nr = gr + dr;
                let nc = gc + dc;
                if nr >= 0
                    && nc >= 0
                    && nr < rows as isize
                    && nc < cols as isize
                    && let Some(z) = elevations[(nr as usize) * cols + (nc as usize)]
                    && z < local_min_z
                {
                    local_min_z = z;
                }
            }
        }
        if !local_min_z.is_finite() {
            continue;
        }

        let rad_f = rad as f64;
        for dr in -rad..=rad {
            for dc in -rad..=rad {
                let nr = gr + dr;
                let nc = gc + dc;
                if nr < 0 || nc < 0 || nr >= rows as isize || nc >= cols as isize {
                    continue;
                }
                let dist = ((dr * dr + dc * dc) as f64).sqrt();
                if dist > rad_f {
                    continue;
                }
                let idx = (nr as usize) * cols + (nc as usize);
                if let Some(z) = elevations[idx] {
                    let elev_drop = ((z - local_min_z) / 14.0).clamp(0.0, 1.0);
                    let w = (1.0 - dist / (rad_f + 0.6)) * (1.0 - elev_drop * 0.75);
                    let seeded = (g.depth_m * w).min(0.55);
                    if seeded > depths[idx] {
                        depths[idx] = seeded;
                    }
                }
            }
        }
    }

    let mut smoothed = depths.clone();
    for r in 1..rows.saturating_sub(1) {
        for c in 1..cols.saturating_sub(1) {
            let idx = r * cols + c;
            if elevations[idx].is_none() {
                continue;
            }
            let avg = (depths[idx] * 2.0
                + depths[idx - 1]
                + depths[idx + 1]
                + depths[idx - cols]
                + depths[idx + cols])
                / 6.0;
            smoothed[idx] = avg;
        }
    }
    smoothed
}

/// Carves sub-grid D8 river thalwegs scaled by cell width (`15 m / dx_m`) so coarse screening
/// grids guide drainage without creating artificial `200 m`-wide deep canyons.
pub(super) fn burn_river_channels(
    elevations: &mut [Option<f64>],
    cols: usize,
    rows: usize,
    dx_m: f64,
) {
    let n = cols * rows;
    if elevations.len() != n || cols < 4 || rows < 4 {
        return;
    }
    let width_ratio = (15.0 / dx_m.max(15.0)).clamp(0.08, 0.50);
    let accum = compute_d8_flow_accum(elevations, cols, rows);
    let min_accum = ((n as f64).sqrt() * 0.45).round().max(8.0) as u32;
    for idx in 0..n {
        if let Some(z) = elevations[idx]
            && accum[idx] >= min_accum
        {
            let depth_carve = (((accum[idx] as f64) / (min_accum as f64)).ln_1p() * 0.35)
                .clamp(0.12, 0.80)
                * width_ratio;
            elevations[idx] = Some(z - depth_carve);
        }
    }
}

/// Identifies natural topographic depressions/backswamps (> 0.25 m lower than all 8 neighbors).
pub(super) fn detect_real_depressions(
    elevations: &[Option<f64>],
    cols: usize,
    rows: usize,
) -> Vec<bool> {
    let mut mask = vec![false; cols * rows];
    let dirs: [(isize, isize); 8] = [
        (-1, 0),
        (1, 0),
        (0, -1),
        (0, 1),
        (-1, -1),
        (-1, 1),
        (1, -1),
        (1, 1),
    ];
    for r in 1..rows.saturating_sub(1) {
        for c in 1..cols.saturating_sub(1) {
            let idx = r * cols + c;
            let Some(z) = elevations[idx] else { continue };
            let mut min_nb = f64::INFINITY;
            let mut touches_sea = false;
            for (dr, dc) in dirs {
                let nidx = ((r as isize + dr) as usize) * cols + ((c as isize + dc) as usize);
                match elevations[nidx] {
                    Some(nz) if nz < min_nb => min_nb = nz,
                    Some(_) => {}
                    None => touches_sea = true,
                }
            }
            if !touches_sea && min_nb.is_finite() && (min_nb - z) > 0.25 {
                mask[idx] = true;
            }
        }
    }
    mask
}

/// Bilinearly interpolates a coarse `nx x ny` spatial grid at `(u, v) in [0, 1]^2`.
pub(super) fn bilinear_sample_grid(values: &[f64], nx: usize, ny: usize, u: f64, v: f64) -> f64 {
    if values.len() != nx * ny || nx == 0 || ny == 0 {
        return 0.0;
    }
    let fx = u.clamp(0.0, 1.0) * ((nx - 1) as f64);
    let fy = v.clamp(0.0, 1.0) * ((ny - 1) as f64);
    let x0 = (fx.floor() as usize).min(nx - 1);
    let y0 = (fy.floor() as usize).min(ny - 1);
    let x1 = (x0 + 1).min(nx - 1);
    let y1 = (y0 + 1).min(ny - 1);
    let tx = fx - (x0 as f64);
    let ty = fy - (y0 as f64);
    let v00 = values[y0 * nx + x0];
    let v10 = values[y0 * nx + x1];
    let v01 = values[y1 * nx + x0];
    let v11 = values[y1 * nx + x1];
    (v00 * (1.0 - tx) + v10 * tx) * (1.0 - ty) + (v01 * (1.0 - tx) + v11 * tx) * ty
}
