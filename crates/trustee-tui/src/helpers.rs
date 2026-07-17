//! Utility functions for text wrapping, visual line computation, and color parsing.
//!
//! The `wrap_line` and `build_visible_lines` functions replace ratatui's
//! `Paragraph` + `.wrap()` + `.scroll()` pipeline with manual pre-wrapping
//! and line slicing. This eliminates the orphan character bug that occurs
//! when the diff-based renderer's scroll offset desynchronises from the
//! dynamically-changing rendered line count.

use ratatui::{
    style::{Color, Style},
    text::{Line, Span, Text},
    widgets::{Paragraph, Wrap},
};

/// Convert a char index to a byte offset in a string.
/// Panics if `char_idx` > number of chars in `s`.
pub(crate) fn char_to_byte_offset(s: &str, char_idx: usize) -> usize {
    s.char_indices()
        .nth(char_idx)
        .map(|(byte_pos, _)| byte_pos)
        .unwrap_or(s.len())
}

/// Count the number of visual (word-wrapped) lines a Text will occupy.
/// Uses ratatui's own Paragraph.line_count() which runs the exact same
/// WordWrapper algorithm that the Paragraph widget uses for rendering,
/// so the count is guaranteed to match.
///
/// This is kept for input-box scroll calculation where Paragraph+Wrap is
/// still used (input doesn't suffer from the orphan bug because it's
/// a fixed-height single-widget area that never scrolls during streaming).
pub(crate) fn estimate_visual_lines(text: &Text, viewport_width: u16) -> usize {
    let w = viewport_width.saturating_sub(2).max(1);
    if text.lines.is_empty() {
        return 1;
    }
    Paragraph::new(text.clone())
        .wrap(Wrap { trim: false })
        .line_count(w)
        .max(1)
}

/// Wrap a single line of text to fit within `width` terminal columns.
///
/// Uses `unicode-width` for accurate column counting (handles CJK, emoji, etc.).
/// Word-wraps at spaces when possible; hard-breaks words that exceed the width.
/// An empty input string returns a single empty line (to preserve blank lines).
///
/// # Arguments
/// * `text` - The text to wrap (should not contain `\n`; split on newlines first).
/// * `width` - Maximum number of terminal columns per visual line.
///
/// # Returns
/// Vector of strings, each fitting within `width` columns.
pub(crate) fn wrap_line(text: &str, width: usize) -> Vec<String> {
    if width == 0 {
        return vec![text.to_string()];
    }

    if text.is_empty() {
        return vec![String::new()];
    }

    let mut result: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut current_width: usize = 0;

    for word in text.split(' ') {
        let word_width = unicode_width::UnicodeWidthStr::width(word);

        if current.is_empty() {
            // First word on this visual line.
            if word_width <= width {
                current.push_str(word);
                current_width = word_width;
            } else {
                // Single word wider than the viewport — hard-break it.
                let mut remaining = word;
                while !remaining.is_empty() {
                    let mut chunk_width = 0;
                    let break_at = remaining
                        .char_indices()
                        .find(|(_, c)| {
                            let cw = unicode_width::UnicodeWidthChar::width(*c).unwrap_or(0);
                            if chunk_width + cw > width {
                                return true;
                            }
                            chunk_width += cw;
                            false
                        })
                        .map(|(idx, _)| idx)
                        .unwrap_or(remaining.len());
                    if break_at == 0 {
                        // Even one char doesn't fit — force at least one char
                        let take = remaining.chars().next().unwrap();
                        let take_len = take.len_utf8();
                        let cw = unicode_width::UnicodeWidthChar::width(take).unwrap_or(0);
                        result.push(remaining[..take_len].to_string());
                        remaining = &remaining[take_len..];
                        let _ = cw; // suppress unused warning
                    } else {
                        result.push(remaining[..break_at].to_string());
                        remaining = &remaining[break_at..];
                    }
                }
                // After hard-breaking, current stays empty so next word starts fresh
                current_width = 0;
            }
        } else {
            // Check if `current + " " + word` fits.
            let needed = current_width + 1 + word_width; // +1 for space
            if needed <= width {
                current.push(' ');
                current.push_str(word);
                current_width = needed;
            } else {
                // Doesn't fit — flush current line, start new one.
                result.push(std::mem::take(&mut current));
                // Now handle the word as the first word on the new line.
                if word_width <= width {
                    current.push_str(word);
                    current_width = word_width;
                } else {
                    // Word wider than viewport — hard-break.
                    let mut remaining = word;
                    while !remaining.is_empty() {
                        let mut chunk_width = 0;
                        let break_at = remaining
                            .char_indices()
                            .find(|(_, c)| {
                                let cw = unicode_width::UnicodeWidthChar::width(*c).unwrap_or(0);
                                if chunk_width + cw > width {
                                    return true;
                                }
                                chunk_width += cw;
                                false
                            })
                            .map(|(idx, _)| idx)
                            .unwrap_or(remaining.len());
                        if break_at == 0 {
                            let take = remaining.chars().next().unwrap();
                            let take_len = take.len_utf8();
                            result.push(remaining[..take_len].to_string());
                            remaining = &remaining[take_len..];
                        } else {
                            result.push(remaining[..break_at].to_string());
                            remaining = &remaining[break_at..];
                        }
                    }
                    current_width = 0;
                }
            }
        }
    }

    // Flush the last accumulated line (even if empty — preserves trailing blank lines).
    if !current.is_empty() || result.is_empty() {
        result.push(current);
    }

    result
}

