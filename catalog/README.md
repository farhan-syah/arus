# Public data catalog

This catalog records source access and collection recipes for ARUS operators.

## Files

| File | Purpose |
| --- | --- |
| `sources.json` | Versioned source metadata, access classes, licences, limits, and connector selection |
| `collections.json` | Default collection recipes and explicit policy acceptance |
| `events-november-2026.json` | Disabled historical acquisition candidates for the November events |

`schema_version` identifies the JSON contract. The loader rejects unknown fields.
`reviewed_on` records documentation review. It does not establish successful collection or historical coverage.

The [acquisition routes](acquisition.md) explain retrieval, refresh, and remaining connector gaps for every source group.

## Source access

| Access class | Meaning |
| --- | --- |
| `public` | A public acquisition route exists. Source-specific service and reuse terms still apply. |
| `public_view_only` | Public viewing exists. Supported downloads and reuse rights remain unconfirmed. |
| `registration` | The numerical archive requires registration or authenticated access. |
| `request_only` | Acquisition requires a separate provider request or product arrangement. |
| `unknown` | A usable public acquisition route remains unestablished. |

`catalog_only` records an acquisition gap. It never authorises automatic retrieval.
Discovery of a satellite scene does not download imagery or establish flood coverage.

## Collection policy

- Default enabled recipes preserve government forecasts and warnings prospectively.
- Government weather requests share a fifteen-second provider interval.
- Open-Meteo recipes require explicit noncommercial acceptance before execution.
- Historical event recipes remain disabled until an operator checks their parameters and policy.
- Refresh cadence controls local collection. It does not establish the age of source observations.
- Result limits can truncate responses. A successful request does not establish complete coverage.
- Credentials, raw responses, receipts, and collection state remain outside Git.

## Modify recipes

- Keep collection IDs unique and stable.
- Select an existing source ID and its connector-specific parameter shape.
- Check dates, model cycles, coordinates, and source terms before enabling a recipe.
- Preserve initialization separately from publication time and retrieval time.
- Preserve nulls and missing intervals rather than treating them as zero rainfall.

The supplied coordinates are approximate regional reference points. They are not surveyed event sites or catchment boundaries.
The mosque, UiTM, and Subang recipes do not assert a shared hydraulic domain.

## Known acquisition gaps

- Exact November forecast runs and usable event-specific flood imagery remain unchecked.
- JPS numerical history, station metadata, and reuse rights require checks.
- Fine terrain, local drainage, operating logs, and independent flood observations remain unestablished public inputs.
- Numerical estimates and stitched forecast histories cannot substitute for observed rainfall or original issued forecasts.
