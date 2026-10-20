# task: implement local contacts link

**Feature:** [FEAT-05-03-storage-ui](FEAT-05-03-storage-ui.md) · **Epic:** [EPIC-05 — People](../EPIC-05.md) · **Status:** todo

## Description
Contacts link: map person names to local OS contacts only — no network calls, no telemetry (PLAN §2G "Contacts link — local contacts only"; §6 P3 People).

## Functional specs
- A named person can be linked to a local contact (name/email from the OS contacts store).
- Linking enriches the People Manager entry (e.g. shows contact name/email).
- Strictly local: no network requests ever (test-enforced).

## Technical specs
- OS contacts access via platform APIs (macOS Contacts / Windows People / Linux desktop portal) behind a small trait in the Tauri shell.
- Mapping stored in catalog (`people` extension); the contact data itself is never copied — only a reference.

## Testing criteria (TDD)
- Integration: link a person to a mock contact → mapping persisted and shown in People Manager.
- Unit: contacts module performs zero network calls (no network client in dependency graph / mocked socket test).

## Acceptance criteria
- [ ] 05-011: Contacts link maps names to local contacts only; no network calls, no telemetry.
