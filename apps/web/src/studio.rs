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
                    button { class: "seg-btn active", "data-basemap": "topo", "Topo" }
                    button { class: "seg-btn", "data-basemap": "sat", "Satellite" }
                    button { class: "seg-btn", "data-basemap": "dark", "Dark" }
                }
                button { id: "btn-2d3d", class: "btn active", "3D" }
                button { id: "btn-sources", class: "btn", "Sources" }
                button { id: "btn-snap", class: "btn", title: "Save PNG snapshot", "PNG" }
                button { id: "btn-rec", class: "btn", title: "Record WebM video", "Rec" }
                button { id: "btn-export-asc", class: "btn", ".ASC" }
                button { id: "btn-export-csv", class: "btn", ".CSV" }
            }
        }
        div { id: "director-caption", class: "hud director-caption hidden" }
        section { id: "public-outlook", class: "hud public-outlook hidden",
            div { class: "po-head",
                span { id: "po-title", class: "po-title", "Flood Outlook" }
                span { id: "po-risk", class: "po-risk moderate", "MODERATE" }
            }
            div { id: "po-summary", class: "po-summary" }
            div { id: "po-hotspots", class: "po-hotspots" }
            div { id: "po-foot", class: "po-foot", "Screening forecast · Check official local warnings" }
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

            div { class: "divider" }

            div { class: "sec-head",
                span { class: "sec-title", "Scenario" }
            }
            div { class: "field",
                div { class: "field-row",
                    span { class: "field-label", "Rainfall" }
                    span { id: "val-rain", class: "field-val", "280 mm/hr" }
                }
                input { id: "ctl-rain", r#type: "range", min: "40", max: "400", step: "10", value: "280" }
            }
            div { class: "field",
                div { class: "field-row",
                    span { class: "field-label", "Duration" }
                    span { id: "val-dur", class: "field-val", "360 s" }
                }
                input { id: "ctl-dur", r#type: "range", min: "60", max: "900", step: "60", value: "360" }
            }

            details { class: "adv-details",
                summary { class: "adv-summary", "Hydraulics & Boundary" }
                div { class: "adv-body",
                    div { class: "row-2",
                        div { class: "field",
                            span { class: "field-label", "Hyetograph" }
                            select { id: "ctl-hyeto", class: "ctl-select",
                                option { value: "peak-storm", selected: true, "Peak Storm" }
                                option { value: "uniform", "Uniform" }
                            }
                        }
                        div { class: "field",
                            span { class: "field-label", "Surface" }
                            select { id: "ctl-soil", class: "ctl-select",
                                option { value: "impervious", selected: true, "Impervious" }
                                option { value: "green-ampt", "Green-Ampt" }
                            }
                        }
                    }
                    div { class: "row-2",
                        div { class: "field",
                            span { class: "field-label", "Boundary" }
                            select { id: "ctl-boundary", class: "ctl-select",
                                option { value: "transmissive", selected: true, "Free Outfall" }
                                option { value: "closed", "Closed Wall" }
                            }
                        }
                        div { class: "field",
                            div { class: "field-row",
                                span { class: "field-label", "Manning n" }
                                span { id: "val-manning", class: "field-val", "0.035" }
                            }
                            input { id: "ctl-manning", r#type: "range", min: "0.015", max: "0.120", step: "0.005", value: "0.035" }
                        }
                    }
                    div { class: "field",
                        div { class: "field-row",
                            span { class: "field-label", "Wind Surge (hydr 0.2)" }
                            span { id: "val-wind", class: "field-val", "0 m/s" }
                        }
                        input { id: "ctl-wind", r#type: "range", min: "0", max: "30", step: "2", value: "0" }
                    }
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
            span { id: "telemetry", class: "telemetry", "Hover map to inspect" }
        }

        // Bottom-Center Compact Playback Capsule
        footer { class: "hud timeline",
            button { id: "btn-play", class: "btn btn-icon active", title: "Play / Pause", "❚❚" }
            button { id: "btn-speed", class: "btn btn-xs", "1x" }
            span { id: "time-readout", class: "time-readout", "00:00 / 06:00" }
            input { id: "scrubber", class: "scrubber", r#type: "range", min: "0", max: "47", value: "0" }
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
