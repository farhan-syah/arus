//! Full-screen 3D Flood Simulation Studio (Dioxus 0.7 SSR shell + vendored MapLibre GL JS v5 + self-hosted fonts).

use dioxus::prelude::*;

use crate::view::Locale;

/// Vendored MapLibre GL JS v5 bundle served offline at `/assets/maplibre-gl.js`.
pub const MAPLIBRE_JS: &str = include_str!("../assets/maplibre-gl.js");
/// Vendored MapLibre GL JS v5 stylesheet served offline at `/assets/maplibre-gl.css`.
pub const MAPLIBRE_CSS: &str = include_str!("../assets/maplibre-gl.css");
/// ARUS Studio client controller served at `/assets/studio.js`.
pub const STUDIO_JS: &str = include_str!("../assets/studio.js");
/// ARUS Studio stylesheet served at `/assets/studio.css`.
pub const STUDIO_CSS: &str = include_str!("../assets/studio.css");

/// Self-hosted Inter (400/500/600) and JetBrains Mono (400/500) Latin woff2 fonts.
pub const FONT_INTER_400: &[u8] = include_bytes!("../assets/fonts/inter-400.woff2");
pub const FONT_INTER_500: &[u8] = include_bytes!("../assets/fonts/inter-500.woff2");
pub const FONT_INTER_600: &[u8] = include_bytes!("../assets/fonts/inter-600.woff2");
pub const FONT_JBMONO_400: &[u8] = include_bytes!("../assets/fonts/jbmono-400.woff2");
pub const FONT_JBMONO_500: &[u8] = include_bytes!("../assets/fonts/jbmono-500.woff2");

