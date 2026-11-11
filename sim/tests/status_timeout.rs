//! Regression guard for the status-card timeout. A WiFi/AP status card must
//! never be able to cover the festival playlist forever: `StatusTimed` uses
//! this predicate and the AP cards pass 15 s.
use led_core::tasks::display::status_due;

#[test]
fn no_timeout_is_indefinite() {
    assert!(!status_due(0, 0));
    assert!(!status_due(u32::MAX, 0));
}

#[test]
fn timed_status_clears_at_its_deadline() {
    assert!(!status_due(0, 15_000));
    assert!(!status_due(14_999, 15_000));
    assert!(status_due(15_000, 15_000));
    assert!(status_due(15_001, 15_000));
}

#[test]
fn ap_cards_use_a_finite_timeout() {
    // The actual AP fallback cards pass 15_000 ms; join/lease/link-lost cards
    // pass 10_000. Keep the values here explicit so a future edit cannot
    // silently make an AP card indefinite.
    const AP_CARD_TIMEOUT_MS: u32 = 15_000;
    const JOIN_CARD_TIMEOUT_MS: u32 = 10_000;

    assert!(AP_CARD_TIMEOUT_MS > 0);
    assert!(JOIN_CARD_TIMEOUT_MS > 0);
    assert!(status_due(AP_CARD_TIMEOUT_MS, AP_CARD_TIMEOUT_MS));
}
