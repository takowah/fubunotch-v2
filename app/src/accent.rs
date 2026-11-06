// The colour of a cover: the island takes it on while the song plays (its
// edge, glow, tint and wave). An average of the pixels weighed by how
// saturated each one is, so grey and black backgrounds don't wash it out,
// then lifted to a bright tone that reads on the dark island. The method is
// Mini Island for Windows' (github.com/nkzwsn/dynamic-island-windows, MIT).

/// The cover's colour, or None for a grey or dark picture (the theme's own then).
pub fn of_rgba(rgba: &[u8]) -> Option<(u8, u8, u8)> {
    let (mut r, mut g, mut b, mut weight) = (0.0f64, 0.0f64, 0.0f64, 0.0f64);
    // Every other pixel is plenty for a 112 px cover.
    for px in rgba.chunks_exact(4).step_by(2) {
        if px[3] < 160 {
            continue;
        }
        let (pr, pg, pb) = (px[0] as f64, px[1] as f64, px[2] as f64);
        let max = pr.max(pg).max(pb);
        if max < 40.0 {
            continue;
        }
        let sat = (max - pr.min(pg).min(pb)) / max;
        let w = sat * sat + 0.02;
        r += pr * w;
        g += pg * w;
        b += pb * w;
        weight += w;
    }
    if weight == 0.0 {
        return None;
    }
    let (r, g, b) = (r / weight, g / weight, b / weight);
    let max = r.max(g).max(b);
    if max <= 0.0 || (max - r.min(g).min(b)) / max < 0.25 {
        return None;
    }
    let k = 235.0 / max;
    Some(((r * k) as u8, (g * k) as u8, (b * k) as u8))
}

#[cfg(test)]
mod tests {
    use super::of_rgba;

    fn solid(r: u8, g: u8, b: u8) -> Vec<u8> {
        [r, g, b, 255].repeat(64)
    }

    #[test]
    fn a_red_cover_is_red_and_bright() {
        let (r, g, b) = of_rgba(&solid(120, 20, 20)).unwrap();
        assert_eq!(r, 235);
        assert!(g < 60 && b < 60);
    }

    #[test]
    fn grey_dark_or_empty_has_none() {
        assert_eq!(of_rgba(&solid(128, 128, 128)), None);
        assert_eq!(of_rgba(&solid(10, 5, 30)), None);
        assert_eq!(of_rgba(&[]), None);
    }

    #[test]
    fn the_saturated_part_wins_over_a_grey_background() {
        let mut px = solid(90, 90, 90);
        px.extend(solid(30, 60, 220).iter().take(4 * 16));
        let (r, _, b) = of_rgba(&px).unwrap();
        assert!(b > r * 2);
    }
}
