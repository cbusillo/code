use crate::theme::{PaletteMode, current_theme, palette_mode, quantize_color_for_palette};
use ratatui::style::Color;

// Legacy color constants - now redirect to theme
pub(crate) fn light_blue() -> Color {
    current_theme().primary
}

pub(crate) fn success_green() -> Color {
    current_theme().success
}

pub(crate) fn success() -> Color {
    current_theme().success
}

pub(crate) fn warning() -> Color {
    current_theme().warning
}

pub(crate) fn error() -> Color {
    current_theme().error
}

// Convenience functions for common theme colors
pub(crate) fn primary() -> Color {
    current_theme().primary
}

#[allow(dead_code)]
pub(crate) fn secondary() -> Color {
    current_theme().secondary
}

pub(crate) fn border() -> Color {
    current_theme().border
}

/// A slightly dimmer variant of the standard border color.
/// Blends the theme border toward the background by 30% to reduce contrast
/// while preserving the original hue relationship.
pub(crate) fn border_dim() -> Color {
    match palette_mode() {
        PaletteMode::Ansi16 => {
            if is_dark_background(current_theme().background) {
                Color::Indexed(8)
            } else {
                Color::Indexed(8)
            }
        }
        PaletteMode::Ansi256 => {
            let b = current_theme().border;
            let bg = current_theme().background;
            let (br, bg_g, bb) = color_to_rgb(b);
            let (rr, rg, rb) = color_to_rgb(bg);
            let t: f32 = 0.30; // 30% toward background
            let mix =
                |a: u8, b: u8| -> u8 { ((a as f32) * (1.0 - t) + (b as f32) * t).round() as u8 };
            let r = mix(br, rr);
            let g = mix(bg_g, rg);
            let bl = mix(bb, rb);
            quantize_color_for_palette(Color::Rgb(r, g, bl))
        }
    }
}

fn is_dark_background(color: Color) -> bool {
    matches!(color, Color::Indexed(0) | Color::Black)
}

#[allow(dead_code)]
pub(crate) fn border_focused() -> Color {
    current_theme().border_focused
}

pub(crate) fn text() -> Color {
    current_theme().text
}

pub(crate) fn text_dim() -> Color {
    current_theme().text_dim
}

pub(crate) fn text_bright() -> Color {
    current_theme().text_bright
}

pub(crate) fn spinner() -> Color {
    current_theme().spinner
}

/// Midpoint color between `text` and `text_dim` for secondary list levels.
pub(crate) fn text_mid() -> Color {
    match palette_mode() {
        PaletteMode::Ansi16 => {
            if is_dark_background(current_theme().background) {
                Color::Indexed(7)
            } else {
                Color::Indexed(8)
            }
        }
        PaletteMode::Ansi256 => {
            let a = current_theme().text;
            let b = current_theme().text_dim;
            mix_toward(a, b, 0.5)
        }
    }
}

pub(crate) fn info() -> Color {
    current_theme().info
}

// Alias for text_dim
pub(crate) fn dim() -> Color {
    text_dim()
}

pub(crate) fn background() -> Color {
    current_theme().background
}

#[allow(dead_code)]
pub(crate) fn selection() -> Color {
    current_theme().selection
}

// Syntax/special helpers
pub(crate) fn function() -> Color {
    current_theme().function
}

pub(crate) fn keyword() -> Color {
    current_theme().keyword
}

// Overlay/scrim helper: a dimmed background used behind modal overlays.
// We derive it from the current theme background so it looks consistent for
// both light and dark themes.
pub(crate) fn color_to_rgb(c: Color) -> (u8, u8, u8) {
    match c {
        Color::Rgb(r, g, b) => (r, g, b),
        Color::Black => (0, 0, 0),
        Color::White => (255, 255, 255),
        Color::Gray => (192, 192, 192),
        Color::DarkGray => (128, 128, 128),
        Color::Red => (205, 49, 49),
        Color::Green => (13, 188, 121),
        Color::Yellow => (229, 229, 16),
        Color::Blue => (36, 114, 200),
        Color::Magenta => (188, 63, 188),
        Color::Cyan => (17, 168, 205),
        Color::LightRed => (255, 102, 102),
        Color::LightGreen => (102, 255, 178),
        Color::LightYellow => (255, 255, 102),
        Color::LightBlue => (102, 153, 255),
        Color::LightMagenta => (255, 102, 255),
        Color::LightCyan => (102, 255, 255),
        // Correct mapping for ANSI-256 indexes used when we quantize themes.
        // This avoids treating all Indexed colors as grayscale and fixes
        // luminance decisions (e.g., mistaking light themes for dark) on
        // terminals that don’t advertise truecolor, including some Windows setups.
        Color::Indexed(i) => ansi256_to_rgb(i),
        // When theme background is Color::Reset (to use terminal default),
        // avoid recursion by treating Reset as pure white in RGB space.
        Color::Reset => (255, 255, 255),
    }
}

