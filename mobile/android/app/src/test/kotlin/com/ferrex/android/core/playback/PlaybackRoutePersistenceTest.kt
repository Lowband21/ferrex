package com.ferrex.android.core.playback

import com.ferrex.android.core.browse.BrowseMediaType
import com.ferrex.android.core.browse.BrowseSourceSurface
import com.ferrex.android.core.browse.MediaRouteArgs
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class PlaybackRoutePersistenceTest {
    @Test
    fun playbackRouteCodecRoundTripsDelimiterHeavyRoutes() {
        val route = PlaybackRouteContract(
            targetMediaId = "target|media",
            logicalMediaId = "logical/media",
            mediaType = BrowseMediaType.Episode,
            startPositionSeconds = 123.25,
            startOver = false,
            sourceDetailRoute = "media/episode/logical%2Fmedia?source=library_grid&libraryId=lib|one",
        )

        val encoded = PlaybackRoutePersistence.encode(route)

        assertEquals(route, PlaybackRoutePersistence.decode(encoded))
    }

    @Test
    fun mediaRouteCodecAndDetailRouteStringRestoreBackTarget() {
        val route = MediaRouteArgs(
            mediaType = BrowseMediaType.Movie,
            mediaId = "movie-id",
            libraryId = "library-id",
            sourceSurface = BrowseSourceSurface.LibraryGrid,
        )

        assertEquals(route, MediaRoutePersistence.decode(MediaRoutePersistence.encode(route)))
        assertEquals(route, MediaRoutePersistence.decodeRouteString(route.toRouteString()))
    }

    @Test
    fun routeCodecsRejectMalformedOrUnknownPayloads() {
        assertNull(PlaybackRoutePersistence.decode("not-a-route"))
        assertNull(MediaRoutePersistence.decode("not-a-route"))
        assertNull(MediaRoutePersistence.decodeRouteString("media/unknown/item?source=library_grid"))
        assertNull(MediaRoutePersistence.decodeRouteString("media/movie/item?source=missing_surface"))
    }

    @Test
    fun authenticatedPersistenceScopeChangesAcrossServerAndUser() {
        val first = PlaybackRoutePersistence.scopeKey("https://ferrex.example", "user-a")

        assertEquals(first, PlaybackRoutePersistence.scopeKey("https://ferrex.example", "user-a"))
        assertFalse(first == PlaybackRoutePersistence.scopeKey("https://ferrex.example", "user-b"))
        assertFalse(first == PlaybackRoutePersistence.scopeKey("https://other.example", "user-a"))
    }

    @Test
    fun windowPolicyLocksOnlyPhoneOrientationAndKeepsTvControlFree() {
        val lockedPhone = PlaybackWindowPolicy.forPlayback(
            surface = PlaybackSurfaceKind.Phone,
            phoneOrientationLocked = true,
        )
        val unlockedPhone = PlaybackWindowPolicy.forPlayback(
            surface = PlaybackSurfaceKind.Phone,
            phoneOrientationLocked = false,
        )
        val tv = PlaybackWindowPolicy.forPlayback(
            surface = PlaybackSurfaceKind.Tv,
            phoneOrientationLocked = true,
        )

        assertTrue(lockedPhone.immersiveFullscreen)
        assertTrue(lockedPhone.transientSystemBarsBySwipe)
        assertTrue(lockedPhone.showsOrientationLockControl)
        assertEquals(PlaybackOrientationRequest.LockedLandscape, lockedPhone.orientationRequest)
        assertEquals(PlaybackOrientationRequest.UserControlled, unlockedPhone.orientationRequest)
        assertTrue(unlockedPhone.showsOrientationLockControl)
        assertTrue(tv.immersiveFullscreen)
        assertTrue(tv.transientSystemBarsBySwipe)
        assertFalse(tv.showsOrientationLockControl)
        assertEquals(PlaybackOrientationRequest.None, tv.orientationRequest)
    }
}
