# ADR-0004: Market data for niche strategy and post-upload tracking

- Status: Accepted
- Date: 2026-09-30

## Context

The Strategy pillar recommends niches and themes for high-CPM markets. There is no public API for CPM by niche and country. The YouTube Analytics API exposes real revenue metrics only for the owner's own monetized channels.

Options considered: a curated local CPM table, a paid market-data provider, and on-demand AI web research.

## Decision

CPM is treated as a **post-upload statistic**, not a pre-upload input.

- **Before upload (Strategy)**: niches and themes are scored on **competition** and **trend**, computed from the YouTube Data API (recent upload volume, view velocity, channel sizes), filtered by the channel's target country and language. Claude proposes candidate themes; JEV ranks them with typed reasons. No CPM table.
- **After upload (tracking)**: a metrics sync job pulls per-publication statistics and aggregates them per channel.
  - YouTube: views, watch time, retention and, for monetized channels, estimated revenue, CPM and RPM (YouTube Analytics API).
  - TikTok and Instagram: views, retention and engagement where the API exposes them. No revenue.
- Over time, JEV also ranks niches by what performed on the user's own channels.

## Consequences

- No invented numbers in the product; every figure has a traceable source.
- YouTube Data API search is quota-expensive; results are cached in SQLite and refreshed on demand, not continuously.
- A paid provider remains possible later as another adapter behind the same interface.
