# Android image pipeline preflight (LOW-345)

LOW-345 is layered on the LOW-344 library cache and LOW-342 recovery substrate:

- `ServerCacheScope` scopes image manifest metadata and Coil disk blobs by canonical server URL and authenticated user id, matching the library cache scope.
- Image metadata/blobs live under `library_cache/v1/scopes/<scope>/images`, so Reset connection and Clear all server cache can remove corrupt image state without requiring an OS app-data wipe.
- Clear selected cache computes image keys from the selected cached library accessor and asks the image cache clearer to remove those manifest records; because Coil disk keys are internal, selected clear conservatively drops server-scoped Coil blob files as a recovery action.
- Mobile and TV recovery screens still expose Retry, Sign out, Change server, and Reset connection before protected screens are shown.

Current server limitation: `/api/v1/images/iid/{iid}` resolves `ImageSize::poster()` only. Android browse/detail image loading therefore uses `/api/v1/images/manifest` for poster, backdrop, profile, and episode still categories, and loads only immutable `/api/v1/images/blob/{token}` URLs when manifest entries are Ready. Any IID URL fallback is documented and guarded as poster-only.
