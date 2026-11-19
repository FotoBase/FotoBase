# Fotobase Edit-Recipe JSON Spec — v1

**Task:** 00-011 · **Status:** approved 2026-10-20 · **Gates:** EPIC-04 op-type freeze · **Plan refs:** §5 (edit recipe, anti-lock-in), §6 P2b

An open, versioned format. Any tool can read and apply a Fotobase recipe from this spec alone. The normative JSON Schema lives at `docs/recipe-v1.schema.json`.

---

## 1. Storage & identity

- Recipes are stored as JSON in the `fb:Recipe` XMP sidecar field (§5), one recipe per photo.
- Top level:

```json
{
  "spec": "fotobase.recipe",
  "version": 1,
  "ops": [ ... ]
}
```

- `spec` is the constant discriminator for foreign readers. `version` is the *spec* version (migrations, §4).
- `ops` is an **ordered** array; application order is array order, top to bottom.

## 2. Op model

- Each op: `{ "type": "<op>", "params": { ... } }`. Unknown `type` ⇒ reader must skip the op and warn, never fail the whole recipe.
- **Ops are pure functions over a linear-light image buffer** (§5): decode → to linear light → fold ops left-to-right → encode. Preview (screen resolution) and export (full resolution) run the *same* code path.
- **Resolution independence:** all geometry is normalized. Coordinates are fractions of image width/height in `[0,1]`; angles are degrees; radii are fractions of the shorter side. A recipe applied at any resolution yields the same image.
- Params are optional with documented defaults; omitting `params` means "use defaults".
- Ops that depend on source-pixel statistics (e.g. `auto_color`) compute them from the *input to that op* at whatever resolution is being rendered — this is the only permitted resolution dependence, and v1 accepts the resulting sub-pixel differences (golden-image tests in EPIC-04 pin preview/export parity within tolerance).

## 3. v1 op types

| Type | Params (defaults) | Notes |
|---|---|---|
| `crop` | `x, y, w, h` (normalized rect) | applied before downstream ops |
| `straighten` | `angle` (deg, CW), `auto_fill: bool` | rotates; auto_fill scales to avoid empty corners |
| `auto_rotate` | `angle_deg` (deg, CW, required, −45…45) | AI-estimated leveling (TASK-07-007); same rotate + auto-crop math as `straighten` with `auto_fill: true` |
| `rotate` | `angle` (deg, CW; multiple of 90, default 0) | quarter-turn rotation (batch edit); lossless, output dimensions swap for 90/270 |
| `auto_color` | — | white-balance fix (I'm Feeling Lucky color pass) |
| `auto_contrast` | — | tone-curve stretch |
| `fill_light` | `amount` [0..1] (0.5) | shadow lift |
| `temperature` | `kelvin_shift` [-1..1] | warm/cool |
| `tint` | `green_magenta` [-1..1] **or** `color` (`#rrggbb`), `amount` [0..1] (0.5) | two forms under one type: without `color`, the Tuning-tab green/magenta white-balance shift; with `color`, the Effects-tab wash toward `color` that keeps each pixel's luminance (TASK-08-001) |
| `saturation` | `amount` [-1..1] | |
| `brightness` | `amount` [-1..1] | |
| `highlight_recovery` | `amount` [0..1] | |
| `shadow_recovery` | `amount` [0..1] | |
| `sepia` | `amount` [0..1] | |
| `warm_colors`, `cool_colors` | `amount` [0..1] | Picasa Tuning-tab equivalents |
| `red_eye` | `regions: [{x, y, r}]`, optional `auto: bool` | normalized circles; `auto: true` detects pupils (red-dominance blobs) instead of using explicit regions |
| `text` | `text, x, y, size, color, font, rotation` | v1: rasterized at export; params normalized |
| `retouch` | `strokes: [{points: [[x,y]...], radius}]` | heal brush (TASK-08-002): classic diffusion heal — pixels under each stroke become the harmonic (Laplace) interpolation of the surrounding pixels. `points` are fractions of width/height (a polyline; one point = a dab); `radius` is a fraction of the **shorter side**, so the brush is round at any aspect ratio. Strokes apply in array order, each from the pixels around its own bounding box, so strokes that do not touch are independent of one another. Write it before geometry ops (`crop`, `straighten`) when the strokes were placed on the uncropped photo |
| `grayscale` | `enabled: bool` | |
| `bw` | `amount` [0..1] (1.0) | B&W blend strength; recipe op, distinct from the view-only B&W display mode |
| `sharpen` | `amount` [0..1] | |
| `warmify` | `amount` [0..1] (0.5) | Effects tab: warm temperature shift plus a gentle saturation lift; distinct from the Tuning-tab `warm_colors` |
| `soft_focus` | `amount` [0..1] (0.5) | blend toward a Gaussian-like blur (radius = 1.2% of the shorter side) plus a faint highlight bloom |
| `vignette` | `amount` [0..1] (0.5) | radial darkening toward the corners; elliptical, central 35% untouched |
| `border` | `amount` [0..1] (0.5), `color` (`#rrggbb`, default `#ffffff`) | frame; thickness = `amount` × 10% of the shorter side (rounded to whole pixels). **Grows the canvas** by that thickness on every side |
| `drop_shadow` | `amount` [0..1] (0.5), `color` (`#rrggbb` background, default `#ffffff`) | photo on a margin canvas with a soft shadow (60% opacity) offset toward the bottom-right; margin = `amount` × 10% of the shorter side. **Grows the canvas** |
| `film_grain` | `amount` [0..1], `seed` (integer, default 0) | deterministic per-pixel noise; pure function of (x, y, seed), so the same op always yields the same pixels |
| `undo_all` | — | clears all ops (equivalent to empty `ops[]`; kept for menu semantics) |

Frame ops (`border`, `drop_shadow`) change the output dimensions, like `crop` and `rotate`; write them after every photo-content op so grain, vignette, etc. act on the photo and not on the frame. Thickness is rounded to whole pixels per resolution, so preview and export canvases may differ by a pixel per side.

Future ops (curves, HSL, perspective, lens correction — F2 Pro tab) will bump the spec version or add op types additively; readers skip unknown types (§2).

## 4. Versioning & migrations

- Spec versions are integers. Fotobase migrates old recipes forward on read; migrated recipes are written back on next save.
- If an op's *algorithm* changes (not its params), the op gains an `algo: 2` param — old recipes keep `algo: 1` semantics so exports remain reproducible. The `version` field tracks the spec shape, not op algorithms.

## 5. Conformance

- **Reader (MUST):** skip unknown ops with warning; accept missing params as defaults; never fail on a well-formed packet with unknown op types.
- **Writer (MUST):** emit only documented op types with fully-migrations-applied `version`; emit normalized geometry only.
- **Round-trip (MUST):** serialize → parse yields identical ops (order and params). This is test-enforced.
- Examples for every v1 op live in `docs/recipe-v1.examples.json` and must validate against the schema (unit-tested).
