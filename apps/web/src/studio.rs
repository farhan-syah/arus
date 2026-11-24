//! Full-screen 3D Globe & Terrain Flood Simulation Studio (Dioxus 0.7 SSR shell + vendored MapLibre GL JS v5).

use dioxus::prelude::*;

use crate::view::Locale;

/// Vendored MapLibre GL JS v5 bundle served offline at `/assets/maplibre-gl.js`.
pub const MAPLIBRE_JS: &str = include_str!("../assets/maplibre-gl.js");
/// Vendored MapLibre GL JS v5 stylesheet served offline at `/assets/maplibre-gl.css`.
pub const MAPLIBRE_CSS: &str = include_str!("../assets/maplibre-gl.css");
/// ARUS 3D Studio client controller served at `/assets/studio.js`.
pub const STUDIO_JS: &str = include_str!("../assets/studio.js");
/// ARUS 3D Studio glass HUD stylesheet served at `/assets/studio.css`.
pub const STUDIO_CSS: &str = include_str!("../assets/studio.css");

#[component]
fn StudioShell(locale: Locale) -> Element {
    let lang_btn = match locale {
        Locale::English => "EN",
        Locale::BahasaMalaysia => "BM",
    };
    let run_label = match locale {
        Locale::English => "Run Simulation",
        Locale::BahasaMalaysia => "Jalankan Simulasi",
    };
    let cmp_label = match locale {
        Locale::English => "Compare vs Baseline",
        Locale::BahasaMalaysia => "Banding vs Asal",
    };

    rsx! {
        div { id: "map" }

        // Top Floating Control Bar
        header { class: "hud topbar",
            div { class: "brand",
                span { class: "brand-mark", "A" }
                span { "ARUS" }
            }
            div { class: "search-wrap",
                select { id: "preset-select", class: "search-select", title: "Jump to basin" }
                input {
                    id: "coord-search",
                    class: "search-input",
                    r#type: "text",
                    placeholder: "lat, lon (anywhere on Earth) + Enter",
                }
            }
            div { id: "honesty-pill", class: "honesty-pill",
                span { class: "dot" }
                "DEMO • SCREENING ONLY"
            }
            div { class: "top-actions",
                button { id: "btn-2d3d", class: "btn active", "3D" }
                button { id: "btn-mode", class: "btn", "Engineer" }
                button { id: "btn-lang", class: "btn", "{lang_btn}" }
                button { id: "btn-sources", class: "btn", "Sources" }
                button { id: "btn-export-asc", class: "btn", ".ASC" }
                button { id: "btn-export-csv", class: "btn", ".CSV" }
            }
        }

        // Left Simulation & BYOD Studio Drawer
        aside { id: "drawer", class: "hud drawer",
            div { class: "sec-title",
                span { "INTERACTION TOOLS" }
                span { style: "color:#38bdf8;", "Click map" }
            }
            div { class: "tool-grid",
                button { class: "btn tool-btn active", "data-tool": "inspect", "Inspect" }
                button { class: "btn tool-btn", "data-tool": "pick", "Pick Area" }
                button { class: "btn tool-btn", "data-tool": "xsection", "Profile A-B" }
                button { class: "btn tool-btn", "data-tool": "bund", "Bund +0.8m" }
                button { class: "btn tool-btn", "data-tool": "pond", "Pond -0.8m" }
                label { class: "btn tool-btn", r#for: "file-asc", "Upload .ASC" }
                input { id: "file-asc", r#type: "file", accept: ".asc,.txt", style: "display:none;" }
            }

            div { class: "intervention-bar",
                span { "Edits: " strong { id: "iv-count", "0 cells" } }
                div { style: "display:flex;gap:4px;",
                    button { id: "btn-iv-undo", class: "btn", style: "padding:2px 6px;font-size:0.68rem;", "Undo" }
                    button { id: "btn-iv-clear", class: "btn", style: "padding:2px 6px;font-size:0.68rem;", "Clear" }
                }
            }

            div { class: "sec-title", span { "STORM & HYDR 0.1.0 PHYSICS" } }
            div { class: "field",
                div { class: "field-row",
                    span { "Rainfall Intensity" }
                    span { id: "val-rain", class: "field-val", "280 mm/hr" }
                }
                input { id: "ctl-rain", r#type: "range", min: "20", max: "400", step: "10", value: "280" }
            }
            div { class: "row-2",
                div { class: "field",
                    div { class: "field-row", span { "Hyetograph" } }
                    select { id: "ctl-hyeto", class: "ctl-select",
                        option { value: "peak-storm", selected: true, "Peak Storm" }
                        option { value: "uniform", "Uniform" }
                    }
                }
                div { class: "field",
                    div { class: "field-row",
                        span { "Duration" }
                        span { id: "val-dur", class: "field-val", "360 s" }
                    }
                    input { id: "ctl-dur", r#type: "range", min: "60", max: "900", step: "60", value: "360" }
                }
            }
            div { class: "row-2 eng-only",
                div { class: "field",
                    div { class: "field-row", span { "Land Surface" } }
                    select { id: "ctl-soil", class: "ctl-select",
                        option { value: "impervious", selected: true, "Impervious" }
                        option { value: "green-ampt", "Green-Ampt" }
                    }
                }
                div { class: "field",
                    div { class: "field-row", span { "Boundary" } }
                    select { id: "ctl-boundary", class: "ctl-select",
                        option { value: "transmissive", selected: true, "Free Outfall" }
                        option { value: "closed", "Closed Wall" }
                    }
                }
            }
            div { class: "field eng-only",
                div { class: "field-row",
                    span { "Manning Roughness n" }
                    span { id: "val-manning", class: "field-val", "0.035" }
                }
                input { id: "ctl-manning", r#type: "range", min: "0.015", max: "0.120", step: "0.005", value: "0.035" }
            }

            div { class: "cta-stack",
                button { id: "btn-run", class: "btn primary", "{run_label}" }
                button { id: "btn-compare", class: "btn warn", disabled: true, "{cmp_label}" }
            }
        }

        // Right Layer & Legend Panel
        aside { class: "hud layer-panel",
            div { class: "sec-title",
                span { "LAYERS" }
                div { style: "display:flex;gap:4px;",
                    button { id: "btn-env", class: "btn", style: "padding:2px 6px;font-size:0.65rem;", "Live t" }
                    button { id: "btn-particles", class: "btn active", style: "padding:2px 6px;font-size:0.65rem;", "Flow" }
                }
            }
            div { class: "layer-tabs",
                button { class: "btn active", "data-layer": "depth", "Depth (h)" }
                button { class: "btn", "data-layer": "velocity", "Velocity" }
                button { class: "btn", "data-layer": "hazard", "FD2320" }
                button { class: "btn", "data-layer": "diff", "Δh Diff" }
            }
            div { id: "legend-bar", class: "legend-bar" }
            div { id: "legend-ticks", class: "legend-ticks" }
            div { id: "legend-note", class: "legend-note" }
        }

        // Floating Hydrograph / Cross-Section / Diff Inspector
        section { id: "inspector", class: "hud inspector hidden",
            div { class: "inspector-head",
                span { id: "inspector-title", "Probe Inspector" }
                button { id: "btn-inspector-close", class: "btn", style: "padding:2px 7px;", "✕" }
            }
            svg { id: "inspector-svg", class: "chart-svg", view_box: "0 0 340 128" }
            div { id: "inspector-stats", class: "mini-stats" }
        }

        // Bottom Timeline Player & Live Cursor Telemetry
        footer { class: "hud timeline",
            button { id: "btn-play", class: "btn active", "❚❚" }
            button { id: "btn-speed", class: "btn", "1x" }
            span { id: "time-readout", class: "time-readout", "t = 0s / 180s" }
            input { id: "scrubber", class: "scrubber", r#type: "range", min: "0", max: "47", value: "0" }
            span { id: "telemetry", class: "telemetry", "Hover map for elevation, depth, WSE & velocity" }
        }

        // Data Sources Transparency Modal
        div { id: "sources-modal", class: "modal-backdrop hidden",
            div { class: "hud modal-card",
                div { class: "inspector-head",
                    span { "Public Data Sources & Coverage (catalog/sources.json)" }
                    button { id: "btn-sources-close", class: "btn", "Close" }
                }
                div { id: "sources-grid", class: "sources-grid" }
            }
        }
    }
}

/// Renders the full-viewport ARUS 3D Flood Simulation Studio HTML document via Dioxus SSR.
#[must_use]
pub fn render_studio_page_html(locale: Locale) -> String {
    let body = dioxus_ssr::render_element(rsx! {
        StudioShell { locale }
    });
    format!(
        "<!doctype html><html><head><meta charset=\"utf-8\">\
         <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\
         <title>ARUS — 3D Flood Simulation Studio</title>\
         <link rel=\"stylesheet\" href=\"/assets/maplibre-gl.css\">\
         <link rel=\"stylesheet\" href=\"/assets/studio.css\">\
         </head><body>{body}\
         <script src=\"/assets/maplibre-gl.js\"></script>\
         <script src=\"/assets/studio.js\"></script>\
         </body></html>"
    )
}
