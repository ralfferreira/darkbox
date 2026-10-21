# ADR 0002: exact endpoint records and separate route groups

Status: accepted, 2026-10-21.

## Context

A URL alone loses method, effective port, parameter names, encoded separators,
trailing slashes and fragment state. Merging aggressively makes an inventory
look tidy while destroying evidence.

## Decision

Store an exact `EndpointKey` for each observation: scheme, lowercase host,
effective port, uppercase method, exact path, ordered parameter names, raw
query and fragment. The record key uses method, origin, path and parameter
names; values never enter it. Store a route pattern separately with a version.

## Consequences

- `GET /a`, `GET /a/`, `POST /a` and `/a%2Fb` remain distinct.
- Generated identifiers group under `{id}` without rewriting the record.
- Parameter values live only in restricted evidence, never in the inventory.
- Inventory can show both the exact path and a grouped route, with the grouping
  labelled as an interpretation.
