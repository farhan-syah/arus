use arus_domain::{GridSpec, SpatialReference};
use arus_geo::{TerrainEdit, TerrainGrid};

fn reference() -> SpatialReference {
    SpatialReference {
        projected_crs: "EPSG:3375".to_owned(),
        vertical_datum: "NGVD-MY".to_owned(),
        origin_m: [0.0, 0.0],
    }
}

#[test]
fn nodata_stays_inactive_and_edits_are_immutable_and_audited() -> Result<(), String> {
    let grid = GridSpec {
        columns: 2,
        rows: 2,
        dx_m: 10.0,
        dy_m: 10.0,
    };
    let base = TerrainGrid::new(
        grid,
        reference(),
        "synthetic survey",
        vec![Some(1.0), None, Some(1.5), Some(2.0)],
    )
    .map_err(|e| e.to_string())?;

    assert_eq!(base.active_count(), 3);
    assert_eq!(base.nodata_count(), 1);
    assert_eq!(base.active_mask(), vec![true, false, true, true]);

    let edited = base
        .apply_edit(TerrainEdit::CellElevationOverride {
            cell: 2,
            new_elevation_m: 1.25,
            reason: "culvert invert correction".to_owned(),
        })
        .map_err(|e| e.to_string())?;

    assert_eq!(base.cells()[2], Some(1.5));
    assert_eq!(edited.cells()[2], Some(1.25));
    assert_eq!(edited.conditioning_history().len(), 1);
    assert_eq!(
        edited.conditioning_history()[0].previous_elevation_m,
        Some(1.5)
    );
    Ok(())
}

#[test]
fn all_nodata_and_invalid_edits_are_rejected() {
    let grid = GridSpec {
        columns: 1,
        rows: 1,
        dx_m: 10.0,
        dy_m: 10.0,
    };
    assert!(TerrainGrid::new(grid, reference(), "empty", vec![None]).is_err());

    let single = TerrainGrid::new(grid, reference(), "single", vec![Some(1.0)]);
    assert!(single.is_ok());
    if let Ok(terrain) = single {
        assert!(
            terrain
                .apply_edit(TerrainEdit::MaskExclusion {
                    cell: 0,
                    reason: "cannot empty grid".to_owned(),
                })
                .is_err()
        );
    }
}
