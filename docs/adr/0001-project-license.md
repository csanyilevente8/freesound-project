# ADR-0001: Project license

**Status:** Proposed (GPL-3.0-or-later) — confirmed with maintainer for M0,
recorded as proposed because publishing is not imminent and the choice remains
reversible until first public release.

## Context

The product is a free and open-source DAW (spec §2.1, §3.2). The license must
be chosen explicitly before publishing and must be compatible with the
dependencies pulled in. The spec names GPL-3.0-or-later (copyleft) and a
permissive license as the two candidate directions.

## Options

1. **GPL-3.0-or-later (copyleft).** Keeps derivative *distributions* open.
   Strongest alignment with "the complete application is free to use" and with
   a community DAW that should not be absorbed into a closed product.
   Constraint: all linked code must be GPL-compatible. Permissive dependencies
   (MIT/Apache-2.0) are compatible; other copyleft/incompatible licenses are
   not.
2. **MIT or Apache-2.0 (permissive).** Maximizes reuse, including in closed
   products. Lower friction for dependencies and plugin ecosystems, but allows
   closed-source forks.
3. **Dual / MPL-2.0 (file-level copyleft).** Middle ground; more complex to
   administer.

## Decision

Adopt **GPL-3.0-or-later** for the application. Rationale: the project's stated
principle is a fully free application and preserving that for downstream
distributions matters more than permitting closed forks. All current
dependencies are MIT/Apache-2.0 (GPL-compatible).

The `license` field in every crate manifest is set to `GPL-3.0-or-later`.

## Consequences

- Every new dependency must be checked for GPL compatibility (tracked in the
  dependency inventory in `docs/architecture/overview.md`).
- Future third-party plugin SDK hosting (VST3/AU/LV2) has licensing nuances
  that must be revisited in a dedicated ADR before implementation.
- A `LICENSE` file with the full GPL-3.0 text must be added before first public
  release (deferred; not required for the private M0 foundation).

## Revisit when

Before the first public release, or if a required dependency proves
GPL-incompatible.
