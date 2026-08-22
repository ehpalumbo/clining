---
type: decision-record
title: "Application-Owned Ports and Owned Service Dependencies"
description: "Moves the port traits from domain to application and has application services own their dependencies as trait objects, eliminating generic and lifetime plumbing from service signatures."
tags:
  - "clining"
  - "architecture"
  - "ports-and-adapters"
  - "dependency-injection"
timestamp: "2026-08-22T23:11:43Z"
related:
  - "[Architecture Overview](../architecture/overview.md)"
resource:
  - "src/application/ports.rs"
  - "src/application/api_model.rs"
  - "src/application/api_invocation.rs"
---

# ADR-001: Application-Owned Ports and Owned Service Dependencies

## Context

The port traits (`ApiStore`, `SpecLoader`, `OpenApiParser`, `HttpInvoker`) originally lived in the domain layer, yet nothing inside the domain ever called them — they exist to be consumed by application services and implemented by infrastructure adapters. Keeping them in the domain implied a domain-level abstraction that the domain itself does not use.

Meanwhile, services were generic over every dependency and borrowed shared state through references: the learn service carried a store reference behind an explicit lifetime, and the CLI app accumulated four generic parameters plus a lifetime just to hold two services. Passing references required blanket trait implementations for `&T` so borrowed adapters would satisfy the ports. Each new service or dependency multiplied type and lifetime noise outward toward the composition root.

## Decision

Ports belong to the layer that consumes them: all four traits now live in the application layer. Application services own their dependencies outright as boxed trait objects — constructors accept any concrete implementation and store it behind a port object, so service types carry no generic parameters or lifetimes.

As part of this reshaping, the narrow learn use case evolved into a broader API model service that both installs models from specs and loads installed models by name; operation invocation became its own self-contained invocation service. Model loading moved behind the model service rather than being exposed as a separate store handle on the CLI app.

## Trade-offs

- **Dynamic dispatch vs. monomorphization**: boxed ports give up static dispatch and compile-time specialization. For clining's call volumes (one spec load per install, one HTTP round trip per invoke) vtable indirection is negligible next to network and filesystem I/O.
- **Looser static guarantees**: generic parameters document at the type level which concrete adapters are wired where; owned trait objects move that knowledge to the composition root. The wiring is still centralized in `main.rs`, only less verbose.
- **Ownership over sharing**: services own their dependencies instead of borrowing them, so adapters cannot be shared between services by reference. Nothing needed that ability; if it ever arises, interior-shared adapters remain possible.

## Consequences

- Service signatures and the CLI app are plain structs; adding a dependency no longer ripples type parameters into downstream constructors.
- Blanket implementations of ports for references were removed along with the need for them.
- The domain is reduced to what is genuinely core: entities, naming rules, and the error vocabulary.
- New use cases slot into existing services (as model loading did) without touching presentation-layer signatures.
