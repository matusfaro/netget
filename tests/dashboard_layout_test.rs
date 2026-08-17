//! Band height allocation for the dashboard rail — the "expanding height"
//! behaviour: generous bands when few instances exist, shrinking but still
//! usable as they accumulate, collapsing (with the selection kept visible)
//! when there are more than can possibly fit.

use netget::tui::bands::{allocate, BAND_COLLAPSED, BAND_MAX, BAND_MIN, BAND_PREF};

#[test]
fn no_bands_or_no_space_allocates_nothing() {
    assert!(allocate(0, 40, 0, false).is_empty());
    assert!(allocate(3, 0, 0, false).is_empty());
}

#[test]
fn a_single_band_gets_a_generous_height_capped_at_max() {
    let layout = allocate(1, 40, 0, false);
    assert_eq!(layout.len(), 1);
    assert_eq!(layout[0].height, BAND_MAX);
    assert!(!layout[0].collapsed);
}

#[test]
fn few_bands_share_space_evenly_and_comfortably() {
    let layout = allocate(3, 30, 1, false);
    assert_eq!(layout.len(), 3);
    for band in &layout {
        assert!(
            band.height >= BAND_PREF,
            "each band should be at least the preferred height, got {}",
            band.height
        );
        assert!(!band.collapsed);
    }
    let total: u16 = layout.iter().map(|b| b.height).sum();
    assert!(total <= 30, "allocation must fit: {total} > 30");
}

#[test]
fn many_bands_shrink_but_keep_the_selected_one_usable() {
    // 6 bands into 30 rows: not all can have BAND_PREF, but all fit at BAND_MIN.
    let selected = 4;
    let layout = allocate(6, 30, selected, false);
    assert_eq!(layout.len(), 6);
    assert_eq!(layout[selected].height, BAND_PREF);
    for (i, band) in layout.iter().enumerate() {
        if i != selected {
            assert!(
                band.height >= 1,
                "band {i} should still render, got {}",
                band.height
            );
        }
    }
    let total: u16 = layout.iter().map(|b| b.height).sum();
    assert!(total <= 30, "allocation must fit: {total} > 30");
}

#[test]
fn too_many_bands_collapse_around_a_visible_selection() {
    // 20 bands into 20 rows: impossible to give each BAND_MIN.
    let selected = 12;
    let layout = allocate(20, 20, selected, false);
    assert_eq!(layout.len(), 20);
    assert_eq!(
        layout[selected].height, BAND_PREF,
        "the selected band must stay usable"
    );
    assert!(!layout[selected].collapsed);

    let total: u16 = layout.iter().map(|b| b.height).sum();
    assert!(total <= 20, "allocation must fit: {total} > 20");

    // Neighbours of the selection survive as one-line summaries before distant
    // bands do.
    assert_eq!(layout[selected - 1].height, BAND_COLLAPSED);
    assert_eq!(layout[selected + 1].height, BAND_COLLAPSED);
}

#[test]
fn maximize_gives_the_selected_band_the_rail_and_collapses_the_rest() {
    let selected = 1;
    let layout = allocate(4, 30, selected, true);
    assert_eq!(layout.len(), 4);
    assert!(
        layout[selected].height > BAND_PREF,
        "maximized band should exceed the preferred height, got {}",
        layout[selected].height
    );
    for (i, band) in layout.iter().enumerate() {
        if i != selected {
            assert_eq!(band.height, BAND_COLLAPSED);
            assert!(band.collapsed);
        }
    }
    let total: u16 = layout.iter().map(|b| b.height).sum();
    assert!(total <= 30, "allocation must fit: {total} > 30");
}

#[test]
fn allocation_never_overflows_for_a_range_of_shapes() {
    for count in 1..40usize {
        for available in [BAND_MIN, 10, 24, 40, 80] {
            for selected in [0, count / 2, count.saturating_sub(1)] {
                for maximized in [false, true] {
                    let layout = allocate(count, available, selected, maximized);
                    let total: u16 = layout.iter().map(|b| b.height).sum();
                    assert!(
                        total <= available,
                        "count={count} available={available} selected={selected} \
                         maximized={maximized}: total {total} overflows"
                    );
                }
            }
        }
    }
}
