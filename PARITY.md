# Developing-feature parity

1:1 with the original RapidRAW develop surface (non-AI) and the Lightroom Darkroom it mirrors. Gate: every row below renders identical output to the reference engine through the new `app` UI. ✅ = working in the new UI; ➖ = engine support exists, UI missing; ❌ = not yet in engine.

## Tone
| Feature | Engine | UI | Notes |
|---|---|---|---|
| Exposure | ✅ | ✅ | -5…+5, baseline exposure for RAW |
| Brightness | ✅ | ❌ | |
| Contrast | ✅ | ✅ | |
| Highlights / Shadows / Whites / Blacks | ✅ | ✅ | |
| Tone curve (luma + RGB channels, spline points) | ✅ | ❌ | |
| Tone mapper (Filmic / AGX; per-format default) | ✅ | ❌ | hidden in old app (advanced) |

## White balance
| Feature | Engine | UI | Notes |
|---|---|---|---|
| Temperature / tint (Kelvin-driven) | ✅ (`white_balance`) | ❌ | presets 2000–20000K |
| As-shot reference (RAW) | ✅ (`as_shot`) | ✅ | implicit at open |
| Custom picker (sample white point) | ✅ (`sample_white_balance`) | ❌ | |

## Colour
| Feature | Engine | UI | Notes |
|---|---|---|---|
| Saturation / Vibrance | ✅ | ✅ | |
| Hue rotation | ✅ | ❌ | |
| Colour mixer (HSL, 8 bands) | ✅ | ❌ | |
| Colour grading (shadows/mid/hi/global hue+sat+lum, blending, balance) | ✅ | ❌ | |
| Colour calibration (primary hue/sat, shadow tint) | ✅ | ❌ | |
| Monochrome / B&W mode? | — | — | old app had none; skip |

## Detail / presence
| Feature | Engine | UI | Notes |
|---|---|---|---|
| Sharpening (amount + threshold) | ✅ | ➖ | amount only |
| Luma / colour noise reduction | ✅ | ❌ | |
| BM3D denoise (non-AI) | ✅ (`bm3d`) | ❌ | Details adjustment path |
| Clarity / Dehaze / Structure | ✅ | ➖ | clarity+dehaze only |
| Glow / Halation / Flare | ✅ | ❌ | engine only |
| Fog removal (luma map) | ✅ (`fog`) | ❌ | |
| Highlight reconstruction | ✅ | ❌ | |

## Effects
| Feature | Engine | UI | Notes |
|---|---|---|---|
| Vignette (amount/midpoint/roundness/feather) | ✅ | ❌ | |
| Grain (amount/size/roughness) | ✅ | ❌ | |
| Chromatic aberration (red-cyan / blue-yellow) | ✅ | ❌ | |
| Colour grading + calibration (above) | ✅ | ❌ | |

## Geometry
| Feature | Engine | UI | Notes |
|---|---|---|---|
| Crop (interactive aspect handling) | ✅ | ❌ | |
| Rotate / straighten slider | ✅ | ❌ | |
| Auto straighten | ✅ (`auto_straighten`) | ❌ | |
| Flip horizontal / vertical | ✅ (`adjustment_utils`) | ❌ | |
| Guided perspective (2-line) | ✅ | ❌ | |

## Optical corrections
| Feature | Engine | UI | Notes |
|---|---|---|---|
| Lens correction auto (lensfun: dist/vig/tcc/ca) | ✅ (`lens_correction`) | ❌ | |
| Lens blur/bokeh | ✅ (`lens_blur`) | ❌ | needs a depth map (AI or imported) |

## Film emulation
| Feature | Engine | UI | Notes |
|---|---|---|---|
| Negative conversion (film scans) | ✅ (`negative_conversion`) | ❌ | |
| LUT import / apply / intensity | ✅ (`lut_processing`) | ❌ | |

## Masks (non-AI)
| Feature | Engine | UI | Notes |
|---|---|---|---|
| Gradient + radial masks | ✅ (`mask_generation`) | ❌ | |
| Brush / parametric (luma/colour range) | ✅ | ❌ | |
| Feather / grow / tolerance | ✅ | ❌ | |
| Mask overlay preview | ✅ (`generate_mask_overlay`) | ❌ | |
| Per-mask adjustment state | ✅ | ❌ | |
| Liquify / heal-clone / retouch | ✅ (`inpainting`) | ❌ | manual tools (non-AI) |

## Auto (non-AI)
| Feature | Engine | UI | Notes |
|---|---|---|---|
| Auto adjustments | ✅ | ❌ | |
| Auto WB (sample) | ✅ | ❌ | |
| Auto lens correction | ✅ | ❌ | |
| Auto straighten | ✅ | ❌ | |

## Darkroom UX parity
| Feature | Engine | UI | Notes |
|---|---|---|---|
| Fit view + zoom/pan (incl. hi-fi zoom) | ➖ (engine display transform) | ❌ | fit only now |
| Before/after (side + split) | ➖ | ❌ | |
| Histogram (RGB/luma interactive) | ✅ (`AnalyticsFrame`/`calculate_histogram`) | ❌ | |
| Waveform | ✅ | ❌ | |
| Clipping warnings toggle | ✅ (`showClipping`) | ❌ | |
| Original preview (hold W) | ➖ | ❌ | |
| Undo/redo | ➖ (sidecar diff) | ❌ | |
| Copy/paste settings | ✅ | ❌ | |
| Reset adjustment(s) | ✅ | ➖ | per-slider only |
| XMP sidecar persistence | ✅ | ❌ | not wired in new UI |
| Single-image export (format/quality/naming/destination/masks-as-image) | ✅ | ❌ | not wired |
| LUT panel + import | ✅ | ❌ | |
| Settings (raw defaults, GPU renderer, preview res) | ✅ (engine settings type) | ❌ | |

Deferred by earlier decision (not parity gaps): presets/library, batch tools, tethering, immich, android, panorama/HDR/collage/multi-exposure, AI masks/tags/denoise-remote.