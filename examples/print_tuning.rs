//! Print the live tuning values (defaults + any WORM_TUNE_* overrides) as
//! JSON. The Darwin driver derives each night's candidates from THIS — the
//! current champion — so the sweep hill-climbs around whatever is committed
//! instead of re-testing a stale hard-coded grid forever.

fn main() {
    let t = worm::tuning::tuning();
    println!(
        "{{\"ESCAPE_MULTIPLE\":{},\"ESCAPE_MARGIN\":{},\"HUNT_SPEND\":{},\"HUNT_CURVE\":{},\"CORNER_GATE\":{},\"DIRECT_GATE\":{},\"ETA_FAST\":{},\"ETA_SLOW\":{},\"SHARE_FAST\":{},\"SHARE_SLOW\":{},\"KNN_BONUS\":{}}}",
        t.escape_multiple,
        t.escape_margin,
        t.hunt_spend,
        t.hunt_curve,
        t.corner_gate,
        t.direct_gate,
        t.eta_fast,
        t.eta_slow,
        t.share_fast,
        t.share_slow,
        t.knn_bonus,
    );
}
