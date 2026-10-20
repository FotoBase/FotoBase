# task: add s3-compatible export plugin

**Feature:** [FEAT-08-02-export-plugin-system](FEAT-08-02-export-plugin-system.md) · **Epic:** [EPIC-08 — Output & sharing](../EPIC-08.md) · **Status:** todo

## Description
Export plugin for S3-compatible object storage (AWS S3, Cloudflare R2, MinIO — PLAN §7 F1 layer 1 lists these as web-album targets; P4 plugin list includes S3-compatible).

## Functional specs
- User configures endpoint, bucket, region, access/secret keys (or credentials file); validation with clear errors.
- Uploads with size choice; album mapping via key prefix (album name → prefix).
- Resumable multipart uploads; idempotent via content-key object naming.

## Technical specs
- Use the official-ish Rust S3 SDK or `object_store` crate (already general-purpose — prefer it over hand-rolled signing).
- Multipart upload with parts tracked so interruption resumes; abort leaves no partial objects visible.
- Object key: `<prefix>/<album>/<content-key>_<filename>`; retries overwrite the same key (idempotent).
- Contract suite against a MinIO testcontainer.

## Testing criteria (TDD)
- Integration: contract suite vs MinIO — upload/resume/retry/album-prefix mapping.
- Integration: kill mid-multipart → resume → single complete object, no visible partial.

## Acceptance criteria
- [ ] 08-010: S3-compatible plugin passes the contract suite against MinIO with prefix-based album mapping.
- [ ] 08-010: Multipart uploads resume without partial objects.