/// Build the full list of visual lines from raw output data with manual word-wrapping.
///
/// This replaces ratatui's `Paragraph` + `.wrap(Wrap { trim: false })` pipeline.
/// Each raw line is split on `\n`, each segment is word-wrapped to `width` columns,
/// and the result is flattened into a single `Vec<Line>` of visual lines.
///
/// Lines prefixed with `\x01` are reasoning/thinking lines and styled with
/// `reasoning_style`. All other lines use `normal_style`.
///
/// # Arguments
/// * `raw_lines` - Raw output lines (may contain `\x01` prefix and embedded `\n`).
/// * `width` - Terminal column width to wrap to (content area, not including borders).
/// * `reasoning_style` - Style for reasoning lines.
/// * `normal_style` - Style for normal lines.
///
/// # Returns
/// Flattened vector of styled visual lines.
pub(crate) fn build_visual_lines(
    raw_lines: &[String],
    width: usize,
    reasoning_style: Style,
    normal_style: Style,
) -> Vec<Line<'static>> {
    let mut visual_lines: Vec<Line> = Vec::new();

    for raw in raw_lines {
        let (style, text) = if let Some(stripped) = raw.strip_prefix('\x01') {
            (reasoning_style, stripped)
        } else {
            (normal_style, raw.as_str())
        };

        // Split on embedded newlines (e.g. multi-line tool output, code blocks)
        for segment in text.split('\n') {
            let wrapped = wrap_line(segment, width);
            for w in wrapped {
                visual_lines.push(Line::from(Span::styled(w, style)));
            }
        }
    }

    visual_lines
}

/// Slice a flat list of visual lines to a visible window.
///
/// Returns only the lines from `[scroll_offset .. scroll_offset + viewport_height]`.
/// If scroll_offset exceeds the line count, returns an empty Text (which Paragraph
/// renders as blank — no orphan characters because there's no scroll mismatch).
///
/// # Arguments
/// * `visual_lines` - Full flat list of pre-wrapped visual lines.
/// * `viewport_height` - Number of visible rows in the content area.
/// * `scroll_offset` - Starting line index (0 = top).
pub(crate) fn slice_visible(
    visual_lines: &[Line<'static>],
    viewport_height: usize,
    scroll_offset: usize,
) -> Text<'static> {
    if visual_lines.is_empty() {
        return Text::default();
    }

    let start = scroll_offset.min(visual_lines.len());
    let end = (start + viewport_height).min(visual_lines.len());

    Text::from(visual_lines[start..end].to_vec())
}

/// Parse a color name string from config into a ratatui `Color`.
/// Supports all named ratatui colors. Unknown values default to `Gray`.
pub(crate) fn parse_color(name: &str) -> Color {
    match name.to_lowercase().as_str() {
        "black" => Color::Black,
        "red" => Color::Red,
        "green" => Color::Green,
        "yellow" => Color::Yellow,
        "blue" => Color::Blue,
        "magenta" => Color::Magenta,
        "cyan" => Color::Cyan,
        "gray" | "grey" => Color::Gray,
        "darkgray" | "darkgrey" => Color::DarkGray,
        "lightred" => Color::LightRed,
        "lightgreen" => Color::LightGreen,
        "lightyellow" => Color::LightYellow,
        "lightblue" => Color::LightBlue,
        "lightmagenta" => Color::LightMagenta,
        "lightcyan" => Color::LightCyan,
        "white" => Color::White,
        "reset" => Color::Reset,
        _ => Color::Gray, // safe default
    }
}
