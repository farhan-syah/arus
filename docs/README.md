# ARUS Documentation

ARUS documentation covers public HTTP contracts, system architecture, data acquisition, and operational forecast recipes.

- [Public HTTP API (`arus-api-v1`)](api/README.md): Routes for the 3D Studio, archived runs, interactive simulations, diffs, GIS exports, and MP4 encoding.
- [Architecture & Workspace Boundaries](architecture.md): Crate responsibilities, dependency direction, `hydr 0.3.0` physics integration, and publication gates.
- [Data Acquisition Guide](data-acquisition.md): Source catalog, collection recipes, SHA-256 receipts, and unattended `systemd` refresh.
- [Forecast & Storyboard Operator Recipe](forecast-and-storyboard-recipe.md): Data sync, 4-stage pipeline, spatial NWP forcing, URL params, and `arus-worker record` MP4 export.
- [Example Storyboard Spec](storyboard-example.json): Declarative JSON storyboard for `arus-worker record`.
- `decisions/`: Published architecture decisions.
