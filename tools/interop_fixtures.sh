#!/usr/bin/env bash
# TASK-03-004: darktable/digiKam interop fixtures.
#
# Regenerates the checked-in sidecar fixtures with the real tools and diffs
# them against the copies in crates/metadata/tests/fixtures/interop/. Runs in
# CI (ubuntu-22.04 ships darktable 3.8.1 — the same version the fixtures were
# produced with; PLAN §4 floor). digiKam has no headless CLI, so its fixture
# is produced by the exiv2 read-modify-write serialization digiKam itself
# uses (00-010 fallback: real-tool output, no GUI).
#
# Usage: tools/interop_fixtures.sh [--update]
#   default: regenerate + diff, exit 1 on drift
#   --update: overwrite the checked-in fixtures (tool version bump only)
set -euo pipefail

cd "$(dirname "$0")/.."
FIXDIR="crates/metadata/tests/fixtures/interop"
UPDATE=0
[[ "${1:-}" == "--update" ]] && UPDATE=1

command -v darktable-cli >/dev/null || { echo "darktable-cli not found"; exit 1; }
command -v exiv2 >/dev/null || { echo "exiv2 not found"; exit 1; }
DT_VER="$(darktable-cli --version 2>&1 | head -1 | awk '{print $NF}')"
EXIV2_VER="$(exiv2 -V | head -1 | awk '{print $NF}')"
echo "darktable-cli $DT_VER / exiv2 $EXIV2_VER"

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT
cd "$WORK"

# 1x1 JPEG, no metadata (deterministic bytes; generated data is never committed).
python3 - <<'EOF'
import base64
jpg = base64.b64decode("/9j/4AAQSkZJRgABAQEAYABgAAD/2wBDAAgGBgcGBQgHBwcJCQgKDBQNDAsLDBkSEw8UHRofHh0aHBwgJC4nICIsIxwcKDcpLDAxNDQ0Hyc5PTgyPC4zNDL/2wBDAQkJCQwLDBgNDRgyIRwhMjIyMjIyMjIyMjIyMjIyMjIyMjIyMjIyMjIyMjIyMjIyMjIyMjIyMjIyMjIyMjIyMjL/wAARCAABAAEDASIAAhEBAxEB/8QAHwAAAQUBAQEBAQEAAAAAAAAAAAECAwQFBgcICQoL/8QAtRAAAgEDAwIEAwUFBAQAAAF9AQIDAAQRBRIhMUEGE1FhByJxFDKBkaEII0KxwRVS0fAkM2JyggkKFhcYGRolJicoKSo0NTY3ODk6Q0RFRkdISUpTVFVWV1hZWmNkZWZnaGlqc3R1dnd4eXqDhIWGh4iJipKTlJWWl5iZmqKjpKWmp6ipqrKztLW2t7i5usLDxMXGx8jJytLT1NXW19jZ2uHi4+Tl5ufo6erx8vP09fb3+Pn6/8QAHwEAAwEBAQEBAQEBAQAAAAAAAAECAwQFBgcICQoL/8QAtREAAgECBAQDBAcFBAQAAQJ3AAECAxEEBSExBhJBUQdhcRMiMoEIFEKRobHBCSMzUvAVYnLRChYkNOEl8RcYGRomJygpKjU2Nzg5OkNERUZHSElKU1RVVldYWVpjZGVmZ2hpanN0dXZ3eHl6goOEhYaHiImKkpOUlZaXmJmaoqOkpaanqKmqsrO0tba3uLm6wsPExcbHyMnK0tPU1dbX2Nna4uPk5ebn6Onq8vP09fb3+Pn6/9oADAMBAAIRAxEAPwD3+oooooA//9k=")
open("photo.jpg", "wb").write(jpg)
EOF

# --- digiKam fixture: a packet with the fields digiKam writes (standard
# PLAN §5 fields plus digiKam:TagsList/ImageHistory), rewritten by exiv2 —
# the same XMP Toolkit serialization digiKam ships.
cat > digi-src.xmp <<'EOF'
<?xpacket begin="" id="W5M0MpCehiHzreSzNTczkc9d"?>
<x:xmpmeta xmlns:x="adobe:ns:meta/" x:xmptk="digiKam 8.4.0">
 <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
  <rdf:Description rdf:about=""
    xmlns:dc="http://purl.org/dc/elements/1.1/"
    xmlns:xmp="http://ns.adobe.com/xap/1.0/"
    xmlns:exif="http://ns.adobe.com/exif/1.0/"
    xmlns:lr="http://ns.adobe.com/lightroom/1.0/"
    xmlns:digiKam="http://www.digikam.org/ns/1.0/">
   <dc:description><rdf:Alt><rdf:li xml:lang="x-default">A beach sunset</rdf:li></rdf:Alt></dc:description>
   <dc:subject><rdf:Bag><rdf:li>Family</rdf:li><rdf:li>Vacation/2024</rdf:li></rdf:Bag></dc:subject>
   <lr:hierarchicalSubject><rdf:Bag><rdf:li>Vacation|2024</rdf:li></rdf:Bag></lr:hierarchicalSubject>
   <xmp:Rating>4</xmp:Rating>
   <exif:GPSLatitude>52,31N</exif:GPSLatitude>
   <exif:GPSLongitude>13,24E</exif:GPSLongitude>
   <digiKam:TagsList><rdf:Bag><rdf:li>Family</rdf:li><rdf:li>Vacation/2024</rdf:li></rdf:Bag></digiKam:TagsList>
   <digiKam:ImageHistory>diffedit=1</digiKam:ImageHistory>
  </rdf:Description>
 </rdf:RDF>
</x:xmpmeta>
<?xpacket end="w"?>
EOF
exiv2 -M "set Xmp.digiKam.ImagePickLabel 3" digi-src.xmp
mv digi-src.xmp digikam-fixture.xmp

# --- darktable fixture: real darktable-cli export; darktable rewrites the
# packet in its own form (attribute simple properties, deg,min GPS, its
# darktable: history) — that output is what Fotobase must parse.
exiv2 -M "add Xmp.dc.subject Family" -M "add Xmp.dc.subject Sunset" \
      -M "add Xmp.lr.hierarchicalSubject Vacation|2024" \
      -M "set Xmp.dc.description lang=x-default A beach sunset" \
      -M "set Xmp.xmp.Rating 4" \
      -M "set Xmp.exif.GPSLatitude 52,31N" -M "set Xmp.exif.GPSLongitude 13,24E" \
      photo.jpg
rm -rf dtcfg
darktable-cli photo.jpg photo-out.jpg --core --configdir dtcfg >/dev/null 2>&1
exiv2 -eX photo-out.jpg
# Timestamps are the only nondeterministic bits — normalize before diffing.
sed -E \
  -e 's/xmp:ModifyDate="[^"]*"/xmp:ModifyDate="TIMESTAMP"/' \
  -e 's/photoshop:DateCreated="[^"]*"/photoshop:DateCreated="TIMESTAMP"/' \
  photo-out.xmp > darktable-fixture.xmp

cd - >/dev/null
for name in darktable digikam; do
  src="$WORK/${name}-fixture.xmp"
  dst="$FIXDIR/${name}-fixture.xmp"
  if [[ "$UPDATE" == 1 ]]; then
    cp "$src" "$dst"
    echo "updated $dst"
  elif diff -u "$dst" "$src" >/dev/null 2>&1; then
    echo "OK: $dst matches live tool output"
  else
    echo "DRIFT: $dst differs from darktable $DT_VER / exiv2 $EXIV2_VER output:"
    diff -u "$dst" "$src" | head -40
    exit 1
  fi
done
