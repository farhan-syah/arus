//! Hydrological DEM conditioning, D8 flow accumulation, river carving, and pre-state wetting.

use std::cmp::Ordering as CmpOrdering;
use std::collections::BinaryHeap;

use crate::ObservedGaugeSeed;

pub(crate) fn compute_wgs84_bounds(
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

pub(crate) fn build_synthetic_valley_cells(cols: usize, rows: usize) -> Vec<Option<f64>> {
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
    cells
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

pub(crate) fn condition_hydrological_dem(elevations: &mut [Option<f64>], cols: usize, rows: usize) {
    let n = cols * rows;
    if elevations.len() != n || cols < 3 || rows < 3 {
        return;
    }
    let mut visited = vec![false; n];
    let mut heap: BinaryHeap<MinF64> = BinaryHeap::with_capacity(n);

    for r in 0..rows {
        for c in 0..cols {
            if r == 0 || r + 1 == rows || c == 0 || c + 1 == cols {
                let idx = r * cols + c;
                if let Some(z) = elevations[idx] {
                    visited[idx] = true;
                    heap.push(MinF64 { val: z, idx });
                }
            }
        }
    }

    let eps = 0.015;
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

pub(crate) fn compute_d8_flow_accum(
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

pub(crate) fn burn_river_channels(
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
    let max_accum = accum.iter().copied().max().unwrap_or(1).max(min_accum + 1);
    for (z_opt, &a) in elevations.iter_mut().zip(accum.iter()) {
        if a >= min_accum
            && let Some(z) = z_opt.as_mut()
        {
            let frac = ((a - min_accum) as f64 / (max_accum - min_accum) as f64).clamp(0.0, 1.0);
            let carve = (0.12 + 0.68 * frac.sqrt()) * width_ratio;
            *z -= carve;
        }
    }
}

pub(crate) fn build_channel_prestate(
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
    let channel_accum_min = ((n as f64).sqrt() * 0.22).round().max(4.0) as u32;

    if antecedent_rain_mm > 0.0 {
        let scale = (antecedent_rain_mm / 180.0).clamp(0.0, 1.5);
        for (idx, z_opt) in elevations.iter().enumerate() {
            let Some(z) = *z_opt else { continue };
            let a = accum[idx];
            if a >= channel_accum_min {
                let d = (0.12 + 0.14 * ((a as f64) / (channel_accum_min as f64)).ln_1p()) * scale;
                depths[idx] = depths[idx].max(d.min(0.65));
            } else if z <= low_z_cutoff && a >= 3 {
                depths[idx] = depths[idx].max(0.04 * scale);
            }
        }
    }

    let [min_lon, min_lat, max_lon, max_lat] = bounds;
    let dlon = (max_lon - min_lon).max(1e-6);
    let dlat = (max_lat - min_lat).max(1e-6);
    for g in gauges {
        if g.depth_m <= 0.0 || !g.depth_m.is_finite() {
            continue;
        }
        let u = (g.lon - min_lon) / dlon;
        let v = (g.lat - min_lat) / dlat;
        if !(-0.1..=1.1).contains(&u) || !(-0.1..=1.1).contains(&v) {
            continue;
        }
        let gc = (u * cols as f64).clamp(0.0, (cols - 1) as f64) as isize;
        let gr = (v * rows as f64).clamp(0.0, (rows - 1) as f64) as isize;
        let rad = g.radius_cells.clamp(1, 8) as isize;
        for dr in -rad..=rad {
            for dc in -rad..=rad {
                let nr = gr + dr;
                let nc = gc + dc;
                if nr < 0 || nc < 0 || nr >= rows as isize || nc >= cols as isize {
                    continue;
                }
                let dist2 = (dr * dr + dc * dc) as f64;
                let rad2 = (rad * rad) as f64;
                if dist2 > rad2 {
                    continue;
                }
                let nidx = (nr as usize) * cols + (nc as usize);
                if elevations[nidx].is_some() {
                    let w = (1.0 - (dist2 / rad2).sqrt()).clamp(0.0, 1.0);
                    depths[nidx] = depths[nidx].max(g.depth_m * w);
                }
            }
        }
    }
    depths
}

pub(crate) fn bilinear_sample_grid(values: &[f64], nx: usize, ny: usize, u: f64, v: f64) -> f64 {
    if nx == 0 || ny == 0 || values.len() < nx * ny {
        return 0.0;
    }
    let fx = (u.clamp(0.0, 1.0) * (nx.saturating_sub(1) as f64)).clamp(0.0, (nx - 1) as f64);
    let fy = (v.clamp(0.0, 1.0) * (ny.saturating_sub(1) as f64)).clamp(0.0, (ny - 1) as f64);
    let x0 = fx.floor() as usize;
    let y0 = fy.floor() as usize;
    let x1 = (x0 + 1).min(nx - 1);
    let y1 = (y0 + 1).min(ny - 1);
    let tx = fx - x0 as f64;
    let ty = fy - y0 as f64;
    let v00 = values[y0 * nx + x0];
    let v10 = values[y0 * nx + x1];
    let v01 = values[y1 * nx + x0];
    let v11 = values[y1 * nx + x1];
    let a = v00 * (1.0 - tx) + v10 * tx;
    let b = v01 * (1.0 - tx) + v11 * tx;
    a * (1.0 - ty) + b * ty
}
