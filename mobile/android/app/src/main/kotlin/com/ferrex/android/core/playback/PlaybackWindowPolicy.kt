package com.ferrex.android.core.playback

enum class PlaybackSurfaceKind {
    Phone,
    Tv,
}

enum class PlaybackOrientationRequest {
    None,
    LockedLandscape,
    UserControlled,
}

data class PlaybackWindowPolicyDecision(
    val immersiveFullscreen: Boolean,
    val transientSystemBarsBySwipe: Boolean,
    val orientationRequest: PlaybackOrientationRequest,
    val showsOrientationLockControl: Boolean,
)

object PlaybackWindowPolicy {
    fun forPlayback(
        surface: PlaybackSurfaceKind,
        phoneOrientationLocked: Boolean,
    ): PlaybackWindowPolicyDecision = when (surface) {
        PlaybackSurfaceKind.Phone -> PlaybackWindowPolicyDecision(
            immersiveFullscreen = true,
            transientSystemBarsBySwipe = true,
            orientationRequest = if (phoneOrientationLocked) {
                PlaybackOrientationRequest.LockedLandscape
            } else {
                PlaybackOrientationRequest.UserControlled
            },
            showsOrientationLockControl = true,
        )
        PlaybackSurfaceKind.Tv -> PlaybackWindowPolicyDecision(
            immersiveFullscreen = true,
            transientSystemBarsBySwipe = true,
            orientationRequest = PlaybackOrientationRequest.None,
            showsOrientationLockControl = false,
        )
    }
}
