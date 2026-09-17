# journal card eval

- run at: 2026-09-17T07:09:29.880456+00:00
- fixtures: `crates/screenpipe-engine/tests/fixtures/journal`
- labels: `crates/screenpipe-engine/tests/fixtures/journal/synthetic.labels.json`
- preset: `default (deepseek)`
- model: `deepseek/deepseek-v4.1-flash`
- prompt: `journal-cards-v4`
- intention: "Ship the journal MVP"

## Per fixture

| fixture | valid | cards | attempts | repairs | category (expected → actual) | relation (expected → actual) | latency | tokens | note |
|---|---|---|---|---|---|---|---|---|---|
| `call-and-gap` | yes | 2 | 1 | 0 | work → work ✓ | other_work → other_work ✓ | 5.0 s | 4790 | — |
| `deep-work` | yes | 1 | 1 | 0 | work → work ✓ | supports_intention → supports_intention ✓ | 4.2 s | 4790 | — |
| `distraction-block` | yes | 2 | 1 | 0 | distraction → distraction ✓ | possible_distraction → possible_distraction ✓ | 5.6 s | 4826 | — |
| `focused-morning` | yes | 1 | 1 | 0 | work → work ✓ | other_work → supports_intention ✗ | 4.0 s | 4760 | — |
| `idle-heavy` | yes | 2 | 1 | 0 | idle → work ✗ | — → other_work | 4.5 s | 4643 | — |
| `meeting-audio` | yes | 1 | 1 | 0 | work → work ✓ | supports_intention → supports_intention ✓ | 4.3 s | 4606 | — |
| `research-spread` | yes | 1 | 1 | 0 | work → work ✓ | supports_intention → supports_intention ✓ | 6.0 s | 4849 | — |
| `split-attention` | yes | 1 | 1 | 0 | work → work ✓ | other_work → supports_intention ✗ | 5.1 s | 4781 | — |

## Summary

| metric | value |
|---|---|
| windows | 8 |
| coverage validity | 100.0 % (8/8) |
| error-card rate | 0.0 % (0/8) |
| attempts per window | 1.00 |
| repairs applied | 0 |
| cards written | 11 |
| category accuracy | 87.5 % (7/8) |
| relation accuracy | 71.4 % (5/7) |
| mean latency | 4.9 s |
| prompt / completion tokens | 34226 / 3819 |
| total tokens (mean per window) | 38045 (4755) |
| cost estimate | n/a (no price given) |

## Gates

| gate | measured | required | result |
|---|---|---|---|
| coverage validity | 100.0 % | ≥ 100 % | pass |
| category accuracy | 87.5 % | ≥ 80 % | pass |

All gates passed.
