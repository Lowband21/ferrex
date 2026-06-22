---
title: "QA and evidence"
description: "Dated Ferrex QA packets, Android/TV evidence matrices, playback/auth regression notes, and manual hardware follow-up status."
sidebar:
  order: 1
  label: "Overview"
---

QA reference pages preserve evidence exactly enough for reviewers to understand what passed, what was blocked, and what must be rerun before release decisions.

## Evidence status labels

- **Pass** rows are historical evidence from the recorded run date and command output.
- **Blocked**, **pending hardware QA**, and **release-readiness follow-up** rows are intentionally stale until rerun on current phone/TV/desktop hardware.
- Android and Android TV pages prioritize no-wipe recovery paths because data-wipe-class recovery failures are release blockers.

## Android, TV, playback, and visual evidence

| Page | Coverage |
| --- | --- |
| [Playback auth regression QA](/reference/qa/playback-auth-regression/) | Server stream tickets, desktop GStreamer/MPV smoke, Android/TV auth-expiry units, and manual playback follow-up. |
| [Android and Android TV final QA acceptance packet](/reference/qa/android-final-qa/) | Diagnostics, recovery, playback/watch-state, TV focus, codegen status, and redaction requirements. |
| [Android library cache preflight](/reference/qa/android-library-cache-preflight/) | Recovery-first library cache substrate and reset expectations. |
| [Android image pipeline preflight](/reference/qa/android-image-pipeline-preflight/) | Large-library artwork recovery, stale-ready behavior, selected-cache clear, and reset matrix. |
| [Android playback QA matrix](/reference/qa/android-playback-matrix/) | Ticketed Media3 playback, watch progress, retry/recovery, and track selection matrix. |
| [Android TV 10-foot QA matrix](/reference/qa/android-tv-10ft-matrix/) | D-pad, Back, focus restore, dense grid, recovery exits, and hardware runbook. |
| [Android visual QA and accessibility runbook](/reference/qa/android-visual-a11y-qa/) | Contrast/tag unit tests, screenshot runner, accessibility dumps, manual phone/TV paths, and redaction rules. |
