# Data acquisition

ARUS gives operators a public source catalog, repeatable collectors, and immutable acquisition evidence.

## Catalog and recipes

| File | Role |
| --- | --- |
| `catalog/sources.json` | Providers, access, licences, documentation, cadence, size limits, and connector selection |
| `catalog/collections.json` | Default enabled feeds and optional point-based collection recipes |
| `catalog/events-november-2026.json` | Explicit historical forecast and satellite discovery candidates |

Source metadata describes access routes. A collection receipt records an actual attempt.
Documentation review does not establish data availability.

```sh
cargo run -p arus-cli -- catalog list
cargo run -p arus-cli -- catalog show jps-rainfall-stage
cargo run -p arus-cli -- catalog check
```

Catalog check and data commands check schema versions, IDs, source references, and connector parameters.
Unknown fields produce errors instead of silently changing collection behaviour.

## Supported collection

| Connector | Collected content | Limitation |
| --- | --- | --- |
| `data_gov_weather` | Official categorical forecasts or warnings | These products do not provide quantitative rainfall forcing |
| `open_meteo_forecast` | Point-based numerical precipitation forecasts | Retrieval time does not establish model initialization or publication time |
| `open_meteo_single_run` | Requested initialization-specific forecast | Historical publication time and event completeness require separate evidence |
| `open_meteo_history` | Stitched model forecast history | This product cannot establish an original full issued forecast |
| `cdse_catalogue` | Sentinel-1 scene metadata | Discovery does not download imagery or detect flooding |
| `catalog_only` | Source metadata only | Automatic retrieval remains unavailable |

Registration, public map viewing, and provider requests remain distinct access classes.
No credentialed, purchased, restricted, or undocumented numerical feed is assumed available.

```sh
cargo run -p arus-cli -- data plan
cargo run -p arus-cli -- data sync --collection selangor-general-forecast
cargo run -p arus-cli -- data status
```

Plan and status commands initialise the archive directory. They make no provider requests.
Explicit collection IDs select individual recipes. Refresh considers enabled recipes and their due times.

## Historical collection

Check event coordinates, boundaries, model, dates, and source terms before collecting historical inputs.
Example coordinates identify approximate regional reference points. They do not identify surveyed event sites or catchment boundaries.

```sh
cargo run -p arus-cli -- \
  --collections catalog/events-november-2026.json catalog check
cargo run -p arus-cli -- \
  --collections catalog/events-november-2026.json data plan
```

The example recipes remain disabled for unattended refresh.
An explicit sync selects a recipe without changing its persisted enabled state.

Open-Meteo's free service restricts commercial use. Declare noncommercial use only when that description applies.

```sh
cargo run -p arus-cli -- \
  --collections catalog/events-november-2026.json \
  data sync --collection mosque-reference-run-20261103 --accept-noncommercial
```

Read the collection file for exact IDs before running historical acquisition.
The declaration applies to the selected invocation. It does not modify the recipe file.

Each historical recipe requests one declared run or bounded date window.
The collector does not invent missing runs, generate hindcasts, or substitute future observed rain.
Keep stitched history, reanalysis, satellite estimates, measurements, and original forecasts separately labelled.

## Continuous collection

```sh
cargo run -p arus-cli -- data refresh
```

Refresh performs one pass and exits. It does not start a background process.

- Cadence determines when the next collection becomes due.
- Provider throttling applies across collections and acquisition attempts.
- Force bypasses collection cadence without bypassing provider limits or source policy.
- Conditional requests reuse validators only for the matching request and available raw evidence.
- Failures retain the last successful acquisition and record the unsuccessful attempt.
- Failed collections retry after five minutes, doubling to one hour within the normal cadence.
- Payload checks reject malformed or incompatible responses.
- Response limits bound each acquisition. Limit-reached responses remain explicitly incomplete.

Enable appropriate recipes only after checking the source terms and requested area.
Default recipes preserve government forecasts daily and warnings every fifteen minutes.
Fixed historical windows remain fixed when refreshed. Modify their parameters to request another period.

Use the supplied systemd examples for unattended refresh after installing the binary and config.
The examples expect an existing `arus` service account.

| Location | Installed content |
| --- | --- |
| `/opt/arus/bin/arus-cli` | Built CLI executable |
| `/etc/arus/sources.json` | Source catalog |
| `/etc/arus/collections.json` | Operator-controlled enabled recipes |
| `/var/lib/arus/acquisition` | Raw evidence and acquisition state |

```sh
cargo build --release -p arus-cli
sudo install -d -m 0755 /opt/arus/bin /etc/arus
sudo install -m 0755 target/release/arus-cli /opt/arus/bin/arus-cli
sudo install -m 0644 catalog/sources.json /etc/arus/sources.json
sudo install -m 0644 catalog/collections.json /etc/arus/collections.json
sudo install -m 0644 deploy/systemd/arus-data-refresh.service /etc/systemd/system/
sudo install -m 0644 deploy/systemd/arus-data-refresh.timer /etc/systemd/system/
sudo systemctl daemon-reload
sudo systemctl enable --now arus-data-refresh.timer
sudo systemctl status arus-data-refresh.timer
journalctl -u arus-data-refresh.service
```

Install the binary and catalog files at the listed paths before enabling the timer.
The timer checks due work every fifteen minutes. Source-specific cadence still controls actual requests.
No scheduler is installed or activated by the CLI.

## Evidence and recovery

| Archive content | Meaning |
| --- | --- |
| `blobs/` | SHA-256-addressed raw payloads |
| `receipts/` | Immutable acquisition attempts and their evidence references |
| Mutable state | Current validators, successful results, failures, and scheduling state |
| `archive.lock` | Exclusive ownership during collection |

Raw payloads remain separate from normalized scientific observations.
Fetch success does not establish fresh measurements, complete historical coverage, or forecast accuracy.
Provider validity, observation time, initialization, publication evidence, and retrieval time remain different concepts.

- Backups must include raw blobs, receipts, and mutable state together.
- Restore backups into an isolated directory before reusing an archive.
- Corrupt blobs and malformed state produce errors. Preserve evidence before recovery.
- Concurrent collectors cannot acquire the same archive lock.
- Failed storage writes must remain visible through command exit status.
- Monitor disk space and failed refresh runs. Raw archives have no automatic deletion policy.

Never commit raw data, credentials, local config, or research notes.
Acquisition tests use synthetic responses and temporary archives. They make no provider requests.
