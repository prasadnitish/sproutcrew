# Security review of the November 10 beta fixes

Branch `beta-fixes-nov10`, changes from `7aab7d4`. A fresh reviewer read every change on November 10, 2026, without network access and without touching credentials. No high-severity issue was found. Each finding was checked against the code before it was fixed.

## Fixed

| Finding | Severity | Fix |
| --- | --- | --- |
| A search result could make the model write its own "Pages the search used" heading, and the app fetched the first such list automatically, so an attacker could choose up to five addresses the Mac would request, with the brief in the address. | Medium | Nothing is fetched automatically any more. The step's result lists the pages, and the person chooses "Add as sources". The app reads only its own section, which is always written last, even when empty. A test covers a forged heading. |
| Fetched pages changed `sources/` during a run, which correctly broke the plan approval but stopped the run with a generic error. | Medium | Removed with the automatic fetch. Adding the pages is the person's act and the screen says the plan is approved again. |
| `send_write` checked the policy, released the app, then sent; a guardrail change could land in between. | Low | The send counts as work in progress before the app is released, so guardrails cannot change until it ends. |
| The cited-page fetch ignored Stop and did not count as running work. | Low | Removed with the automatic fetch. |
| A tampered model-list file could mark a paid model free in the picker and the estimate. | Low | "Free" and unknown prices are worked out again when the file is read. A test covers a forged file. |
| `approve_write` recorded every approval as "Nitish". | Low, older | The UI passes the approver, as the plan approval does; the default is "Local user". |

## Open

- OpenRouter's search fee (about $0.007 a search, Exa engine) is not inside the request's reserved worst case, and an OpenRouter route in token mode has no dollar bound at all. The fee shows in the estimate and in the reported cost, but nothing caps it. This belongs with the decision on a dollar cap for paid OpenRouter models (`docs/issues.md` item 99).

## Checked and sound

Approval binding (plan, policy, sources, roles, budget and route settings are all still in the hash, and `set_guardrail` goes through the policy change that needs a new approval); guardrail changes never touch checking roles and always pass validation; retries hold and release their own reservations, stop on request, and never retry a daily cap or empty account; web search is refused on free models in the request and in plan validation; page fetches go through the public-web checks (https only, public addresses, rebinding protection, redirect checks, size and time limits); kept pages are ordinary untrusted sources; the web step still sees only the brief; the local log holds fixed kinds and scrubbed lines only and its exports use absolute paths with an empty environment; the OpenRouter key goes only to openrouter.ai over HTTPS; no new broad capability was added.
