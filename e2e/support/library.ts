// Throw-away photo library for the e2e specs: a RAW+JPEG pair that the pair
// detector must offer, plus one unrelated photo. The JPEG is a real 16x16
// image (the scanner and thumbnailer see a valid file); the RAW is opaque
// bytes — only its name and extension matter to the catalog.

import { createHash } from "node:crypto";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";

// A real 16x16 JPEG (gradient, verified against the strict zune-jpeg
// decoder the render pipeline uses - the previous hand-made base64 was a
// malformed stream no backend could decode, TASK-04-010).
const TINY_JPEG_BASE64 =
  "/9j/4AAQSkZJRgABAgAAAQABAAD/wAARCAAQABADAREAAhEBAxEB/9sAQwADAgICAgIDAgICAwMDAwQGBAQEBAQIBgYFBgkICgoJCAkJCgwPDAoLDgsJCQ0RDQ4PEBAREAoMEhMSEBMPEBAQ/9sAQwEDAwMEAwQIBAQIEAsJCxAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQ/8QAHwAAAQUBAQEBAQEAAAAAAAAAAAECAwQFBgcICQoL/8QAtRAAAgEDAwIEAwUFBAQAAAF9AQIDAAQRBRIhMUEGE1FhByJxFDKBkaEII0KxwRVS0fAkM2JyggkKFhcYGRolJicoKSo0NTY3ODk6Q0RFRkdISUpTVFVWV1hZWmNkZWZnaGlqc3R1dnd4eXqDhIWGh4iJipKTlJWWl5iZmqKjpKWmp6ipqrKztLW2t7i5usLDxMXGx8jJytLT1NXW19jZ2uHi4+Tl5ufo6erx8vP09fb3+Pn6/8QAHwEAAwEBAQEBAQEBAQAAAAAAAAECAwQFBgcICQoL/8QAtREAAgECBAQDBAcFBAQAAQJ3AAECAxEEBSExBhJBUQdhcRMiMoEIFEKRobHBCSMzUvAVYnLRChYkNOEl8RcYGRomJygpKjU2Nzg5OkNERUZHSElKU1RVVldYWVpjZGVmZ2hpanN0dXZ3eHl6goOEhYaHiImKkpOUlZaXmJmaoqOkpaanqKmqsrO0tba3uLm6wsPExcbHyMnK0tPU1dbX2Nna4uPk5ebn6Onq8vP09fb3+Pn6/9oADAMBAAIRAxEAPwD5V8KfCn7n+jfpX7BmHEG+p5/B/FPw+8ezeE/hT9z/AEb9K+FzDiDfU/qvg/in4fePYvCfwp+5/o3p2r4jMOIN9T/Mbg/ij4fePZvCnwp+5/o36V8LmHEG+p/VXB/FHw+8f//Z";

export const PAIR_JPEG = "IMG_0001.jpg";
export const PAIR_RAW = "IMG_0001.cr2";
export const SOLO = "solo.jpg";

export interface Library {
  dir: string;
  /** SHA-256 of every original, taken before the app saw the folder. */
  checksums: Record<string, string>;
}

export function sha256(file: string): string {
  return createHash("sha256").update(fs.readFileSync(file)).digest("hex");
}

export function makeLibrary(): Library {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "fb-e2e-lib-"));
  const jpeg = Buffer.from(TINY_JPEG_BASE64, "base64");
  fs.writeFileSync(path.join(dir, PAIR_JPEG), jpeg);
  fs.writeFileSync(path.join(dir, PAIR_RAW), Buffer.alloc(256, 0x42));
  fs.writeFileSync(path.join(dir, SOLO), jpeg);

  // One shutter press: the pair shares its timestamp exactly, so the
  // detector's window is not what this spec exercises.
  const stamp = new Date("2024-06-01T10:00:00Z");
  for (const name of [PAIR_JPEG, PAIR_RAW, SOLO]) {
    fs.utimesSync(path.join(dir, name), stamp, stamp);
  }

  const checksums: Record<string, string> = {};
  for (const name of [PAIR_JPEG, PAIR_RAW, SOLO]) {
    checksums[name] = sha256(path.join(dir, name));
  }
  return { dir, checksums };
}

// TASK-06-002: a geotagged library for the map-pin specs. `count` tiny
// JPEGs in one tight cluster around the panel's initial center
// (map-src.ts MAP_INITIAL_CENTER), so the world view shows a single
// cluster carrying every photo and a zoom-in resolves it. GPS lives in
// `.xmp` sidecars — the source of truth (PLAN §5) — as plain decimal
// degrees, which the sidecar reader accepts.
//
// `plain` (TASK-06-007) adds untagged JPEGs (`plain_NNN.jpg`, no sidecar)
// to the same folder, so a spec can exercise the "Geotagged" filter over a
// mixed library.
//
// `center` (TASK-06-008) overrides the cluster center; the default keeps
// the panel's initial view. The reverse-geocode spec passes Lisbon, so the
// embedded GeoNames dataset resolves every photo to the same city.
export function makeGeotaggedLibrary(
  count = 40,
  plain = 0,
  center: { lat: number; lon: number } = { lat: 25, lon: 10 },
): Library {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "fb-e2e-geo-"));
  const jpeg = Buffer.from(TINY_JPEG_BASE64, "base64");
  const checksums: Record<string, string> = {};
  for (let i = 0; i < count; i += 1) {
    // One tight grid around the requested center.
    const lat = center.lat + (i % 8) * 0.02;
    const lon = center.lon + Math.floor(i / 8) * 0.02;
    const name = `geo_${String(i).padStart(3, "0")}.jpg`;
    // A per-photo payload byte: identical files would share one content
    // hash, and the catalog keys pins by hash — the map would draw a
    // single pin for the whole library (TASK-06-004 selection needs one
    // pin per photo).
    const payload = Buffer.concat([jpeg, Buffer.from([i])]);
    fs.writeFileSync(path.join(dir, name), payload);
    fs.writeFileSync(
      path.join(dir, `${name}.xmp`),
      `<?xpacket begin="﻿" id="W5M0MpCehiHzreSzNTczkc9d"?>
<x:xmpmeta xmlns:x="adobe:ns:meta/">
 <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
  <rdf:Description rdf:about=""
    xmlns:exif="http://ns.adobe.com/exif/1.0/"
   exif:GPSLatitude="${lat}"
   exif:GPSLongitude="${lon}">
  </rdf:Description>
 </rdf:RDF>
</x:xmpmeta>
<?xpacket end="w"?>
`,
    );
    checksums[name] = sha256(path.join(dir, name));
  }
  for (let i = 0; i < plain; i += 1) {
    const name = `plain_${String(i).padStart(3, "0")}.jpg`;
    fs.writeFileSync(
      path.join(dir, name),
      Buffer.concat([jpeg, Buffer.from([count + i])]),
    );
    checksums[name] = sha256(path.join(dir, name));
  }
  return { dir, checksums };
}
