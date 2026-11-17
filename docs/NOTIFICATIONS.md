# Input-needed notifications

Settings → Your workspace → **Notify me when input is needed** enables optional Mac alerts. It is off by default on a new installation; the local preference survives a restart. **Send a test notification** requests a generic test alert.

The owner/friend procedure is [the 2.0.5 launch checklist](FRIEND_LAUNCH_CHECK.md). A fresh friend account can test notifications without an API key or inference. The test button sends immediately: switch to Finder right away and, if the banner disappears, check Notification Center. Record banner delivery, Notification Center presence and click-to-inbox separately. On November 9 the owner reported **“Notification Center only; clicking opens Decision inbox.”** This establishes user-observed OS delivery and click-through for 2.0.5 on the existing profile. A desktop banner was not observed; no cause is inferred. The fresh-profile check remains separate.

The global Decision inbox and its sidebar count cover plan approvals, policy Asks, findings needing disposition, escalations, code-diff review, coordinator proposals, external-write proposals and failed steps. Inbox rows open the owning project and step. A failed step is a request to inspect the failure, not permission to retry or spend.

The Rust shell checks the same inbox every three seconds, skipping a busy application lock. The open inbox refreshes every five seconds. The UI count stays unknown until the first snapshot. Unreadable projects are visible as errors rather than a clear board. A hidden sidebar has a review-input banner so the count is still reachable.

Mac notifications group new items, use the highest risk tier among those new items, suppress duplicates, and rate-limit requests to once per 30 seconds. Items already visible while the app is in the foreground do not also trigger a system alert. Changed approval scope changes item identity; resolved items can notify again if they reappear. A rejected notification request remains eligible after the cooldown. The tracker resets on process restart; notification history is not an approval ledger.

System notifications contain only a generic count and risk class, never project names, source text, model output, action destinations or keys. Clicking one restores/focuses the window and asks the UI to open the Decision inbox. It does not dispatch, approve, reject or send anything. Review and action approvals remain separate, hash-bound operations.

Keep SproutCrew open, hidden or minimized. Quit ends the watcher. macOS permissions, Focus and screen-sharing settings control actual delivery. The desktop notification plugin's permission API reports Granted without checking macOS, so this app deliberately does not label that response as proof of OS authorization or delivery. A successful send means **requested**, not **delivered**.

## Verification

- `foreground_is_quiet_background_groups_new_decisions_and_never_repeats`: foreground suppression, background grouping, risk priority, generic content, repeat suppression, resolution/reappearance.
- `cooldown_retains_unseen_work_and_failures_can_retry_without_a_storm`: failed-request cooldown and changed scope.
- The failed-run report/inbox fixture verifies a failure deep-links to its exact stage and disappears from pending input after a later successful fixture task, while report history remains intact.
- Native 2.0.2 showed four pending items, including the earlier 429, and the failure row returned to `outline`. Enabled the preference and requested a test through the native command. macOS Settings showed SproutCrew alerts on with Desktop and Notification Center selected. Global notifications while mirroring/sharing were off, and the CUA window-sharing indicator was active. A visible banner and a real notification click were **not verified** in this session; that system setting was preserved.
- [Native preferences evidence](launch-evidence/notification-settings-2026-11-08.jpg).

References: [Tauri notification plugin](https://v2.tauri.app/plugin/notification/); installed Rust plugin `tauri-plugin-notification` 2.8.0 desktop implementation. The new dependency's missing crate notice was supplied from its exact upstream VCS commit; `scripts/notices.py` passes with 341 crates and 6 production UI packages.

The watcher reuses the existing cross-project projection rather than adding a second index. It scans project history under a nonblocking application lock; if large project collections make this expensive, an incremental inbox projection is the next measured optimization.
