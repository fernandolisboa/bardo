# ADR-0002: MVP includes all four pillars

- Status: Accepted
- Date: 2026-09-30

## Context

The product has four pillars, in priority order: Strategy, Production, Editing, Publishing. The alternatives considered for the MVP were a thin vertical slice (manual niche, generate, simple timeline, export, manual upload) or a single pillar first.

## Decision

The MVP includes **all four pillars**. Scope is controlled by the **depth** of each pillar, defined in [`docs/spec/mvp.md`](../spec/mvp.md), not by leaving pillars out.

## Consequences

- The value of the product (the whole workflow in one place) is present from the first release.
- The MVP is larger; the depth cuts in the spec are the main scope lever and must be defended in review.
- Decisions that could otherwise wait (market data source, publishing networks, scheduling) had to be made up front: see ADR-0003 to ADR-0006.
- The biggest technical risk (timeline + ffmpeg + GPUI) and the biggest external risk (platform audits) are both on the MVP path and should be started first.