// Convert an ANSI-256 color index into an approximate RGB triple using the
// standard xterm 256-color palette: 0–15 ANSI, 16–231 6×6×6 cube, 232–255 grayscale.
fn ansi256_to_rgb(i: u8) -> (u8, u8, u8) {
    // ANSI 16 base colors
    const ANSI16: [(u8, u8, u8); 16] = [
        (0, 0, 0),       // 0 black
        (205, 0, 0),     // 1 red
        (0, 205, 0),     // 2 green
        (205, 205, 0),   // 3 yellow
        (0, 0, 205),     // 4 blue
        (205, 0, 205),   // 5 magenta
        (0, 205, 205),   // 6 cyan
        (229, 229, 229), // 7 gray
        (127, 127, 127), // 8 dark gray
        (255, 102, 102), // 9 light red
        (102, 255, 178), // 10 light green
        (255, 255, 102), // 11 light yellow
        (102, 153, 255), // 12 light blue
        (255, 102, 255), // 13 light magenta
        (102, 255, 255), // 14 light cyan
        (255, 255, 255), // 15 white
    ];

    if i < 16 {
        return ANSI16[i as usize];
    }
    if (16..=231).contains(&i) {
        // 6×6×6 color cube
        let idx = i - 16;
        let r = idx / 36;
        let g = (idx % 36) / 6;
        let b = idx % 6;
        let step = [0, 95, 135, 175, 215, 255];
        return (step[r as usize], step[g as usize], step[b as usize]);
    }
    // Grayscale ramp 232–255 maps to 8,18,28,...,238
    let level = i.saturating_sub(232);
    let v = 8 + 10 * level;
    (v, v, v)
}

fn blend_rgb(a: (u8, u8, u8), b: (u8, u8, u8), t: f32) -> (u8, u8, u8) {
    let inv = 1.0 - t;
    let r = (a.0 as f32 * inv + b.0 as f32 * t).round() as u8;
    let g = (a.1 as f32 * inv + b.1 as f32 * t).round() as u8;
    let bl = (a.2 as f32 * inv + b.2 as f32 * t).round() as u8;
    (r, g, bl)
}

/// Blend `from` toward `to` by fraction `t` (0.0..=1.0) in RGB space.
#[allow(dead_code)]
pub(crate) fn mix_toward(from: Color, to: Color, t: f32) -> Color {
    let a = color_to_rgb(from);
    let b = color_to_rgb(to);
    let (r, g, b) = blend_rgb(a, b, t.clamp(0.0, 1.0));
    quantize_color_for_palette(Color::Rgb(r, g, b))
}

fn is_dark_rgb(rgb: (u8, u8, u8)) -> bool {
    let l = (0.2126 * rgb.0 as f32 + 0.7152 * rgb.1 as f32 + 0.0722 * rgb.2 as f32) / 255.0;
    l < 0.55
}

/// Lightly tint the terminal background toward an accent color. Matches the
/// blending used for success backgrounds in diff rendering so shared surfaces
/// stay consistent.
pub(crate) fn tint_background_toward(accent: Color) -> Color {
    let bg = color_to_rgb(background());
    let fg = color_to_rgb(accent);
    let alpha = if is_dark_rgb(bg) { 0.20 } else { 0.10 };
    let (r, g, b) = blend_rgb(bg, fg, alpha);
    Color::Rgb(r, g, b)
}

fn assistant_tint_alpha(bg: Color, mid_turn: bool) -> f32 {
    match (is_dark_rgb(color_to_rgb(bg)), mid_turn) {
        (true, false) => 0.34,
        (true, true) => 0.06,
        (false, false) => 0.14,
        (false, true) => 0.04,
    }
}

fn assistant_final_accent(bg: Color) -> Color {
    if is_dark_rgb(color_to_rgb(bg)) {
        // Bias completed assistant cards toward a vivid DOS-like blue.
        Color::Rgb(0, 0, 139)
    } else {
        current_theme().primary
    }
}

fn assistant_dark_completed_bg() -> Color {
    quantize_color_for_palette(Color::Rgb(0, 0, 139))
}

