//! Regression tests for the Persian/non-ASCII session-name truncation panic
//! (nghr 811ed903): `&command[..77]` sliced by BYTE index, which panics when
//! byte 77 falls mid-character (Persian/Arabic = 2 bytes, CJK/emoji = 3–4).
//!
//! The replacement `truncate_session_name` must (1) never panic on any UTF-8
//! input, and (2) keep pure-ASCII behavior byte-for-byte identical to the
//! original implementation:
//!   original: if command.len() > 80 { command[..77] + "..." } else { command }

use trustee_core::session::truncate_session_name;

/// (a) Long pure-ASCII input → byte-for-byte identical to the legacy behavior.
#[test]
fn ascii_long_truncates_identically_to_legacy() {
    let cmd = "A".repeat(100);
    assert_eq!(truncate_session_name(&cmd), format!("{}...", "A".repeat(77)));
}

/// (a-boundary) Exactly 80 chars → returned unchanged (both legacy and new).
#[test]
fn ascii_exactly_80_chars_unchanged() {
    let cmd = "A".repeat(80);
    assert_eq!(truncate_session_name(&cmd), cmd);
}

/// (b) Persian (2-byte chars) longer than 80 CHARS: byte 77 is odd and lands
/// mid-character — the exact reported crash (`end byte index 77 is not a char
/// boundary; it is inside 'ن'`). Must not panic; must cut at the last char
/// boundary at or before byte 77 and append "...".
#[test]
fn persian_long_does_not_panic_and_cuts_at_boundary() {
    let cmd = "ن".repeat(100); // 200 bytes, 100 chars — would panic under the old code
    let name = truncate_session_name(&cmd);
    assert!(name.ends_with("..."));
    // 38 Persian chars = 76 bytes (last boundary ≤ 77), then the ellipsis.
    assert_eq!(name, format!("{}...", "ن".repeat(38)));
}

/// (b-boundary) Persian command whose total size is under the threshold but
/// multi-byte must be returned unchanged (chars, not bytes, decide).
#[test]
fn persian_short_unchanged() {
    let cmd = "سلام دنیا".to_string(); // 18 bytes, 9 chars
    assert_eq!(truncate_session_name(&cmd), cmd);
}

/// (c) Command with a char boundary exactly AT byte 77 — legacy and new agree.
#[test]
fn char_boundary_exactly_at_77_same_as_legacy() {
    // 77 ASCII chars, then more: the char at byte 77 starts exactly at the cut.
    let cmd = format!("{}{}", "A".repeat(77), "BCDE");
    assert_eq!(truncate_session_name(&cmd), format!("{}...", "A".repeat(77)));
}

/// (d) 4-byte chars (CJK ideograph / emoji) straddling the byte-77 cut:
/// a char occupying bytes 76..80 must be excluded, not sliced through.
#[test]
fn four_byte_char_straddling_cut_is_excluded_not_sliced() {
    // 20 x 4-byte chars = bytes 0..80; the 20th occupies 76..80, straddling 77.
    let cmd = "\u{20BB7}".repeat(100); // '𠮷'
    let name = truncate_session_name(&cmd);
    assert_eq!(name, format!("{}...", "\u{20BB7}".repeat(19)));
}

/// (d2) Mixed scripts: the cut may fall anywhere; the result must always be
/// valid UTF-8 (boundary-safe) with the ellipsis, and its char count must be
/// ≥ what a bytewise cut would give (never longer than 77 bytes + "...").
#[test]
fn mixed_scripts_never_panic_and_stay_under_budget() {
    let cmd = format!("{}{}{}", "abc ".repeat(10), "نص فارسی ".repeat(20), "🙂".repeat(30));
    let name = truncate_session_name(&cmd);
    assert!(name.ends_with("..."));
    assert!(name.len() <= 80, "name body must stay within the byte budget, got {}", name.len());
    assert!(name.is_char_boundary(name.len() - 3)); // ellipsis split is a boundary
}

/// (f) Empty and short inputs pass through untouched.
#[test]
fn empty_and_single_char_unchanged() {
    assert_eq!(truncate_session_name(""), "");
    assert_eq!(truncate_session_name("x"), "x");
}
