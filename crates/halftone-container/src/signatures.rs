//! Shared, extendable lists of writer signatures found in image metadata.
//!
//! These are *substring* matches (case-insensitive) against text metadata — PNG text
//! chunks, JPEG comments, EXIF `Software`/`ImageDescription`, and XMP. A match is a
//! near-provenance signal (the writer announced itself), not a forensic inference, so
//! it is reported as its own labelled evidence and never merged with the model layers.

/// A named signature and the class of tool it identifies.
pub struct Signature {
    /// Lower-case substring to look for.
    pub needle: &'static str,
    /// Human-readable tool name.
    pub tool: &'static str,
}

/// Known image-generation tools that stamp identifiable strings into metadata.
pub const GENERATORS: &[Signature] = &[
    Signature {
        needle: "stable diffusion",
        tool: "Stable Diffusion",
    },
    Signature {
        needle: "stable-diffusion",
        tool: "Stable Diffusion",
    },
    Signature {
        needle: "automatic1111",
        tool: "AUTOMATIC1111 (SD WebUI)",
    },
    Signature {
        needle: "comfyui",
        tool: "ComfyUI",
    },
    Signature {
        needle: "invokeai",
        tool: "InvokeAI",
    },
    Signature {
        needle: "novelai",
        tool: "NovelAI",
    },
    Signature {
        needle: "midjourney",
        tool: "Midjourney",
    },
    Signature {
        needle: "dall-e",
        tool: "DALL·E",
    },
    Signature {
        needle: "dall·e",
        tool: "DALL·E",
    },
    Signature {
        needle: "dalle",
        tool: "DALL·E",
    },
    Signature {
        needle: "openai",
        tool: "OpenAI",
    },
    Signature {
        needle: "adobe firefly",
        tool: "Adobe Firefly",
    },
    Signature {
        needle: "firefly",
        tool: "Adobe Firefly",
    },
    Signature {
        needle: "imagen",
        tool: "Google Imagen",
    },
    Signature {
        needle: "gemini",
        tool: "Google Gemini",
    },
    Signature {
        needle: "flux",
        tool: "FLUX",
    },
    Signature {
        needle: "sdxl",
        tool: "SDXL",
    },
    Signature {
        needle: "leonardo.ai",
        tool: "Leonardo.Ai",
    },
    Signature {
        needle: "ideogram",
        tool: "Ideogram",
    },
    Signature {
        needle: "playground",
        tool: "Playground AI",
    },
    Signature {
        needle: "getimg",
        tool: "getimg.ai",
    },
    Signature {
        needle: "krea",
        tool: "Krea",
    },
    Signature {
        needle: "recraft",
        tool: "Recraft",
    },
    Signature {
        needle: "kandinsky",
        tool: "Kandinsky",
    },
];

/// Known non-camera *editors*. Naming one indicates editing, not generation.
pub const EDITORS: &[Signature] = &[
    Signature {
        needle: "adobe photoshop",
        tool: "Adobe Photoshop",
    },
    Signature {
        needle: "lightroom",
        tool: "Adobe Lightroom",
    },
    Signature {
        needle: "gimp",
        tool: "GIMP",
    },
    Signature {
        needle: "affinity photo",
        tool: "Affinity Photo",
    },
    Signature {
        needle: "pixelmator",
        tool: "Pixelmator",
    },
    Signature {
        needle: "paint.net",
        tool: "Paint.NET",
    },
    Signature {
        needle: "snapseed",
        tool: "Snapseed",
    },
];

/// Dedicated-camera and phone makes that essentially always write a MakerNote in
/// camera-native files; a missing MakerNote alongside such a make is suggestive.
pub const CAMERA_MAKES: &[&str] = &[
    "canon",
    "nikon",
    "sony",
    "fujifilm",
    "panasonic",
    "olympus",
    "om digital",
    "pentax",
    "ricoh",
    "leica",
    "hasselblad",
    "sigma",
    "apple",
    "samsung",
    "google",
    "xiaomi",
    "huawei",
    "oneplus",
    "motorola",
    "dji",
    "gopro",
];

/// First generator whose signature appears in `haystack` (matched case-insensitively).
pub fn find_generator(haystack: &str) -> Option<&'static str> {
    let hay = haystack.to_lowercase();
    GENERATORS
        .iter()
        .find(|s| hay.contains(s.needle))
        .map(|s| s.tool)
}

/// First editor whose signature appears in `haystack`.
pub fn find_editor(haystack: &str) -> Option<&'static str> {
    let hay = haystack.to_lowercase();
    EDITORS
        .iter()
        .find(|s| hay.contains(s.needle))
        .map(|s| s.tool)
}

/// Whether `make` names a make that is expected to carry a MakerNote.
pub fn is_camera_make(make: &str) -> bool {
    let m = make.to_lowercase();
    CAMERA_MAKES.iter().any(|c| m.contains(c))
}