fn assistant_dark_mid_turn_bg() -> Color {
    quantize_color_for_palette(Color::Rgb(0, 0, 72))
}

fn assistant_card_prominence(color: Color, bg: Color) -> u32 {
    let (cr, cg, cb) = color_to_rgb(color);
    let (br, bg_g, bb) = color_to_rgb(bg);
    let dr = cr as i32 - br as i32;
    let dg = cg as i32 - bg_g as i32;
    let db = cb as i32 - bb as i32;
    (dr * dr + dg * dg + db * db) as u32
}

fn assistant_card_tint(bg: Color, accent: Color, alpha: f32) -> Color {
    mix_toward(bg, accent, alpha)
}

fn assistant_bg_for(bg: Color, accent: Color) -> Color {
    if is_dark_rgb(color_to_rgb(bg)) {
        return assistant_dark_completed_bg();
    }
    assistant_card_tint(bg, accent, assistant_tint_alpha(bg, false))
}

fn assistant_mid_turn_bg_for(bg: Color, accent: Color) -> Color {
    if is_dark_rgb(color_to_rgb(bg)) {
        return assistant_dark_mid_turn_bg();
    }

    let completed = assistant_bg_for(bg, accent);
    let completed_prominence = assistant_card_prominence(completed, bg);
    let base_alpha = assistant_tint_alpha(bg, true);

    for alpha in [base_alpha, base_alpha * 0.75, base_alpha * 0.5, 0.0] {
        let candidate = assistant_card_tint(bg, accent, alpha);
        if assistant_card_prominence(candidate, bg) <= completed_prominence {
            return candidate;
        }
    }

    completed
}

fn blend_with_black(rgb: (u8, u8, u8), alpha: f32) -> (u8, u8, u8) {
    // target = bg*(1-alpha) + black*alpha => bg*(1-alpha)
    let inv = 1.0 - alpha;
    let r = (rgb.0 as f32 * inv).round() as u8;
    let g = (rgb.1 as f32 * inv).round() as u8;
    let b = (rgb.2 as f32 * inv).round() as u8;
    (r, g, b)
}

fn is_light(rgb: (u8, u8, u8)) -> bool {
    let l = (0.2126 * rgb.0 as f32 + 0.7152 * rgb.1 as f32 + 0.0722 * rgb.2 as f32) / 255.0;
    l >= 0.6
}

pub(crate) fn overlay_scrim() -> Color {
    let bg = current_theme().background;
    let rgb = color_to_rgb(bg);
    // For light themes, use a slightly stronger darkening; for dark themes, a gentler one.
    let alpha = if is_light(rgb) { 0.18 } else { 0.10 };
    let (r, g, b) = blend_with_black(rgb, alpha);
    quantize_color_for_palette(Color::Rgb(r, g, b))
}

/// Background for completed assistant messages.
///
/// Rich terminals keep the stronger accent tint so completed assistant cards
/// remain easy to scan without falling back to ANSI16.
pub(crate) fn assistant_bg() -> Color {
    match palette_mode() {
        PaletteMode::Ansi16 => {
            if is_dark_background(current_theme().background) {
                Color::Indexed(4)
            } else {
                Color::Indexed(7)
            }
        }
        PaletteMode::Ansi256 => {
            let bg = current_theme().background;
            assistant_bg_for(bg, assistant_final_accent(bg))
        }
    }
}

/// Background for mid-turn assistant messages.
///
/// Uses the same accent family as `assistant_bg` but a weaker mix so progress
/// inserts feel secondary even after ANSI256 quantization.
pub(crate) fn assistant_mid_turn_bg() -> Color {
    match palette_mode() {
        PaletteMode::Ansi16 => assistant_bg(),
        PaletteMode::Ansi256 => {
            let bg = current_theme().background;
            assistant_mid_turn_bg_for(bg, assistant_final_accent(bg))
        }
    }
}

/// Background for multiline code blocks rendered in assistant markdown.
///
/// New behavior: match the assistant message background so code cards feel
/// integrated with the transcript instead of appearing as stark white/black
/// panels. Borders and inner padding also use this same background.
pub(crate) fn code_block_bg() -> Color {
    assistant_bg()
}

