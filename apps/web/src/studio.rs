//! Full-screen 3D Flood Simulation Studio (Dioxus 0.7 SSR shell + vendored MapLibre GL JS v5 + self-hosted fonts).

use dioxus::prelude::*;

use crate::view::Locale;

/// Vendored MapLibre GL JS v5 bundle served offline at `/assets/maplibre-gl.js`.
pub const MAPLIBRE_JS: &str = include_str!("../assets/maplibre-gl.js");
/// Vendored MapLibre GL JS v5 stylesheet served offline at `/assets/maplibre-gl.css`.
pub const MAPLIBRE_CSS: &str = include_str!("../assets/maplibre-gl.css");
/// ARUS Studio client controller served at `/assets/studio.js`.
pub const STUDIO_JS: &str = include_str!("../assets/studio.js");
/// ARUS Browser WebGPU / WASM CPU simulation Web Worker served at `/assets/sim-worker.js`.
pub const SIM_WORKER_JS: &str = include_str!("../assets/sim-worker.js");
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
                        placeholder: "Search place or lat, lon",
                    }
                    button { id: "btn-save-default", class: "btn btn-xs", title: "Save current view as your personal default view", "Set Default" }
                    div { class: "alerts-wrap",
                        button { id: "btn-alerts", class: "btn btn-xs alert-btn none", title: "Active flood alerts within monitored coverage",
                            span { class: "alert-tri", "▲" }
                            span { id: "alerts-badge", "0" }
                        }
                        div { id: "alerts-dropdown", class: "hud alerts-dropdown hidden",
                            div { class: "alerts-dd-head",
                                span { id: "alerts-dd-title", "Active Flood Alerts" }
                                span { id: "alerts-dd-sub", class: "alerts-dd-sub", "" }
                            }
                            div { id: "alerts-dd-list", class: "alerts-dd-list" }
                        }
                    }
                }
            }
            div { class: "top-actions",
                div { class: "seg-group",
                    button { class: "seg-btn active", "data-profile": "public", title: "Current active flood view (t = 0) and forecast", "Live" }
                    button { class: "seg-btn", "data-profile": "researcher", title: "What-if simulation studio, interventions, and GIS exports", "Simulate" }
                }
                button { id: "btn-open-layers", class: "btn", title: "Map layers, display, and units", "Layers & View" }
                button { id: "btn-open-sim", class: "btn", title: "Simulation window, initial state, stress-test, and physics", "Settings…" }
                button { id: "btn-saved-sims", class: "btn", title: "Save or open configs and completed simulations (stored locally in your browser)", "Saved (0)" }
                button { id: "btn-share", class: "btn", title: "Copy shareable link (Live view or Simulation scenario)", "Share" }
                button { id: "btn-sources", class: "btn", "Sources" }
                button { id: "btn-about", class: "btn", title: "About ARUS, disclaimer, self-hosting, and feedback", "About" }
                button { id: "btn-snap", class: "btn", title: "Save PNG snapshot", "PNG" }
                button { id: "btn-rec", class: "btn", title: "Record video locally in your browser", "Rec" }
                button { id: "btn-export-asc", class: "btn", ".ASC" }
                button { id: "btn-export-csv", class: "btn", ".CSV" }
            }
        }
        div { id: "director-caption", class: "hud director-caption hidden" }
        div { id: "sim-progress", class: "hud sim-progress hidden", role: "status", "aria-live": "polite",
            span { class: "sim-spinner" }
            span { id: "sim-progress-text", "Running simulation…" }
            button { id: "btn-sim-abort", class: "btn btn-xs", "Cancel" }
        }
        div { id: "toast-bar", class: "hud toast-bar hidden", role: "alert", "aria-live": "assertive",
            span { id: "toast-text", "" }
            button { id: "btn-toast-retry", class: "btn btn-xs hidden", "Retry" }
            button { id: "btn-toast-close", class: "btn btn-xs", "aria-label": "Dismiss", "✕" }
        }
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
                        span { class: "lm-k", "Flooded Extent" }
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
                button { class: "btn tool-btn", "data-tool": "floodpin", title: "Drop observed ground-report flood depth pin to seed top-level map", "Flood Pin" }
                button { class: "btn tool-btn", "data-tool": "bund", title: "Click points along a riverbank to draw a continuous levee bund", "Bund Line" }
                button { class: "btn tool-btn", "data-tool": "pond", "Pond -0.8m" }
            }

            div { id: "floodpin-bar", class: "intervention-bar hidden",
                span { "Pins: " strong { id: "fp-count", "0" } }
                div { class: "iv-actions",
                    select { id: "fp-depth", class: "ctl-select ctl-select-xs", title: "Observed water depth for next pin",
                        option { value: "0.2", "0.2m Ankle" }
                        option { value: "0.5", selected: true, "0.5m Knee" }
                        option { value: "1.0", "1.0m Waist" }
                        option { value: "1.5", "1.5m Chest" }
                    }
                    button { id: "btn-fp-clear", class: "btn btn-xs", "Clear" }
                }
            }

            div { id: "intervention-bar", class: "intervention-bar hidden",
                span { "Edits: " strong { id: "iv-count", "0 cells" } }
                div { class: "iv-actions",
                    select { id: "bund-height", class: "ctl-select ctl-select-xs", title: "Bund crest height above terrain",
                        option { value: "0.8", "+0.8m" }
                        option { value: "1.5", "+1.5m" }
                        option { value: "2.0", selected: true, "+2.0m" }
                        option { value: "2.5", "+2.5m" }
                    }
                    button { id: "btn-iv-breach", class: "btn btn-xs", title: "Punch a breach gap in the middle of the bund to compare Intact vs Destroyed", "Breach" }
                    button { id: "btn-iv-undo", class: "btn btn-xs", "Undo" }
                    button { id: "btn-iv-clear", class: "btn btn-xs", "Clear" }
                }
            }

            div { class: "cta-stack",
                button { id: "btn-run", class: "btn primary", "Run Simulation" }
                button { id: "btn-storm-test", class: "btn", title: "Run a 100-yr design thunderstorm (220 mm/hr) to stress-test drainage or levees", "Test 100-yr Storm" }
                button { id: "btn-compare", class: "btn compare hidden", disabled: true, "Compare vs Baseline" }
            }

            div { id: "nwp-section", class: "nwp-section",
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
                button { class: "btn", "data-layer": "arrival", "Arrival" }
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

        // Public Click-Anywhere Site Flood Verdict Card
        section { id: "site-verdict", class: "hud site-verdict hidden",
            div { class: "inspector-head",
                div { class: "sv-title-wrap",
                    span { id: "sv-title", class: "inspector-title", "Site Flood Check" }
                    span { id: "sv-badge", class: "lm-badge low", "DRY" }
                }
                button { id: "btn-sv-close", class: "btn btn-xs", "aria-label": "Close site verdict", "✕" }
            }
            div { id: "sv-summary", class: "sv-summary", "" }
            div { id: "sv-stats", class: "mini-stats" }
            div { class: "sv-actions",
                label { class: "sv-ffe",
                    span { "Floor height" }
                    select { id: "sv-ffe-select", class: "ctl-select",
                        option { value: "0", selected: true, "Ground (0 m)" }
                        option { value: "0.3", "+0.3 m slab" }
                        option { value: "0.6", "+0.6 m raised" }
                        option { value: "1.2", "+1.2 m stilts" }
                    }
                }
                button { id: "btn-sv-raise", class: "btn primary btn-xs", "Test +0.8 m Pad" }
            }
        }

        // Floating Probe / Cross-Section / Diff Inspector
        section { id: "inspector", class: "hud inspector hidden",
            div { class: "inspector-head",
                span { id: "inspector-title", class: "inspector-title", "Inspector" }
                div { class: "iv-actions",
                    button { id: "btn-inspector-pin", class: "btn btn-xs", title: "Pin probe to compare multiple points", "Pin" }
                    button { id: "btn-inspector-close", class: "btn btn-xs", "aria-label": "Close inspector", "✕" }
                }
            }
            svg { id: "inspector-svg", class: "chart-svg", view_box: "0 0 340 124" }
            div { id: "inspector-stats", class: "mini-stats" }
        }

        // Bottom-Left Status & Cursor Telemetry Pill
        div { class: "hud status-pill",
            div { id: "honesty-pill", class: "honesty-inline", title: "Engine: Server · F64",
                span { class: "dot" }
                span { id: "honesty-text", "Loading DEM…" }
            }
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
            div { class: "fb-controls-row",
                button { id: "fb-play", class: "btn btn-icon active", title: "Play / Pause", "aria-label": "Play or pause forecast", "❚❚" }
                div { class: "fb-main-col",
                    div { id: "fb-track", class: "fb-track", role: "slider", "aria-label": "Forecast timeline",
                        div { id: "fb-fill", class: "fb-fill" }
                    }
                    div { id: "fb-segments", class: "fb-segments" }
                }
            }
        }

        // Data Sources Modal
        div { id: "sources-modal", class: "modal-backdrop hidden", role: "dialog", "aria-modal": "true",
            div { class: "hud modal-card",
                div { class: "inspector-head",
                    span { class: "inspector-title", "Public Data Sources & Coverage" }
                    button { id: "btn-sources-close", class: "btn btn-xs", "Close" }
                }
                div { id: "sources-grid", class: "sources-grid" }
            }
        }

        // Popup 1: Spacious Display & Layers Modal (Zero Solver Cost, Draft + Apply)
        div { id: "layers-modal", class: "modal-backdrop hidden", role: "dialog", "aria-modal": "true",
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
                        div { class: "divider" }
                        div { class: "sec-title", "HUD & Units" }
                        div { class: "row-2",
                            div { class: "field",
                                span { class: "field-label", "Units" }
                                select { id: "opt-units", class: "ctl-select",
                                    option { value: "metric", selected: true, "Metric (m, km²)" }
                                    option { value: "imperial", "Imperial (ft, mi²)" }
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
                                option { value: "arrival", "Flood Arrival Time (ETA)" }
                                option { value: "diff", "Baseline vs Intervention (Δh)" }
                            }
                        }
                        div { class: "field",
                            div { class: "field-row",
                                span { class: "field-label", "Min Flood Cutoff" }
                                span { id: "val-cutoff", class: "field-val", "8 cm (Auto)" }
                            }
                            input { id: "opt-cutoff", r#type: "range", min: "0.05", max: "0.30", step: "0.01", value: "0.08" }
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
        div { id: "sim-modal", class: "modal-backdrop hidden", role: "dialog", "aria-modal": "true",
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
                            div { class: "sec-title", "Simulation Window & Initial State" }
                            div { class: "field",
                                span { class: "field-label", "Forecast Duration" }
                                select { id: "sim-horizon", class: "ctl-select",
                                    option { value: "6h", "6 Hours (Short-Term)" }
                                    option { value: "1d", "1 Day (24 Hours)" }
                                    option { value: "2d", selected: true, "2 Days (48 Hours)" }
                                    option { value: "3d", "3 Days (72 Hours)" }
                                    option { value: "5d", "5 Days (120 Hours)" }
                                    option { value: "15d", disabled: true, "Days 6–15 Ensemble (Server context)" }
                                }
                            }
                            div { class: "field",
                                span { class: "field-label", "Start From (Initial State)" }
                                select { id: "sim-start-mode", class: "ctl-select",
                                    option { value: "live", selected: true, "Current Live Data (14d Wet + Active Gauges)" }
                                    option { value: "dry", "Dry Ground (Start from 0)" }
                                }
                            }
                            div { class: "field",
                                span { class: "field-label", "Rain Sensitivity Multiplier" }
                                select { id: "sim-rain-mult", class: "ctl-select",
                                    option { value: "0.5", "0.5× (Drier)" }
                                    option { value: "1", selected: true, "1.0× (Forecast Baseline)" }
                                    option { value: "1.25", "1.25× (+25% Rain)" }
                                    option { value: "1.5", "1.5× (+50% Rain)" }
                                    option { value: "2", "2.0× (Double Rain)" }
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
                                div { class: "coord-well",
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
                                div { class: "row-2",
                                    span { class: "field-label", "Storm Drain Score" }
                                    span { id: "sim-storm-drain-val", class: "field-label", style: "text-align:right;font-weight:600;", "50% · Average" }
                                }
                                input { id: "sim-storm-drain", class: "ctl-range", r#type: "range", min: "0", max: "100", step: "5", value: "50" }
                                div { style: "display:flex;align-items:center;gap:6px;margin-top:4px;",
                                    label { class: "btn-ghost", r#for: "sim-storm-drain-file", style: "font-size:11px;padding:3px 7px;cursor:pointer;", "Upload Drain Grid (.json)" }
                                    input { id: "sim-storm-drain-file", r#type: "file", accept: ".json,application/json", style: "display:none;" }
                                    button { id: "sim-storm-drain-clear", class: "btn-ghost hidden", r#type: "button", style: "font-size:11px;padding:3px 7px;", "Clear" }
                                    span { id: "sim-storm-drain-status", class: "field-label", style: "font-size:11px;", "Baseline default" }
                                }
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
                                    option { value: "auto", selected: true, "Auto (Browser WebGPU / WASM → Server F64)" }
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
        // Recording Ready Preview & Download Modal
        div { id: "rec-preview-modal", class: "modal-backdrop hidden", role: "dialog", "aria-modal": "true",
            div { class: "hud modal-card", style: "max-width:680px;width:min(92vw,680px)",
                div { class: "inspector-head",
                    span { class: "inspector-title", "Recording Ready" }
                    button { id: "btn-rec-close", class: "btn btn-xs", "aria-label": "Close", "×" }
                }
                div { class: "modal-body", style: "display:flex;flex-direction:column;gap:10px",
                    video {
                        id: "rec-preview-video",
                        controls: true,
                        autoplay: true,
                        r#loop: true,
                        playsinline: true,
                        style: "width:100%;max-height:55vh;border-radius:8px;background:#000",
                    }
                    div { id: "rec-preview-meta", style: "font-size:12px;color:var(--ink-3)", "" }
                }
                div { class: "modal-foot",
                    span { style: "font-size:11px;color:var(--ink-3)", "Recorded locally in your browser." }
                    a { id: "btn-rec-download", class: "btn primary", style: "text-decoration:none", download: "arus-recording.mp4", "Download Video" }
                }
            }
        }
        // Saved Configs & Completed Simulations Modal (Browser IndexedDB)
        div { id: "saved-sims-modal", class: "modal-backdrop hidden", role: "dialog", "aria-modal": "true",
            div { class: "hud modal-card",
                div { class: "inspector-head",
                    span { class: "inspector-title", "Saved Configs & Simulations" }
                    button { id: "btn-saved-close", class: "btn btn-xs", "aria-label": "Close", "×" }
                }
                div { class: "modal-body", style: "display:flex;flex-direction:column;gap:12px",
                    div { style: "display:flex;gap:8px;flex-wrap:wrap;align-items:center",
                        input {
                            id: "save-sim-name",
                            class: "ctl-select",
                            r#type: "text",
                            placeholder: "Scenario name (optional)…",
                            style: "flex:1;min-width:180px",
                        }
                        button { id: "btn-save-config", class: "btn", title: "Save setup parameters, location, and edits only (no result frames)", "Save Config" }
                        button { id: "btn-save-sim", class: "btn primary", title: "Save setup plus full flood result so reopening needs zero re-solving", "Save Simulation" }
                    }
                    div { id: "saved-sims-hint", style: "font-size:11px;color:var(--ink-3)",
                        "Stored locally in your browser (IndexedDB). Reopening a saved simulation loads its flood frames immediately without re-solving."
                    }
                    div { id: "saved-sims-list", class: "saved-sims-list" }
                }
                div { class: "modal-foot",
                    span { id: "saved-sims-summary", style: "font-size:11px;color:var(--ink-3)", "0 saved items" }
                }
            }
        }
        // About, Curated-Data Disclaimer, Self-Hosting & Support Modal
        div { id: "about-modal", class: "modal-backdrop hidden", role: "dialog", "aria-modal": "true",
            div { class: "hud modal-card",
                div { class: "inspector-head",
                    span { class: "inspector-title", "About ARUS & Data Disclaimer" }
                    button { id: "btn-about-close", class: "btn btn-xs", "aria-label": "Close", "×" }
                }
                div { class: "modal-body", style: "display:flex;flex-direction:column;gap:12px;font-size:13px;line-height:1.5;color:var(--ink-2)",
                    div { style: "padding:10px 12px;border-radius:8px;background:var(--hud-well);border:1px solid var(--hud-border)",
                        strong { style: "color:var(--ink-1);display:block;margin-bottom:4px", "Curated Open-Data Screening Only" }
                        "This hosted instance serves regional weather, tide, and terrain datasets curated by the ARUS team. Open 30 m terrain and global NWP forecasts have resolution limits and never replace official flood warnings from your national meteorological or hydrology agency."
                    }
                    div {
                        strong { style: "color:var(--ink-1);display:block;margin-bottom:4px", "Own & Manage Your Own Data" }
                        "ARUS is open-source Rust software. Agencies, engineers, and researchers can build and self-host ARUS locally with private LiDAR DEMs, surveyed drainage networks, and live telemetry gauges."
                    }
                    div { style: "display:flex;flex-wrap:wrap;gap:8px;padding-top:4px",
                        a {
                            class: "btn",
                            href: "https://github.com/farhan-syah/arus",
                            target: "_blank",
                            rel: "noopener noreferrer",
                            "GitHub Repo ↗"
                        }
                        a {
                            class: "btn",
                            href: "https://github.com/farhan-syah/arus/issues/new",
                            target: "_blank",
                            rel: "noopener noreferrer",
                            "Report Issue / Suggest Feature ↗"
                        }
                        a {
                            class: "btn",
                            style: "text-decoration:none",
                            href: "https://ko-fi.com/farhansyah",
                            target: "_blank",
                            rel: "noopener noreferrer",
                            "Support on Ko-fi ↗"
                        }
                    }
                }
                div { class: "modal-foot",
                    span { style: "font-size:11px;color:var(--ink-3)", "Anonymous JS error telemetry helps strengthen ARUS (no PII or cookies)." }
                    button { id: "btn-about-ok", class: "btn primary", "Got it" }
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
         <link rel=\"icon\" href=\"data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 32 32'%3E%3Crect width='32' height='32' rx='8' fill='%230284c7'/%3E%3Ctext x='16' y='22' text-anchor='middle' fill='white' font-family='sans-serif' font-size='18' font-weight='bold'%3EA%3C/text%3E%3C/svg%3E\"/>\
         <link rel=\"stylesheet\" href=\"assets/maplibre-gl.css\"/>\
         <link rel=\"stylesheet\" href=\"assets/studio.css\"/>\
         <script src=\"assets/maplibre-gl.js\"></script>\
         </head><body>{body}<script src=\"assets/studio.js\"></script></body></html>"
    )
}
