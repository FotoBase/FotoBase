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
| `auto_color` | — | white-balance fix (I'm Feeling Lucky color pass) |
| `auto_contrast` | — | tone-curve stretch |
| `fill_light` | `amount` [0..1] (0.5) | shadow lift |
| `temperature` | `kelvin_shift` [-1..1] | warm/cool |
| `tint` | `green_magenta` [-1..1] | |
| `saturation` | `amount` [-1..1] | |
| `brightness` | `amount` [-1..1] | |
| `highlight_recovery` | `amount` [0..1] | |
| `shadow_recovery` | `amount` [0..1] | |
| `sepia` | `amount` [0..1] | |
| `warm_colors`, `cool_colors` | `amount` [0..1] | Picasa Tuning-tab equivalents |
| `red_eye` | `regions: [{x, y, r}]`, optional `auto: bool` | normalized circles; `auto: true` detects pupils (red-dominance blobs) instead of using explicit regions |
| `text` | `text, x, y, size, color, font, rotation` | v1: rasterized at export; params normalized |
| `retouch` | `strokes: [{points: [[x,y]...], radius}]` | classic diffusion heal |
| `grayscale` | `enabled: bool` | |
| `bw` | `amount` [0..1] (1.0) | B&W blend strength; recipe op, distinct from the view-only B&W display mode |
| `sharpen` | `amount` [0..1] | |
| `undo_all` | — | clears all ops (equivalent to empty `ops[]`; kept for menu semantics) |

Future ops (curves, HSL, perspective, lens correction — F2 Pro tab) will bump the spec version or add op types additively; readers skip unknown types (§2).

## 4. Versioning & migrations

- Spec versions are integers. Fotobase migrates old recipes forward on read; migrated recipes are written back on next save.
- If an op's *algorithm* changes (not its params), the op gains an `algo: 2` param — old recipes keep `algo: 1` semantics so exports remain reproducible. The `version` field tracks the spec shape, not op algorithms.

## 5. Conformance

- **Reader (MUST):** skip unknown ops with warning; accept missing params as defaults; never fail on a well-formed packet with unknown op types.
- **Writer (MUST):** emit only documented op types with fully-migrations-applied `version`; emit normalized geometry only.
- **Round-trip (MUST):** serialize → parse yields identical ops (order and params). This is test-enforced.
- Examples for every v1 op live in `docs/recipe-v1.examples.json` and must validate against the schema (unit-tested).
