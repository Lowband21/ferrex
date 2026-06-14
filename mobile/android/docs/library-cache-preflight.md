# Android library cache preflight (LOW-344)

LOW-344 depends on the LOW-342 recovery/auth substrate. This workspace is based on `fix(android): add recovery-first auth substrate (#55)` and the recovery exits required for protected screens are present before enabling the repository cache:

- Mobile auth/recovery/home screens expose Retry, Sign out, Change server, and Reset connection in `app/src/main/kotlin/com/ferrex/android/ui/recovery/PhoneRecoveryScreens.kt`.
- TV auth/recovery/home screens expose D-pad reachable Retry, Sign out, Change server, and Reset connection in `app/src/tv/kotlin/com/ferrex/android/tv/ui/TvRecoveryScreens.kt`.
- `AuthManager.resetConnection()` now invokes the server/user-scoped cache clear hook before clearing saved connection data, so reset remains a data-wipe-free recovery exit for corrupt or mismatched library cache state.

If any of those exits disappear, LOW-344 cache work should be deferred rather than shipping protected screens that can trap users behind corrupt cache or auth state.

LOW-345 extends this preflight for image manifest metadata and Coil blobs in `image-pipeline-preflight.md`.
