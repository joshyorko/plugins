# LF-07 implementation and verification

1. Finish PR #62 hosted CI, enforce compiled graph demonstration, correct LF-05 completion scope and reproduce/fix the independent context-clear finding.
2. Extend the existing report contract with explicit bounded continuation; preserve legacy-stop behavior and read-only recovery origins.
3. Implement current admitted task/failed-check guards and durable same-owner dispatch without replacing the active observer.
4. Prove behavior and denials with dedicated fixture integration plus existing safety regression suites.
5. Record exact commits/test/CI evidence, keep #62 draft, and create a separate stacked draft for LF-07. No production changes or live workers.

One integration owner commits and pushes. Runtime implementation, fixture tests and UI race repair have disjoint file ownership. Independent safety review checks the resulting runtime diff before acceptance.
