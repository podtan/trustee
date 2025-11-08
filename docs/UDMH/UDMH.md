# Unified Data Morphism Hypothesis (UDMH)

## Abstract

All software systems, regardless of paradigm, can be fully described and constructed using only **data** and the **rules that govern how data can be accessed, manipulated, extracted, and moved**.  
Behavior, architecture, patterns, objects, functions, modules, and services are all secondary representations of these primary data relationships.

## Core Principle

> **The only truly fundamental primitive in software is data, and all software is the controlled morphing of data.**

## The Five Data Domains

| Domain         | Definition |
|----------------|------------|
| **Information** | The real world state represented in software. Persistent semantic domain data. |
| **Access**      | The permissible ways this data may be viewed or read. Includes boundaries, auth, queries, visibility and indexing. |
| **Manipulation**| The lawful mutation rules of data: creation, change, deletion, invalidation or correction. |
| **Extract**     | Derivation of new representations of data from existing data or environment. Includes inference, computation, aggregation, projection. |
| **Movement**    | How data travels through processes, boundaries, runtimes, apps, microservices, protocols, and APIs. Routing + distribution. |

## Hypothesis

Any software system can be synthesized, analyzed, and reasoned about entirely by:

1. Defining all **Information** forms  
2. Defining all **Access** rules  
3. Defining all **Manipulation** rules  
4. Defining all **Extract** rules  
5. Defining all **Movement** rules

…and an engine capable of interpreting these 5 categories can generate the complete executable system — regardless of implementation language, paradigm, or runtime.

## Implications

- Architecture reduces to data calculus.  
- State safety becomes rule safety.  
- Code generation becomes deterministic.  
- LLMs become extremely reliable system builders because the design space is structured and composable.  
- OOP / FP / Agents / Microservices become simply strategy choices for encoding morph rules — not conceptual foundations.

## Modular
Two questions. Two rules. That's it.

1. Does this need different ownership? → New module
2. Does this need new movement? → New movement rule (evaluate if new module needed)

Everything else (Access, Manipulation, Extract) stays within the owning module.
This keeps modularity simple while preserving UDMH's data-centric philosophy.
