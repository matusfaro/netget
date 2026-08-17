//! Band height allocation: how the rail's vertical space is divided among
//! server/client bands.
//!
//! The requirement is "expanding height": with few instances each band gets
//! generous room; as instances accumulate they shrink but stay individually
//! scrollable, and past that they collapse to one-line summaries with the
//! selection kept visible.

/// Most rows one band ever gets (title + content), however much space is free.
pub const BAND_MAX: u16 = 14;
/// Comfortable height: enough for a few connections and requests.
pub const BAND_PREF: u16 = 9;
/// Smallest height that still shows content (1 title + 3 content rows).
pub const BAND_MIN: u16 = 4;
/// A collapsed band: title row only.
pub const BAND_COLLAPSED: u16 = 1;

/// Height assigned to one band this frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BandLayout {
    pub height: u16,
    pub collapsed: bool,
}

/// Allocate heights for `count` bands within `available` rows, keeping
/// `selected` as usable as possible.
///
/// Returns one entry per band. The sum never exceeds `available` (bands that
/// do not fit at all get height 0 and must not be rendered — the caller scrolls
/// the band list so the selection is among those that do).
pub fn allocate(count: usize, available: u16, selected: usize, maximized: bool) -> Vec<BandLayout> {
    if count == 0 || available == 0 {
        return Vec::new();
    }

    // Maximize: the selected band takes everything left after collapsing others.
    if maximized {
        let others = count as u16 - 1;
        let collapsed_rows = others.min(available.saturating_sub(BAND_MIN));
        let selected_height = available.saturating_sub(collapsed_rows).max(BAND_MIN);
        let mut out = Vec::with_capacity(count);
        let mut spent = 0u16;
        for i in 0..count {
            if i == selected {
                out.push(BandLayout {
                    height: selected_height.min(available.saturating_sub(spent)),
                    collapsed: false,
                });
                spent = spent.saturating_add(selected_height);
            } else {
                let h = if spent < available { BAND_COLLAPSED } else { 0 };
                out.push(BandLayout {
                    height: h,
                    collapsed: h > 0,
                });
                spent = spent.saturating_add(h);
            }
        }
        return out;
    }

    let n = count as u16;

    // Case 1: everyone can have a comfortable band — spread the surplus, capped.
    if n.saturating_mul(BAND_PREF) <= available {
        let each = (available / n).min(BAND_MAX);
        return vec![
            BandLayout {
                height: each,
                collapsed: false,
            };
            count
        ];
    }

    // Case 2: everyone fits at the minimum — selected gets preferred, rest share.
    if n.saturating_mul(BAND_MIN) <= available {
        let sel_height = BAND_PREF.min(available.saturating_sub((n - 1) * BAND_MIN));
        let remaining = available.saturating_sub(sel_height);
        let others = (n - 1).max(1);
        let base = remaining / others;
        let mut extra = remaining % others;
        let mut out = Vec::with_capacity(count);
        for i in 0..count {
            if i == selected {
                out.push(BandLayout {
                    height: sel_height,
                    collapsed: false,
                });
            } else {
                let mut h = base;
                if extra > 0 {
                    h += 1;
                    extra -= 1;
                }
                out.push(BandLayout {
                    height: h,
                    collapsed: false,
                });
            }
        }
        return out;
    }

    // Case 3: too many to all show content. The selected band keeps a usable
    // height; neighbours collapse to one line each, oldest dropped entirely.
    let sel_height = BAND_PREF.min(available);
    let mut remaining = available.saturating_sub(sel_height);
    let mut out = vec![
        BandLayout {
            height: 0,
            collapsed: true,
        };
        count
    ];
    out[selected.min(count - 1)] = BandLayout {
        height: sel_height,
        collapsed: false,
    };

    // Walk outward from the selection so context around it survives.
    let sel = selected.min(count - 1);
    let mut offset = 1usize;
    while remaining > 0 && offset < count {
        for idx in [sel.checked_sub(offset), sel.checked_add(offset)]
            .into_iter()
            .flatten()
        {
            if idx < count && out[idx].height == 0 && remaining > 0 {
                out[idx] = BandLayout {
                    height: BAND_COLLAPSED,
                    collapsed: true,
                };
                remaining -= BAND_COLLAPSED;
            }
        }
        offset += 1;
    }
    out
}
