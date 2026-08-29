# Monorepo areas and shared contracts

Ordinary repository Markdown. Retained as the plain baseline for the `area-drilldown` page
family; it is not a design.

## Areas

- **web** (`apps/web`) — auth token schema (consumes), product catalog API (consumes), locale bundle format (owns), telemetry field names (consumes), design tokens (consumes)
- **iOS** (`apps/ios`) — auth token schema (consumes), product catalog API (migrating), locale bundle format (consumes), design tokens (consumes)
- **Android** (`apps/android`) — auth token schema (consumes), product catalog API (migrating), locale bundle format (consumes)
- **identity service** (`services/identity`) — auth token schema (owns), event envelope (consumes), telemetry field names (consumes)
- **catalog service** (`services/catalog`) — product catalog API (owns), event envelope (consumes), auth token schema (consumes)
- **data platform** (`platform/data`) — event envelope (consumes), telemetry field names (consumes)

## Shared contracts

- **auth token schema** — owner: identity service; touched by 5 areas
- **event envelope** — owner: none; touched by 3 areas
- **product catalog API** — owner: catalog service; touched by 4 areas
- **locale bundle format** — owner: web; touched by 3 areas
- **telemetry field names** — owner: none; touched by 3 areas
- **design tokens** — owner: none; touched by 2 areas

## Contracts with no owner

event envelope, telemetry field names, design tokens — 3 of 6.
