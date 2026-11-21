# Explicit Update and Remove

Real launcher captures from `a1242fd2e6ddfc4b1b52884dc0565b889e1b4f84`, using fresh Linux homes, an unsigned local developer game and a local test registry. These are not screenshots of a signed distribution or a production SDK.

| State | 1280×800 | 800×600 |
| --- | --- | --- |
| Installed, with single Update and Update all | [Open](installed-1280x800.png) | [Open](installed-800x600.png) |
| Single Update, two affected worlds | [Open](single-1280x800.png) | [Open](single-800x600.png) |
| After cancelling the single Update | [Open](cancelled-1280x800.png) | [Open](cancelled-800x600.png) |
| Update all, two replacements and three affected worlds | [Open](all-1280x800.png) | [Open](all-800x600.png) |
| After approving Update all | [Open](updated-1280x800.png) | [Open](updated-800x600.png) |
| One Remove question: the selected mod, two dependents and three affected worlds | [Open](remove-1280x800.png) | [Open](remove-800x600.png) |
| The same Remove question without saved worlds | [Open](remove-no-worlds-1280x800.png) | [Open](remove-no-worlds-800x600.png) |
| After cancelling Remove | [Open](remove-cancelled-1280x800.png) | [Open](remove-cancelled-800x600.png) |
| After approving Remove: the dependents are still installed and off | [Open](removed-1280x800.png) | [Open](removed-800x600.png) |
| Missing dependency prevents direct re-enable | [Open](activation-blocked-1280x800.png) | [Open](activation-blocked-800x600.png) |
| The held native mod is not on Home | [Open](held-home-1280x800.png) | [Open](held-home-800x600.png) |
| Installed can update the held mod; direct enable stays disabled | [Open](held-1280x800.png) | [Open](held-800x600.png) |
| A proved replacement restores the held mod without another build prompt | [Open](restored-1280x800.png) | [Open](restored-800x600.png) |
| A failed replacement offers to keep the working version | [Open](failed-1280x800.png) | [Open](failed-800x600.png) |
| After Continue, the old version stays on | [Open](kept-predecessor-1280x800.png) | [Open](kept-predecessor-800x600.png) |

The capture runs checked the committed pointer, installed rows, enable list and local archives after both cancellations, and checked the exact resulting versions and enabled set after approval. The Remove ran through the real launcher and left both dependent packages byte-for-byte unchanged. The native fixture compiled the staged Rust source via the launcher's compiler worker. The hold is a seeded typed receipt, not a run of Switch; Update retired it only after proof. Continue after the deliberate compiler error preserved the existing pointer, rows, list and archive.