/// Color for horizontal rules inside assistant messages.
/// Defined as halfway from the theme background toward the assistant background tint.
/// This makes the rule more pronounced than the cell background while staying subtle.
pub(crate) fn assistant_hr() -> Color {
    match palette_mode() {
        PaletteMode::Ansi16 => {
            if is_dark_background(current_theme().background) {
                Color::Indexed(8)
            } else {
                Color::Indexed(7)
            }
        }
        PaletteMode::Ansi256 => {
            let bg = current_theme().background;
            let cell = assistant_bg();
            let candidate = mix_toward(bg, assistant_final_accent(bg), 0.15);
            let cand_p = assistant_card_prominence(candidate, bg);
            let cell_p = assistant_card_prominence(cell, bg);
            let result = if cand_p < cell_p {
                candidate
            } else {
                let (r, g, b) = blend_with_black(color_to_rgb(cell), 0.12);
                Color::Rgb(r, g, b)
            };
            quantize_color_for_palette(result)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        assistant_bg_for, assistant_card_prominence, assistant_dark_completed_bg,
        assistant_dark_mid_turn_bg, assistant_final_accent, assistant_mid_turn_bg_for,
        assistant_tint_alpha,
    };
    use ratatui::style::Color;
    use std::sync::Mutex;

    static TEST_LOCK: Mutex<()> = Mutex::new(());

    struct ForcedAnsi256Guard {
        previous: Option<String>,
    }

    impl Drop for ForcedAnsi256Guard {
        fn drop(&mut self) {
            if let Some(value) = self.previous.take() {
                // Safe here because the guard is only used in a locked test.
                unsafe { std::env::set_var("CODE_FORCE_ANSI256", value) };
            } else {
                // Safe here because the guard is only used in a locked test.
                unsafe { std::env::remove_var("CODE_FORCE_ANSI256") };
            }
        }
    }

    fn force_ansi256() -> ForcedAnsi256Guard {
        let previous = std::env::var("CODE_FORCE_ANSI256").ok();
        // Safe here because the guard is only used in a locked test.
        unsafe { std::env::set_var("CODE_FORCE_ANSI256", "1") };
        ForcedAnsi256Guard { previous }
    }

    #[test]
    fn assistant_tint_is_stronger_on_dark_backgrounds() {
        assert_eq!(assistant_tint_alpha(Color::Rgb(7, 11, 20), false), 0.34);
        assert_eq!(assistant_tint_alpha(Color::Rgb(7, 11, 20), true), 0.06);
    }

    #[test]
    fn assistant_tint_stays_more_reserved_on_light_backgrounds() {
        assert_eq!(assistant_tint_alpha(Color::Rgb(245, 247, 250), false), 0.14);
        assert_eq!(assistant_tint_alpha(Color::Rgb(245, 247, 250), true), 0.04);
    }

    #[test]
    fn assistant_final_accent_uses_vivid_blue_on_dark_backgrounds() {
        assert_eq!(
            assistant_final_accent(Color::Rgb(7, 11, 20)),
            Color::Rgb(0, 0, 139)
        );
    }

    #[test]
    fn assistant_dark_backgrounds_use_direct_dos_blues() {
        let bg = Color::Rgb(7, 11, 20);
        let completed = assistant_bg_for(bg, Color::Rgb(0, 0, 139));
        let mid_turn = assistant_mid_turn_bg_for(bg, Color::Rgb(0, 0, 139));

        assert_eq!(completed, assistant_dark_completed_bg());
        assert_eq!(mid_turn, assistant_dark_mid_turn_bg());
        assert!(assistant_card_prominence(mid_turn, bg) < assistant_card_prominence(completed, bg));
    }

    #[test]
    fn mid_turn_card_never_outshines_completed_card_after_quantization() {
        let _guard = TEST_LOCK.lock().unwrap();
        let _env_guard = force_ansi256();

        let cases = [
            (
                Color::Rgb(11, 13, 16),
                Color::Rgb(0, 0, 139),
                "DarkCarbonNight",
            ),
            (
                Color::Rgb(12, 12, 8),
                Color::Rgb(0, 0, 139),
                "DarkAmberTerminal",
            ),
            (
                Color::Rgb(250, 250, 250),
                Color::Rgb(0, 162, 255),
                "LightPhoton",
            ),
            (
                Color::Rgb(247, 247, 245),
                Color::Rgb(0, 95, 204),
                "DarkPaperLightPro",
            ),
        ];

        for (bg, accent, name) in cases {
            let completed = assistant_bg_for(bg, accent);
            let mid_turn = assistant_mid_turn_bg_for(bg, accent);

            assert!(
                assistant_card_prominence(mid_turn, bg) <= assistant_card_prominence(completed, bg),
                "mid-turn tint should stay below completed tint for {name}: completed={completed:?} mid_turn={mid_turn:?}"
            );
        }
    }
}
