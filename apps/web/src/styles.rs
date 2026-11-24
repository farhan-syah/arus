//! Self-contained responsive CSS design system for ARUS Dioxus SSR pages.

/// Inline stylesheet embedded in ARUS SSR HTML documents (zero external CDN dependency).
pub const ARUS_STYLESHEET: &str = r#"
:root {
  --bg: #0b1220;
  --surface: #111c30;
  --surface-alt: #16243d;
  --border: #243656;
  --text: #e6eefb;
  --muted: #93a7c6;
  --accent: #38bdf8;
  --accent-strong: #0ea5e9;
  --ok: #22c55e;
  --warn: #f59e0b;
  --danger: #ef4444;
  --radius: 12px;
  --mono: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace;
  --sans: system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif;
}
@media (prefers-color-scheme: light) {
  :root {
    --bg: #f4f7fb;
    --surface: #ffffff;
    --surface-alt: #eef3fa;
    --border: #d5e0ef;
    --text: #0f172a;
    --muted: #475569;
    --accent: #0284c7;
    --accent-strong: #0369a1;
  }
}
* { box-sizing: border-box; }
body {
  margin: 0;
  background: var(--bg);
  color: var(--text);
  font-family: var(--sans);
  line-height: 1.55;
}
a { color: var(--accent); text-decoration: none; }
a:hover { text-decoration: underline; }
.arus-shell {
  max-width: 1120px;
  margin: 0 auto;
  padding: 24px 20px 56px;
}
.arus-topbar {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  padding-bottom: 18px;
  margin-bottom: 24px;
  border-bottom: 1px solid var(--border);
}
.arus-brand {
  display: flex;
  align-items: center;
  gap: 10px;
  font-weight: 700;
  font-size: 1.15rem;
  letter-spacing: 0.04em;
  color: var(--text);
}
.arus-brand-mark {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 32px;
  height: 32px;
  border-radius: 8px;
  background: linear-gradient(135deg, #0284c7, #38bdf8);
  color: #ffffff;
  font-weight: 800;
  font-size: 0.95rem;
}
.arus-nav {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 10px;
  font-size: 0.9rem;
}
.arus-pill {
  display: inline-block;
  padding: 5px 11px;
  border-radius: 999px;
  border: 1px solid var(--border);
  background: var(--surface);
  color: var(--text);
  font-size: 0.85rem;
}
.arus-pill-active {
  border-color: var(--accent);
  background: rgba(56, 189, 248, 0.14);
  color: var(--accent);
  font-weight: 600;
}
.arus-hero {
  background: var(--surface);
  border: 1px solid var(--border);
  border-radius: var(--radius);
  padding: 24px;
  margin-bottom: 24px;
}
.arus-hero h1 { margin: 0 0 8px; font-size: 1.65rem; line-height: 1.25; }
.arus-subtitle { margin: 0; color: var(--muted); max-width: 72ch; }
.arus-kpi-grid {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(220px, 1fr));
  gap: 14px;
  margin-bottom: 24px;
}
.arus-kpi {
  background: var(--surface);
  border: 1px solid var(--border);
  border-radius: var(--radius);
  padding: 16px 18px;
}
.arus-kpi-label {
  font-size: 0.78rem;
  text-transform: uppercase;
  letter-spacing: 0.06em;
  color: var(--muted);
}
.arus-kpi-value {
  margin-top: 6px;
  font-size: 1.15rem;
  font-weight: 700;
  font-family: var(--mono);
  word-break: break-word;
}
.arus-grid {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(320px, 1fr));
  gap: 16px;
}
.arus-outlook {
  background: var(--surface);
  border: 1px solid var(--border);
  border-radius: var(--radius);
  padding: 20px;
}
.arus-outlook h2 {
  margin: 0 0 10px;
  font-size: 1.15rem;
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
}
.arus-banner {
  margin: 0 0 14px;
  padding: 8px 12px;
  border-radius: 8px;
  font-size: 0.85rem;
  font-weight: 600;
  background: rgba(245, 158, 11, 0.14);
  border: 1px solid rgba(245, 158, 11, 0.35);
  color: var(--warn);
}
.arus-banner-operational {
  background: rgba(34, 197, 94, 0.14);
  border-color: rgba(34, 197, 94, 0.35);
  color: var(--ok);
}
.arus-unsupported {
  margin: 0;
  padding: 12px;
  border-radius: 8px;
  background: rgba(239, 68, 68, 0.12);
  border: 1px solid rgba(239, 68, 68, 0.35);
  color: var(--text);
  font-size: 0.92rem;
}
.arus-meter-track {
  width: 100%;
  height: 8px;
  border-radius: 999px;
  background: var(--surface-alt);
  overflow: hidden;
  margin: 10px 0 14px;
}
.arus-meter-fill {
  height: 100%;
  border-radius: 999px;
  background: linear-gradient(90deg, #0ea5e9, #38bdf8);
}
.arus-meter-exceeded {
  background: linear-gradient(90deg, #f59e0b, #ef4444);
}
.arus-episodes {
  list-style: none;
  padding: 0;
  margin: 12px 0 0;
  display: flex;
  flex-direction: column;
  gap: 8px;
}
.arus-episodes li {
  background: var(--surface-alt);
  border: 1px solid var(--border);
  border-radius: 8px;
  padding: 10px 12px;
  font-family: var(--mono);
  font-size: 0.82rem;
}
.arus-run-list {
  list-style: none;
  padding: 0;
  margin: 0;
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(260px, 1fr));
  gap: 12px;
}
.arus-run-item a {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 14px 16px;
  background: var(--surface);
  border: 1px solid var(--border);
  border-radius: var(--radius);
  color: var(--text);
  font-family: var(--mono);
  font-size: 0.92rem;
}
.arus-run-item a:hover {
  border-color: var(--accent);
  text-decoration: none;
}
.arus-table-wrap {
  overflow-x: auto;
  background: var(--surface);
  border: 1px solid var(--border);
  border-radius: var(--radius);
}
.arus-table {
  width: 100%;
  border-collapse: collapse;
  font-size: 0.88rem;
}
.arus-table th, .arus-table td {
  text-align: left;
  padding: 12px 14px;
  border-bottom: 1px solid var(--border);
  vertical-align: top;
}
.arus-table th {
  font-size: 0.78rem;
  text-transform: uppercase;
  letter-spacing: 0.05em;
  color: var(--muted);
  background: var(--surface-alt);
}
.arus-badge {
  display: inline-block;
  padding: 2px 8px;
  border-radius: 999px;
  font-size: 0.75rem;
  font-family: var(--mono);
  border: 1px solid var(--border);
  background: var(--surface-alt);
}
.arus-badge-public {
  border-color: rgba(34, 197, 94, 0.4);
  color: var(--ok);
}
.arus-footer {
  margin-top: 32px;
  padding-top: 16px;
  border-top: 1px solid var(--border);
  color: var(--muted);
  font-size: 0.82rem;
  display: flex;
  flex-wrap: wrap;
  justify-content: space-between;
  gap: 8px;
}
"#;
