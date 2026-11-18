// Pictures handed to Slint at exactly the size they are drawn, in physical
// pixels. Slint's software renderer scales an image by picking the nearest
// pixel (i-slint-renderer-software draw_texture_line / fetch_blend_pixel), so
// a cover or logo drawn at any other size comes out with jagged edges and
// rounded corners like stairs. Scaling here, by averaging, keeps them smooth.

/// A square RGBA picture (straight alpha, side `n`) averaged to side `out`.
/// Colour is averaged premultiplied, so transparent pixels don't darken edges.
pub fn resize_square(px: &[u8], n: u32, out: u32) -> Vec<u8> {
    if out == 0 || n == 0 || px.len() != (n * n * 4) as usize {
        return Vec::new();
    }
    if out == n {
        return px.to_vec();
    }
    let step = n as f32 / out as f32;
    let mut dst = vec![0u8; (out * out * 4) as usize];
    for oy in 0..out {
        let (y0, y1) = (oy as f32 * step, (oy + 1) as f32 * step);
        for ox in 0..out {
            let (x0, x1) = (ox as f32 * step, (ox + 1) as f32 * step);
            let (mut r, mut g, mut b, mut a, mut wsum) = (0f32, 0f32, 0f32, 0f32, 0f32);
            let mut y = y0.floor();
            while y < y1 {
                let wy = y1.min(y + 1.0) - y0.max(y);
                let mut x = x0.floor();
                while x < x1 {
                    let w = (x1.min(x + 1.0) - x0.max(x)) * wy;
                    let (sx, sy) = ((x as u32).min(n - 1), (y as u32).min(n - 1));
                    let i = ((sy * n + sx) * 4) as usize;
                    let pa = px[i + 3] as f32 / 255.0;
                    r += px[i] as f32 * pa * w;
                    g += px[i + 1] as f32 * pa * w;
                    b += px[i + 2] as f32 * pa * w;
                    a += pa * w;
                    wsum += w;
                    x += 1.0;
                }
                y += 1.0;
            }
            let o = ((oy * out + ox) * 4) as usize;
            if a > 0.0 && wsum > 0.0 {
                dst[o] = (r / a).round().min(255.0) as u8;
                dst[o + 1] = (g / a).round().min(255.0) as u8;
                dst[o + 2] = (b / a).round().min(255.0) as u8;
                dst[o + 3] = (a / wsum * 255.0).round().min(255.0) as u8;
            }
        }
    }
    dst
}

/// Logical px to the physical px Slint will draw (at least 1).
pub fn physical(logical: f32, scale: f32) -> u32 {
    ((logical * scale).round() as u32).max(1)
}

pub fn image(px: &[u8], n: u32) -> slint::Image {
    slint::Image::from_rgba8(slint::SharedPixelBuffer::<slint::Rgba8Pixel>::clone_from_slice(px, n, n))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn averaging_down_keeps_flat_colour_and_softens_an_edge() {
        // 4x4: left half opaque red, right half transparent.
        let mut px = vec![0u8; 4 * 4 * 4];
        for y in 0..4 {
            for x in 0..2 {
                let i = (y * 4 + x) * 4;
                px[i..i + 4].copy_from_slice(&[255, 0, 0, 255]);
            }
        }
        let out = resize_square(&px, 4, 2);
        assert_eq!(&out[0..4], &[255, 0, 0, 255], "flat stays flat");
        assert_eq!(out[7], 0, "transparent stays transparent");
        // 4 -> 3: the middle column straddles the edge and comes out half see-through.
        let out = resize_square(&px, 4, 3);
        let mid = out[(1 * 4) + 3];
        assert!(mid > 100 && mid < 160, "a soft edge pixel, alpha {mid}");
        assert_eq!(&out[4..7], &[255, 0, 0], "no dark fringe at the edge");
    }
}
