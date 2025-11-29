## Agent Evolution — Part 3

I hit the wall again. The remaining questions I have are listed below along with my current thinking, trade-offs, and a pragmatic path forward.

### The questions I'm left with

1. If I want Trustee to be modular and every module replaceable, where should I keep the types: `trustee[types]` or `abk[types]`? What about `CATS` and `UMF` — keep all types in `trustee`? This feels like an egg-and-chicken problem. C experts solved ABI with headers — how do they do it? What does Kubernetes do? What about PostgreSQL?
2. What if I want to use another programming language (not Rust) as a native extension rather than WASM? What's the pragmatic option?
3. What are the low-hanging fruits in the current architecture based on established software-engineering practices?
4. I'm unhappy with `abk[agent]` and `abk[orchestration]` — I was forced to create them to extract `abk[cli]`. How could I remove or migrate those concepts so the design is cleaner?
5. I think Data Flow is the core idea for composing software. UDML felt like a dead end — what are current trends and similar ideas? Is there a workable approach?

---
