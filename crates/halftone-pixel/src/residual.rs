//! Luma extraction and high-pass residual.
//!
//! The residual (pixel minus 3×3 local mean) removes image content and keeps
//! high-frequency structure: sensor noise, compression blocking, decoder seams.
//! Everything downstream works on this, never on the pixels themselves.

/// A single-channel f32 image.
#[derive(Debug, Clone)]
pub struct Plane {
    /// Width in pixels.
    pub w: usize,
    /// Height in pixels.
    pub h: usize,
    /// Row-major samples.
    pub data: Vec<f32>,
}

impl Plane {
    /// Sample with clamped coordinates.
    #[inline]
    fn get(&self, x: isize, y: isize) -> f32 {
        let x = x.clamp(0, self.w as isize - 1) as usize;
        let y = y.clamp(0, self.h as isize - 1) as usize;
        self.data[y * self.w + x]
    }
}

/// Decode bytes to a luma plane. Any format the `image` crate was built with.
pub fn luma(bytes: &[u8]) -> Result<Plane, String> {
    let img = image::load_from_memory(bytes).map_err(|e| e.to_string())?;
    let g = img.to_luma8();
    let (w, h) = (g.width() as usize, g.height() as usize);
    Ok(Plane {
        w,
        h,
        data: g.into_raw().into_iter().map(|p| p as f32).collect(),
    })
}

/// High-pass residual: `p - mean3x3(p)`, borders clamped.
pub fn highpass(p: &Plane) -> Plane {
    let mut out = vec![0f32; p.w * p.h];
    for y in 0..p.h {
        for x in 0..p.w {
            let mut s = 0f32;
            for dy in -1isize..=1 {
                for dx in -1isize..=1 {
                    s += p.get(x as isize + dx, y as isize + dy);
                }
            }
            out[y * p.w + x] = p.data[y * p.w + x] - s / 9.0;
        }
    }
    Plane {
        w: p.w,
        h: p.h,
        data: out,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn highpass_kills_constants_and_gradients() {
        let w = 32;
        let grad = Plane {
            w,
            h: w,
            data: (0..w * w).map(|i| (i % w) as f32).collect(),
        };
        let r = highpass(&grad);
        // Interior of a linear gradient has zero residual.
        let interior: f32 = (2..w - 2)
            .flat_map(|y| (2..w - 2).map(move |x| (x, y)))
            .map(|(x, y)| r.data[y * w + x].abs())
            .sum();
        assert!(interior < 1e-3, "{interior}");
    }
}
