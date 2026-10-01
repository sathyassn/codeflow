---
id: ADR-0045
uid: 87f62666-6c09-42f3-9656-b3a175518538
title: define durable work authority and canonical record layout
date: 2026-07-28
status: superseded
superseded_by: ADR-0046
architecture_impact: docs/architecture.md — project-management records gain one canonical flat layout, shared enumeration, legacy read compatibility, and spec recall
---

# ADR-0045: durable work authority and canonical record layout

## Context

CodeFlow's CLI writes flat epic and task files, while some readers and comments
treated a historical nested layout as canonical. Status, validation, allocation,
and recall consequently maintained separate discovery logic; nested tasks were
not visible to the typed store, and frozen specs were promised as recallable but
were not indexed. Consuming projects also need to coexist with team trackers and
spec-driven methods without duplicating work status or introducing another
database authority.

## Decision

CodeFlow writes one flat stable-ID namespace:
`epics/EPC-NNN.md`, `tasks/TSK-NNN-MMM.md`, and `specs/SPC-NNN.md`.
Core readers share symlink-safe record enumeration and retain read-only
compatibility for historical nested epic/task records. Specs join the
project-management recall corpus.

Each durable work item has one authority for status, acceptance, and lifecycle.
CodeFlow markdown is the default for finite repo-local gated work; established
external trackers or planning methods remain authoritative where their
operating shape requires it. `external_refs` carries opaque links only—never
mirrored status. Host-local databases remain rebuildable caches or single-host
runtime stores, not shared team truth.

Task closeout records review-relevant bounded discoveries and evidence. A
material graph, scope, ownership, acceptance/interface, or safety change must
receive dual-approved Plan vN+1 before implementation continues; closeout cannot
authorize it retrospectively. Shared engineering doctrine remains in
`AGENTS.md` and skills rather than being copied into every work item.

## Consequences

Paths are predictable and stable across single-surface repositories and
monorepos, while existing nested records remain usable. External methods can
compose with CodeFlow's worktree, review, verification, and PR controls without
brand-specific adapters or duplicate task trees. Independent worktrees can
still allocate the same sequential ID before either commit is visible, so
multi-task plans must serialize work-item allocation or resolve the visible
same-path conflict.

The flat namespace is less visually grouped on disk than nested folders; stable
IDs, parent metadata, search, and generated status provide that grouping
without multiplying path conventions.

## Architecture impact

`docs/architecture.md` now names the canonical flat paths, shared enumerator,
legacy read compatibility, and spec recall behavior.
