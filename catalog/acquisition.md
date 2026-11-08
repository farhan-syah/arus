# Source acquisition routes

This register explains acquisition and ongoing refresh for ARUS operators.

| Source IDs | How to obtain data | Ongoing collection |
| --- | --- | --- |
| `met-weather-forecast`, `met-weather-warning` | Run the government weather recipes through the CLI. These products contain categorical forecasts and warnings. | Default recipes archive daily forecasts and fifteen-minute warning checks. Provider requests share throttling. |
| `openmeteo-gfs-forecast` | Check the service terms, declare applicable noncommercial use, and select the point forecast recipe. | Enable an approved recipe to retain new snapshots. Fetch time establishes when this collector sees the response. |
| `openmeteo-gfs-single-run`, `openmeteo-gfs-history` | Select an explicit initialization or bounded historical interval. Keep original runs separate from stitched history. | Fixed historical recipes retain their requested period. New run or date parameters require another recipe or config change. |
| `sentinel1-catalogue` | Select a point and time interval to discover scene metadata through CDSE OData. | Repeat discovery to capture newly published metadata. Imagery acquisition and flood detection remain separate capabilities. |
| `nasa-imerg-early`, `nasa-imerg-late`, `nasa-imerg-final` | Register through the linked NASA archive and select the product/version, bounds, and time interval. No authenticated collector exists yet. | Early/Late support recent estimates. Final arrives later. Separate product versions and revisions when adding a connector. |
| `noaa-gfs`, `noaa-gefs` | Use the linked public bucket indexes to select model cycle, lead, variables, grid, and ensemble member. | A future binary collector must preserve complete file manifests and missing cycles. No bulk model download runs automatically. |
| `copernicus-dem`, `osm-features` | Use the official collection or export route. Check licence, tile/area, version, coordinate reference, and acquisition dates. | Refresh by product release or mapped changes. These sources cannot establish surveyed drainage dimensions or local terrain accuracy. |
| `jps-rainfall-stage`, `met-radar-history` | Check the official interface or provider request route. Public display does not establish a supported numerical archive. | Automation requires checked access, cadence, station metadata, units, and reuse rights. No scraper or assumed API runs. |
| `local-drainage-geometry`, `local-flood-observations` | Establish a permitted source with dated geometry or independent measurements. Public availability remains unconfirmed. | Updates require provenance and event applicability. Missing records remain explicit data gaps. |

Read `sources.json` for primary documentation and source-specific licence records.
Run catalog inspection before changing a collection recipe.

```sh
cargo run -p arus-cli -- catalog show nasa-imerg-early
cargo run -p arus-cli -- catalog show noaa-gefs
cargo run -p arus-cli -- catalog show sentinel1-catalogue
```

## Required acquisition evidence

- Provider and product IDs identify the upstream source.
- Request parameters identify the location, period, initialization, variables, and member where applicable.
- Checksums identify exact retained bytes.
- Receipts identify actual requests, responses, errors, and retrieval times.
- Source timestamps retain their declared reference and timezone.
- Publication evidence establishes historical availability only when supplied or directly observed.
- Coverage and missing intervals remain separate from HTTP success.
- Redistribution rights remain separate from technical access.

No catalog entry establishes that every historical file exists.
Do not substitute another product or version without recording that change.
