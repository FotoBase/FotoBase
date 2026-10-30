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
  "/9j/4AAQSkZJRgABAgAAAQABAAD/wAARCAAQABADAREAAhEBAxEB/9sAQwADAgICAgIDAgICAwMDAwQGBAQEBAQIBgYFBgkICgoJCAkJCgwPDAoLDgsJCQ0RDQ4PEBAREAoMEhMSEBMPEBAQ/9sAQwEDAwMEAwQIBAQIEAsJCxAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQ/8QAHwAAAQUBAQEBAQEAAAAAAAAAAAECAwQFBgcICQoL/8QAtRAAAgEDAwIEAwUFBAQAAAF9AQIDAAQRBRIhMUEGE1FhByJxFDKBkaEII0KxwRVS0fAkM2JyggkKFhcYGRolJicoKSo0NTY3ODk6Q0RFRkdISUpTVFVWV1hZWmNkZWZnaGlqc3R1dnd4eXqDhIWGh4iJipKTlJWWl5iZmqKjpKWmp6ipqrKztLW2t7i5usLDxMXGx8jJytLT1NXW19jZ2uHi4+Tl5ufo6erx8vP09fb3+Pn6/8QAHwEAAwEBAQEBAQEBAQAAAAAAAAECAwQFBgcICQoL/8QAtREAAgECBAQDBAcFBAQAAQJ3AAECAxEEBSExBhJBUQdhcRMiMoEIFEKRobHBCSMzUvAVYnLRChYkNOEl8RcYGRomJygpKjU2Nzg5OkNERUZHSElKU1RVVldYWVpjZGVmZ2hpanN0dXZ3eHl6goOEhYaHiImKkpOUlZaXmJmaiqOkpaanqKmqsrO0tba3uLm6usLDxMXGx8jJytLT1NXW19jZ2uHi4+Tl5ufo6erx8vP09fb3+Pn6/9oADAMBAAIRAxEAPwD5V8KfCn7n+jfpX7BmHEG+p5/B/FPw+8ezeE/hT9z/AEb9K+FzDiDfU/qvg/in4fePYvCfwp+5/o3p2r4jMOIN9T/Mbg/ij4fePZvCnwp+5/o36V8LmHEG+p/VXB/FHw+8f//Z";

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
