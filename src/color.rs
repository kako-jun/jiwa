//! 24-bit color primitives shared by [`reveal`](crate::reveal) and
//! [`pulse`](crate::pulse).

/// 24-bit RGB triple.
///
/// Not a wrapper around `crossterm::style::Color` / `ratatui::style::Color`
/// on purpose — pulling either in would force every caller to agree on
/// the same renderer. Map at the call site:
///
/// ```
/// use jiwa::Rgb;
/// let c = Rgb(160, 220, 160);
/// // ratatui::style::Color::Rgb(c.0, c.1, c.2)
/// // crossterm::style::Color::Rgb { r: c.0, g: c.1, b: c.2 }
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgb(pub u8, pub u8, pub u8);

/// Linearly interpolate two RGB triples. `t` is clamped to `[0.0, 1.0]`,
/// so callers can pass raw progress fractions without pre-clamping.
pub fn lerp_rgb(a: Rgb, b: Rgb, t: f32) -> Rgb {
    let t = t.clamp(0.0, 1.0);
    Rgb(
        lerp_u8(a.0, b.0, t),
        lerp_u8(a.1, b.1, t),
        lerp_u8(a.2, b.2, t),
    )
}

pub(crate) fn lerp_u8(a: u8, b: u8, t: f32) -> u8 {
    let af = a as f32;
    let bf = b as f32;
    (af + (bf - af) * t).round().clamp(0.0, 255.0) as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lerp_rgb_endpoints_are_exact() {
        let a = Rgb(10, 20, 30);
        let b = Rgb(200, 100, 50);
        assert_eq!(lerp_rgb(a, b, 0.0), a);
        assert_eq!(lerp_rgb(a, b, 1.0), b);
    }

    #[test]
    fn lerp_rgb_clamps_out_of_range_t() {
        let a = Rgb(0, 0, 0);
        let b = Rgb(100, 100, 100);
        assert_eq!(lerp_rgb(a, b, -1.0), a);
        assert_eq!(lerp_rgb(a, b, 2.0), b);
    }

    #[test]
    fn lerp_rgb_midpoint_is_average() {
        let a = Rgb(0, 0, 0);
        let b = Rgb(200, 100, 50);
        assert_eq!(lerp_rgb(a, b, 0.5), Rgb(100, 50, 25));
    }
}
