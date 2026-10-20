# ADR-0010: Name diligence sweep — registries and domains

**Date:** 2026-10-20 · **Status:** accepted (registry checks done; claims/registration pending) · **Task refs:** 00-005, 00-006

## Context

PLAN §12 requires handle/domain availability checks before the first public commit. "Fotobase" was chosen in ADR-0004; this ADR records the empirical sweep.

## Decision (findings, 2026-10-20)

| Registry | Check | Result |
|---|---|---|
| GitHub `FotoBase/FotoBase` | HTTP probe | **Free** at check (2026-10-20, 404 — no user/org) → org + repo created same day |
| crates.io `fotobase` | Registry API | **Free** (404 — no crate) → to claim |
| npm `fotobase` | Registry API | **Free** (404 — no package) → to claim |
| `fotobase.app` | whois + DNS | **Taken** (registered, name.com NS, resolves) |
| `fotobase.dev` | Google RDAP | **Free** (404 — unregistered) |
| `fotobase.io` | whois (`Domain not found`) | **Free** |

**Domain pick:** `.dev` (first free choice in the §12 preference order `.app`/`.dev`/`.io`; Google-registry, HTTPS-enforced — fits a product site). `.io` as fallback.

## Consequences (updated 2026-10-20 after scope review)

- **crates.io / npm dropped:** the app ships via GitHub Releases only — no packages will be published to either registry, so no claims are needed. Both names verified free anyway (above) in case that ever changes.
- **GitHub:** org **`FotoBase`** created 2026-10-20, monorepo **`FotoBase/FotoBase`** (`github.com/FotoBase/FotoBase.git`). Single repo per PLAN §4 (app + crates + frontend + tools); splits out only if the plugin ecosystem or standalone tools demand it.
- **Domain deferred:** `fotobase.dev` and `fotobase.io` verified free (2026-10-20); registration postponed until a website exists. Not a pre-commit blocker — the repo itself is the presence.
- No rename trigger; trademark go recorded in ADR-0011.
