# Reporting a security problem

Write to hello@sproutcrew.app with what you found and how to reproduce it. Please do not open a public issue for a security problem, and do not include real API keys or private documents in a report.

You will get a reply from the developer, Nitish Prasad. SproutCrew is built by one person, so there is no fixed response time, but security reports come first.

## What matters most

- A way for text from a model, a file or a web page to change a permission, start an action, or reach a command line.
- A way to send anything outside the Mac, or delete anything, without the person approving that exact action.
- A way for an API key to reach a log, an export, a notification or a host it was not saved for.
- A way to spend past a limit the person set.

## How the app is reviewed

Before each release a reviewer with no network access and no credentials reads every change since the last one. The most recent review is in `docs/SECURITY_AUDIT_2026-11.md`, and the fixes that followed are in the [changelog](https://www.sproutcrew.app/changelog).
