# Visual-comparison protocol — display transform vs OS viewers

**Task:** TASK-02-002 · **Feature:** FEAT-02-01 color management · **PLAN §6 P1.5, §9**

The golden-image tests (`crates/render/tests/golden.rs`) prove the transform
is stable across code changes. This protocol verifies the *absolute* result —
that Fotobase previews match what the OS viewer shows on the same display.
It uses fixed fixtures and expected deltas, so it is repeatable by anyone.

## Fixtures

Three synthetic JPEGs with saturated primaries (maximum transform
sensitivity), one per source profile, each with its profile embedded in
APP2. They are committed under `crates/render/tests/fixtures/golden/`:

| Fixture | Source profile |
| --- | --- |
| `src-srgb.jpg` | sRGB |
| `src-display-p3.jpg` | Display-P3 |
| `src-adobe-rgb.jpg` | Adobe RGB (1998) |

Each is a 64×8 strip of 8×8 blocks: pure R, G, B, C, M, Y, white, and a
64-level gray; the bottom half of each block is the same color at 50%.

## Steps

1. **Export preview.** In a debug build, render each fixture through the
   app's preview path (`preview://` pipeline) with the display profile
   active, and save the on-screen result. On macOS, `screencapture -l` on
   the viewer window is sufficient; do not resize the window between runs.
2. **Open the same fixture in the OS viewer** on the same display at the
   same size:
   - macOS: Preview.app (or Photos).
   - Windows: Photos / IrfanView (ICM-aware).
   - Linux: skip — sRGB fallback by design (PLAN §6 P1.5); the golden tests
     cover the transform itself.
3. **Compare.** Screenshot both and compute the per-channel RMS delta
   (0–255 scale) over the pure-primary blocks:

   ```sh
   python3 tools/visual_compare.py preview.png viewer.png
   ```

## Expected deltas

- sRGB fixture on any display: RMS ≤ 2 vs the OS viewer (identical source
  and display spaces — any larger delta is a bug).
- Display-P3 / Adobe RGB fixtures on a wide-gamut display: RMS ≤ 4 vs the
  OS viewer. Small deltas come from viewer tone/contrast defaults, not the
  transform; they should stay constant between runs on the same OS.
- The red/green/blue blocks of the P3 and Adobe RGB fixtures must look
  visibly more saturated than the sRGB fixture's when viewed on a
  wide-gamut display — that is the direction the 02-001 test asserts
  numerically.

## Failure interpretation

- Deltas grow after a code change → transform regression; run
  `cargo test -p fotobase-render --test golden` first (regenerate goldens
  only for intentional behavior changes, and review the diff).
- Deltas differ between OS viewers but not between app runs → viewer-side
  tuning (e.g. Photos' "boost"); compare against the more neutral viewer.
- Uniform shift across all blocks → display-profile acquisition problem
  (wrong profile picked up), not a transform bug.
