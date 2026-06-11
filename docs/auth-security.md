# Authentication Security Model

Ferrex uses password-authenticated full sessions for account and admin work, and device-bound PIN sessions for playback convenience. The server never stores raw device PINs; proof-based device PIN setup/change routes and the device PIN login route accept only client-derived proof material. The current desktop player PIN-login compatibility path still submits the entered secret through the username/password login flow, so the no-raw-PIN guarantee applies to the proof-based device PIN routes rather than that legacy login path.

## Password login and full scope

- Username/password login creates a full-scope session plus a refresh token.
- Admin-only routes require full scope. Playback/PIN sessions are rejected by admin middleware even when the user has an admin role.
- The default security setting `device_trust_policy.admin_pin_unlock_enabled` is `false`. Attempts to enable it through the settings API are rejected; if this ever changes it must be an explicit persisted setting and UI label.

## Remember device, auto-login, and trust duration

- `remember_device` is a per-device choice. If a device-login request omits it, the persisted `device_trust_policy.remember_device_default` supplies the default.
- Remembered devices must register a device public key. Proof-route PIN login then requires both the client-derived PIN proof and a fresh device-possession signature.
- Device trust uses the `auth_device_sessions.trusted_until` timestamp. Password device login, PIN login, and refresh extend `trusted_until` by `device_trust_policy.trust_duration_days`; device listing and trust-status endpoints report that timestamp.
- Legacy device rows without `trusted_until` fall back to the same configured last-activity window for eligibility.
- Auto-login is local client state plus the user preference; disabling/revoking clears the local/device trust path and requires re-authentication.

## PIN policy and proof-route design

- Official clients enforce the persisted PIN policy before accepting PIN setup or PIN-login input: numeric PINs, length 4-8 by default, no simple sequential values, and no runs longer than two identical digits by default.
- The proof-based device routes (`/auth/device/pin/set`, `/auth/device/pin/change`, and `/auth/device/pin`) use client-derived PIN proof material plus a fresh device-possession challenge. On those routes the server cannot re-check raw PIN length or patterns after derivation, so server-side enforcement is limited to non-empty proof material, registered device ownership, `trusted_until`, max-attempt lockout, and global rate limits.
- The desktop player still has a legacy PIN-login compatibility path that uses username/password login with the entered secret. Until it is migrated to `/auth/device/pin`, do not describe that path as no-raw-PIN.
- Security settings and device-auth responses return `pin_policy` and `device_trust_policy` so clients can display labels and validate raw input consistently.

## Profile listing privacy

- Public/unauthenticated profile discovery should not reveal the full user directory. Known-device profile listing uses the presented device fingerprint and returns lightweight user cards only for a recognized device.
- Official clients cache lightweight user summaries for offline recovery hints, but clear those caches when setup status indicates a different/fresh server or an authorization error.

## Lockout, revoke, and recovery

- PIN failures increment the device session counter. At `device_trust_policy.pin_max_attempts` failures, the device is locked until `pin_lockout_minutes` elapses.
- Revoking a device clears trust, disables auto-login for that device, and revokes sessions/refresh tokens bound to the device.
- Password changes revoke existing sessions and device trust. Recovery from forgotten PIN or revoked/expired trust is full password login followed by setting a new PIN on the registered device.
- If local client auth state becomes stale or unrecoverable, clearing cached auth/device state is safe: server-side password login remains the recovery path.
