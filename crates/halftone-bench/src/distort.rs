//! WAVES-style distortion suite. Each distortion is a pure function on bytes so
//! the harness can run sources on the distorted output exactly as a user would.

/// A named distortion.
#[derive(Debug, Clone, PartialEq)]
pub enum Distortion {
    /// Identity.
    None,
    /// JPEG re-encode at quality.
    Jpeg(u8),
    /// Resize by factor.
    Resize(f32),
    /// Centre crop by fraction removed.
    Crop(f32),
    /// Gaussian blur sigma.
    Blur(f32),
    /// Gaussian noise sigma (0–255 scale).
    Noise(f32),
}

impl Distortion {
    /// Stable name for tables.
    pub fn name(&self) -> String {
        match self {
            Self::None => "none".into(),
            Self::Jpeg(q) => format!("jpeg_q{q}"),
            Self::Resize(f) => format!("resize_{f}"),
            Self::Crop(f) => format!("crop_{f}"),
            Self::Blur(s) => format!("blur_{s}"),
            Self::Noise(s) => format!("noise_{s}"),
        }
    }

    /// The default suite.
    pub fn default_suite() -> Vec<Self> {
        vec![
            Self::None,
            Self::Jpeg(50),
            Self::Jpeg(75),
            Self::Resize(0.5),
            Self::Crop(0.1),
            Self::Blur(1.0),
            Self::Noise(5.0),
        ]
    }
}
