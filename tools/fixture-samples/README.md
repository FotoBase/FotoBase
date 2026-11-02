# fixture-samples

Tiny pre-made valid HEIC files duplicated across generated fixtures
(PLAN §11 Q8, decided 2026-10-20 — no libheif toolchain dependency in CI).

`sample-1.heic` was produced once via macOS `sips`:

    sips -s format heic -s formatOptions 10 sample-1.jpg --out sample-1.heic

and committed (a few KB). Do not regenerate per CI run; do not grow these.

`sample-1.avif` (TASK-01-012) was produced the same way, via macOS `sips`:

    sips -s format avif -s formatOptions low sample-1.heic --out sample-1.avif

Same rules apply: committed once, a few KB, never regenerated in CI.

`sample-1.jxl` (TASK-04-014) is a 64×48 JPEG XL, produced once with
pillow-jxl-plugin (libjxl encoder bindings; no `cjxl` system dependency in
CI — jxl-oxide is decode-only so no Rust encoder exists):

    python3 -c "from PIL import Image; import pillow_jxl; \
Image.new('RGB',(64,48)).save('sample-1.jxl', quality=90)"

Same rules apply: committed once, a few KB, never regenerated in CI.