#[component]
fn StudioShell() -> Element {
    rsx! {
        div { id: "map" }

        // Top Floating Navigation Bar
        header { class: "hud topbar",
            div { class: "top-left",
                div { class: "brand",
                    span { class: "brand-mark", "A" }
                    span { class: "brand-name", "ARUS" }
                }
                div { class: "search-wrap",
                    select { id: "preset-select", class: "search-select", title: "Jump to basin" }
                    input {
                        id: "coord-search",
                        class: "search-input",
                        r#type: "text",
                        placeholder: "Search city or lat, lon…",
                    }
                }
            }
            div { class: "top-actions",
                div { class: "seg-group",
                    button { class: "seg-btn active", "data-profile": "public", "Public" }
                    button { class: "seg-btn", "data-profile": "researcher", "Researcher" }
                }
                button { id: "btn-open-layers", class: "btn", title: "Map layers, display, and units", "Layers & View" }
                button { id: "btn-open-sim", class: "btn primary", title: "Forecast, Site Stress-Test, and physics settings", "Simulate" }
                button { id: "btn-sources", class: "btn", "Sources" }
                button { id: "btn-snap", class: "btn", title: "Save PNG snapshot", "PNG" }
                button { id: "btn-rec", class: "btn", title: "Record MP4 video (via vidr)", "Rec" }
                button { id: "btn-export-asc", class: "btn", ".ASC" }
                button { id: "btn-export-csv", class: "btn", ".CSV" }
            }
        }
        div { id: "director-caption", class: "hud director-caption hidden" }
        div { class: "left-stack",
            section { id: "public-outlook", class: "hud public-outlook hidden",
                div { class: "po-head",
                    span { id: "po-title", class: "po-title", "Flood Outlook" }
                    span { id: "po-risk", class: "po-risk moderate", "MODERATE" }
                }
                div { id: "po-summary", class: "po-summary" }
                div { id: "po-hotspots", class: "po-hotspots" }
                div { id: "po-foot", class: "po-foot", "Screening forecast · Check official local warnings" }
            }

            // Live Per-Frame Area Flood Gauge (relative to calculated boundary)
            section { id: "live-metrics", class: "hud live-metrics hidden",
                div { class: "lm-head",
                    span { class: "lm-title", "Area Flood Index" }
                    div { class: "lm-head-right",
                        span { id: "lm-trend", class: "lm-trend", "" }
                        span { id: "lm-badge", class: "lm-badge low", "LOW" }
                    }
                }
                div { class: "lm-bar-wrap",
                    div { class: "lm-bar" }
                    div { id: "lm-marker", class: "lm-marker" }
                }
                div { class: "lm-bar-labels",
                    span { "Low" }
                    span { "Mod" }
                    span { "High" }
                    span { "Danger" }
                }
                div { class: "lm-grid",
                    div { class: "lm-cell",
                        span { class: "lm-k", "Avg Depth" }
                        strong { id: "lm-avg", class: "lm-v", "0.00 m" }
                    }
                    div { class: "lm-cell",
                        span { class: "lm-k", "Peak Depth" }
                        strong { id: "lm-peak", class: "lm-v", "0.00 m" }
                    }
                    div { class: "lm-cell",
                        span { class: "lm-k", "Flooded Area" }
                        strong { id: "lm-pct", class: "lm-v", "0%" }
                    }
                    div { class: "lm-cell",
                        span { class: "lm-k", "Stagnant" }
                        strong { id: "lm-stagnant", class: "lm-v", "0.0 km²" }
                    }
                }
            }
        }

        // Left Scenario & Tools Drawer
        aside { id: "drawer", class: "hud drawer",
            div { class: "sec-head",
                span { class: "sec-title", "Tools" }
                label { class: "upload-link", r#for: "file-asc", "Upload .ASC" }
                input { id: "file-asc", r#type: "file", accept: ".asc,.txt", style: "display:none;" }
            }
            div { class: "tool-grid",
                button { class: "btn tool-btn active", "data-tool": "inspect", "Inspect" }
                button { class: "btn tool-btn", "data-tool": "xsection", "Profile A–B" }
                button { class: "btn tool-btn", "data-tool": "pick", "Pick Basin" }
                button { class: "btn tool-btn", "data-tool": "bund", "Bund +0.8m" }
                button { class: "btn tool-btn", "data-tool": "pond", "Pond -0.8m" }
            }

            div { id: "intervention-bar", class: "intervention-bar hidden",
                span { "Edits: " strong { id: "iv-count", "0 cells" } }
                div { class: "iv-actions",
                    button { id: "btn-iv-undo", class: "btn btn-xs", "Undo" }
                    button { id: "btn-iv-clear", class: "btn btn-xs", "Clear" }
                }
            }

            div { class: "cta-stack",
                button { id: "btn-run", class: "btn primary", "Run Simulation" }
                button { id: "btn-compare", class: "btn compare hidden", disabled: true, "Compare vs Baseline" }
            }

            div { class: "divider" }

            div { class: "sec-head",
                span { class: "sec-title", "Multi-Model NWP" }
                button { id: "btn-nwp", class: "btn btn-xs", "Fetch Live" }
            }
            div { id: "nwp-card", class: "nwp-card hidden",
                div { class: "nwp-head",
                    span { id: "nwp-agree", class: "nwp-badge", "Agreement" }
                    button { id: "btn-nwp-apply", class: "upload-link", "Apply Peak" }
                }
                div { id: "nwp-rows", class: "nwp-rows" }
            }
        }

        // Top-Right Layer & Legend Card
        aside { class: "hud layer-panel",
            div { class: "sec-head",
                span { class: "sec-title", "Layers" }
                div { class: "layer-toggles",
                    button { id: "btn-env", class: "btn btn-xs", "Live" }
                    button { id: "btn-particles", class: "btn btn-xs active", "Flow" }
                }
            }
            div { class: "layer-tabs",
                button { class: "btn active", "data-layer": "depth", "Depth" }
                button { class: "btn", "data-layer": "velocity", "Velocity" }
                button { class: "btn", "data-layer": "hazard", "Hazard" }
                button { class: "btn", "data-layer": "diff", "Δh Diff" }
            }
            div { id: "legend-bar", class: "legend-bar" }
            div { id: "legend-ticks", class: "legend-ticks" }
            div { id: "legend-note", class: "legend-note" }
            div { class: "legend-river",
                span { class: "river-swatch" }
                span { "Normal river channel" }
            }
        }

        // Floating Probe / Cross-Section / Diff Inspector
        section { id: "inspector", class: "hud inspector hidden",
            div { class: "inspector-head",
                span { id: "inspector-title", class: "inspector-title", "Inspector" }
                button { id: "btn-inspector-close", class: "btn btn-xs", "✕" }
            }
            svg { id: "inspector-svg", class: "chart-svg", view_box: "0 0 340 124" }
            div { id: "inspector-stats", class: "mini-stats" }
        }

        // Bottom-Left Status & Cursor Telemetry Pill
        div { class: "hud status-pill",
            div { id: "honesty-pill", class: "honesty-inline",
                span { class: "dot" }
                span { id: "honesty-text", "Loading DEM…" }
            }
            span { class: "pill-sep", "·" }
            span { id: "engine-badge", class: "engine-badge", "Server · F64" }
            span { class: "pill-sep", "·" }
            span { id: "telemetry", class: "telemetry", "Hover map to inspect" }
        }

        // Bottom-Center Compact Playback Capsule
        footer { id: "timeline", class: "hud timeline",
            button { id: "btn-play", class: "btn btn-icon active", title: "Play / Pause", "❚❚" }
            button { id: "btn-speed", class: "btn btn-xs", "1x" }
            span { id: "time-readout", class: "time-readout", "00:00 / 06:00" }
            input { id: "scrubber", class: "scrubber", r#type: "range", min: "0", max: "47", value: "0" }
        }

        // Adaptive Multi-Day / Interval Forecast Timeline
        footer { id: "forecast-bar", class: "hud forecast-bar hidden",
            div { class: "fb-callout-rail",
                div { id: "fb-callout", class: "fb-callout", "Wed 25 · 08:00 MYT" }
            }
            div { class: "fb-track",
                div { id: "fb-fill", class: "fb-fill" }
            }
            div { id: "fb-segments", class: "fb-segments" }
        }

        // Data Sources Modal
        div { id: "sources-modal", class: "modal-backdrop hidden",
            div { class: "hud modal-card",
                div { class: "inspector-head",
                    span { class: "inspector-title", "Public Data Sources & Coverage" }
                    button { id: "btn-sources-close", class: "btn btn-xs", "Close" }
                }
                div { id: "sources-grid", class: "sources-grid" }
            }
        }

        // Popup 1: Spacious Display & Layers Modal (Zero Solver Cost, Draft + Apply)
        div { id: "layers-modal", class: "modal-backdrop hidden",
            div { class: "hud modal-card modal-wide",
                div { class: "inspector-head",
                    span { class: "inspector-title", "Display & Layers" }
                    button { id: "btn-layers-close", class: "btn btn-xs", "✕" }
                }
                div { class: "modal-grid-2",
                    div { class: "modal-sec",
                        div { class: "sec-title", "Map Layers" }
                        label { class: "toggle-row",
                            input { id: "opt-show-flood", r#type: "checkbox", checked: true }
                            span { "Flood overlay (depth / hazard / velocity)" }
                        }
                        label { class: "toggle-row",
                            input { id: "opt-show-rivers", r#type: "checkbox", checked: true }
                            span { "Normal river channels" }
                        }
                        label { class: "toggle-row",
                            input { id: "opt-show-boundary", r#type: "checkbox", checked: true }
                            span { "District / basin boundary & mask" }
                        }
                        label { class: "toggle-row",
                            input { id: "opt-show-markers", r#type: "checkbox", checked: true }
                            span { "Hotspot & coastal tide markers" }
                        }
                        label { class: "toggle-row",
                            input { id: "opt-show-particles", r#type: "checkbox", checked: true }
                            span { "Animated velocity streamlines" }
                        }
                        label { class: "toggle-row",
                            input { id: "opt-is-3d", r#type: "checkbox", checked: true }
                            span { "3D tilted view" }
                        }
                        label { class: "toggle-row",
                            input { id: "opt-show-labels", r#type: "checkbox", checked: true }
                            span { "Map place labels" }
                        }
                    }
                    div { class: "modal-sec",
                        div { class: "sec-title", "Appearance & Thresholds" }
                        div { class: "field",
                            span { class: "field-label", "Basemap" }
                            select { id: "opt-basemap", class: "ctl-select",
                                option { value: "topo", selected: true, "Topo (Light)" }
                                option { value: "sat", "Satellite" }
                                option { value: "dark", "Dark" }
                            }
                        }
                        div { class: "field",
                            span { class: "field-label", "Active Overlay" }
                            select { id: "opt-layer", class: "ctl-select",
                                option { value: "depth", selected: true, "Flood Depth (h)" }
                                option { value: "velocity", "Flow Velocity (|v|)" }
                                option { value: "hazard", "DEFRA FD2320 Hazard" }
                                option { value: "diff", "Baseline vs Intervention (Δh)" }
                            }
                        }
                        div { class: "field",
                            span { class: "field-label", "Sheet-Runoff Cutoff" }
                            select { id: "opt-cutoff", class: "ctl-select",
                                option { value: "0.02", "2 cm (0.02 m)" }
                                option { value: "0.06", selected: true, "6 cm (0.06 m — default)" }
                                option { value: "0.10", "10 cm (0.10 m)" }
                                option { value: "0.15", "15 cm (0.15 m)" }
                            }
                        }
                        div { class: "field",
                            div { class: "field-row",
                                span { class: "field-label", "Flood Opacity" }
                                span { id: "val-flood-opacity", class: "field-val", "95%" }
                            }
                            input { id: "opt-flood-opacity", r#type: "range", min: "40", max: "100", step: "5", value: "95" }
                        }
                        div { class: "field",
                            div { class: "field-row",
                                span { class: "field-label", "River Line Opacity" }
                                span { id: "val-river-opacity", class: "field-val", "72%" }
                            }
                            input { id: "opt-river-opacity", r#type: "range", min: "20", max: "100", step: "4", value: "72" }
                        }
                        div { class: "divider" }
                        div { class: "sec-title", "HUD & Units" }
                        div { class: "row-2",
                            div { class: "field",
                                span { class: "field-label", "Units" }
                                select { id: "opt-units", class: "ctl-select",
                                    option { value: "metric", selected: true, "Metric (m, km², mm/hr)" }
                                    option { value: "imperial", "Imperial (ft, mi², in/hr)" }
                                }
                            }
                            div { class: "field",
                                span { class: "field-label", "Clock" }
                                select { id: "opt-clock", class: "ctl-select",
                                    option { value: "12h", selected: true, "12-hour (AM/PM)" }
                                    option { value: "24h", "24-hour" }
                                }
                            }
                        }
                        label { class: "toggle-row",
                            input { id: "opt-show-metrics", r#type: "checkbox", checked: true }
                            span { "Show Area Flood Index card" }
                        }
                        label { class: "toggle-row",
                            input { id: "opt-show-legend", r#type: "checkbox", checked: true }
                            span { "Show Layers & Legend card" }
                        }
                    }
                }
                div { class: "modal-foot",
                    button { id: "btn-layers-reset", class: "btn", "Reset defaults" }
                    div { class: "modal-foot-right",
                        button { id: "btn-layers-cancel", class: "btn", "Cancel" }
                        button { id: "btn-layers-apply", class: "btn primary", "Apply" }
                    }
                }
            }
        }

        // Popup 2: Spacious Simulation Settings Modal (Live Forecast / Site Stress-Test / Physics)
        div { id: "sim-modal", class: "modal-backdrop hidden",
            div { class: "hud modal-card modal-wide",
                div { class: "inspector-head",
                    span { class: "inspector-title", "Simulation Settings" }
                    button { id: "btn-sim-close", class: "btn btn-xs", "✕" }
                }
                div { class: "modal-tabs",
                    button { class: "seg-btn active", "data-sim-tab": "forecast", "Live Forecast" }
                    button { class: "seg-btn", "data-sim-tab": "stress", "Site Stress-Test" }
                    button { class: "seg-btn", "data-sim-tab": "physics", "Physics & Advanced" }
                }
                div { id: "sim-tab-forecast", class: "sim-tab-pane",
                    div { class: "modal-grid-2",
                        div { class: "modal-sec",
                            div { class: "sec-title", "Forecast Horizon" }
                            div { class: "field",
                                span { class: "field-label", "Horizon Mode" }
                                select { id: "sim-horizon", class: "ctl-select",
                                    option { value: "5d", selected: true, "Days 1–5 (Deterministic Hourly 2D)" }
                                    option { value: "15d", "Days 6–15 (Ensemble Quantiles P10/P50/P90)" }
                                    option { value: "mjo", "Weeks 2–4 (MJO Regime Context)" }
                                }
                            }
                            div { class: "field",
                                span { class: "field-label", "Rain Sensitivity Multiplier" }
                                select { id: "sim-rain-mult", class: "ctl-select",
                                    option { value: "0.5", "0.5× (Drier)" }
                                    option { value: "1.0", selected: true, "1.0× (Forecast Baseline)" }
                                    option { value: "1.25", "1.25× (+25% Rain)" }
                                    option { value: "1.5", "1.5× (+50% Rain)" }
                                    option { value: "2.0", "2.0× (Double Rain)" }
                                }
                            }
                        }
                        div { class: "modal-sec",
                            div { class: "sec-title", "Server Context & MJO Regime" }
                            div { id: "sim-context-info", class: "context-box", "No regional server context loaded. Using custom/live studio parameters." }
                        }
                    }
                }
                div { id: "sim-tab-stress", class: "sim-tab-pane hidden",
                    div { class: "modal-grid-2",
                        div { class: "modal-sec",
                            div { class: "sec-title", "Design Storm (1–5 Days)" }
                            div { class: "field",
                                span { class: "field-label", "Storm Preset" }
                                select { id: "sim-stress-preset", class: "ctl-select",
                                    option { value: "60", "Moderate Rain (60 mm/day)" }
                                    option { value: "150", selected: true, "Heavy Monsoon (150 mm/day)" }
                                    option { value: "300", "Extreme Event (300 mm/day)" }
                                    option { value: "custom", "Custom mm/day" }
                                }
                            }
                            div { class: "row-2",
                                div { class: "field",
                                    span { class: "field-label", "Daily Rain (mm/day)" }
                                    input { id: "sim-stress-mm-day", class: "ctl-select", r#type: "number", min: "10", max: "600", step: "10", value: "150" }
                                }
                                div { class: "field",
                                    span { class: "field-label", "Duration (1–5 days)" }
                                    select { id: "sim-stress-days", class: "ctl-select",
                                        option { value: "1", "1 Day (24 h)" }
                                        option { value: "2", "2 Days (48 h)" }
                                        option { value: "3", selected: true, "3 Days (72 h)" }
                                        option { value: "5", "5 Days (120 h)" }
                                    }
                                }
                            }
                        }
                        div { class: "modal-sec",
                            div { class: "sec-title", "Site / House Pad Elevation Test" }
                            div { class: "field",
                                span { class: "field-label", "Raise Site Pad (checks neighbor Δh impact)" }
                                select { id: "sim-stress-pad", class: "ctl-select",
                                    option { value: "0", selected: true, "None (existing ground)" }
                                    option { value: "0.5", "+0.5 m raised pad" }
                                    option { value: "0.8", "+0.8 m raised pad" }
                                    option { value: "1.2", "+1.2 m raised pad" }
                                }
                            }
                            div { class: "field",
                                span { class: "field-label", "Target Site Coordinate" }
                                div { class: "field-row",
                                    span { id: "sim-stress-coord", class: "field-val", "Basin center" }
                                    button { id: "btn-stress-pick", class: "btn btn-xs", "Pick on map" }
                                }
                            }
                        }
                    }
                }
                div { id: "sim-tab-physics", class: "sim-tab-pane hidden",
                    div { class: "modal-grid-2",
                        div { class: "modal-sec",
                            div { class: "sec-title", "Composable Physics Toggles" }
                            label { class: "toggle-row",
                                input { id: "sim-start-dry", r#type: "checkbox" }
                                span { "Start from 0 (dry basin, ignore antecedent/gauges)" }
                            }
                            label { class: "toggle-row",
                                input { id: "sim-enable-evap", r#type: "checkbox", checked: true }
                                span { "Evaporation (FAO-56 / 4.5 mm/day)" }
                            }
                            label { class: "toggle-row",
                                input { id: "sim-enable-drain", r#type: "checkbox", checked: true }
                                span { "Subsurface & storm drainage (3.0 mm/hr)" }
                            }
                            div { class: "field",
                                span { class: "field-label", "Coastal Tide-Lock" }
                                select { id: "sim-tide-mode", class: "ctl-select",
                                    option { value: "live", selected: true, "Live Hourly Tide" }
                                    option { value: "spring-high", "Spring High Tide (+2.2 m)" }
                                    option { value: "off", "Off (Free Coastal Outfall)" }
                                }
                            }
                            div { class: "field",
                                span { class: "field-label", "Compute Target" }
                                select { id: "sim-compute", class: "ctl-select",
                                    option { value: "auto", selected: true, "Auto (Browser WASM -> Server F64)" }
                                    option { value: "server", "Server F64 Reference" }
                                }
                            }
                        }
                        div { class: "modal-sec",
                            div { class: "sec-title", "Researcher Hydraulics" }
                            div { class: "row-2",
                                div { class: "field",
                                    span { class: "field-label", "Rainfall (mm/hr)" }
                                    input { id: "sim-rain", class: "ctl-select", r#type: "number", min: "0", max: "1000", step: "10", value: "280" }
                                }
                                div { class: "field",
                                    span { class: "field-label", "Duration (s)" }
                                    input { id: "sim-dur", class: "ctl-select", r#type: "number", min: "10", max: "3600", step: "30", value: "360" }
                                }
                            }
                            div { class: "row-2",
                                div { class: "field",
                                    span { class: "field-label", "Hyetograph" }
                                    select { id: "sim-hyeto", class: "ctl-select",
                                        option { value: "peak-storm", selected: true, "Peak Storm" }
                                        option { value: "uniform", "Uniform" }
                                    }
                                }
                                div { class: "field",
                                    span { class: "field-label", "Surface Soil" }
                                    select { id: "sim-soil", class: "ctl-select",
                                        option { value: "impervious", selected: true, "Impervious" }
                                        option { value: "green-ampt", "Modified Green-Ampt" }
                                    }
                                }
                            }
                            div { class: "row-2",
                                div { class: "field",
                                    span { class: "field-label", "Boundary" }
                                    select { id: "sim-boundary", class: "ctl-select",
                                        option { value: "transmissive", selected: true, "Free Outfall" }
                                        option { value: "closed", "Closed Wall" }
                                    }
                                }
                                div { class: "field",
                                    span { class: "field-label", "Manning n" }
                                    input { id: "sim-manning", class: "ctl-select", r#type: "number", min: "0.015", max: "0.120", step: "0.005", value: "0.035" }
                                }
                            }
                            div { class: "field",
                                span { class: "field-label", "Coastal Wind Surge (m/s)" }
                                input { id: "sim-wind", class: "ctl-select", r#type: "number", min: "0", max: "30", step: "2", value: "0" }
                            }
                        }
                    }
                }
                div { class: "modal-foot",
                    button { id: "btn-sim-reset", class: "btn", "Reset defaults" }
                    div { class: "modal-foot-right",
                        button { id: "btn-sim-cancel", class: "btn", "Cancel" }
                        button { id: "btn-sim-apply", class: "btn primary", "Apply & Run" }
                    }
                }
            }
        }
    }
}

/// Renders the full-viewport ARUS Flood Simulation Studio HTML document via Dioxus SSR.
#[must_use]
pub fn render_studio_page_html(_locale: Locale) -> String {
    let body = dioxus_ssr::render_element(rsx! { StudioShell {} });
    format!(
        "<!doctype html><html lang=\"en\" data-theme=\"light\"><head>\
         <meta charset=\"utf-8\"/>\
         <meta name=\"viewport\" content=\"width=device-width,initial-scale=1\"/>\
         <title>ARUS — Flood Simulation Studio</title>\
         <link rel=\"stylesheet\" href=\"/assets/maplibre-gl.css\"/>\
         <link rel=\"stylesheet\" href=\"/assets/studio.css\"/>\
         <script src=\"/assets/maplibre-gl.js\"></script>\
         </head><body>{body}<script src=\"/assets/studio.js\"></script></body></html>"
    )
}
