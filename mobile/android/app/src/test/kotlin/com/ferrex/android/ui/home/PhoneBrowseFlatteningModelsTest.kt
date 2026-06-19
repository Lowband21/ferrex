package com.ferrex.android.ui.home

import com.ferrex.android.core.library.LibraryFreshness
import com.ferrex.android.core.library.RetryClassification
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class PhoneBrowseFlatteningModelsTest {
    @Test
    fun homeSummaryLineUsesCompactSignedInConnectionAndServerCopy() {
        val line = phoneHomeSignedInLine(
            displayName = "Jules",
            username = "jules",
            connectionTitle = "Online",
            serverUrl = "https://ferrex.local",
        )

        assertEquals("Signed in as Jules • Online • https://ferrex.local", line)
        assertFalse(line.contains("Ferrex Mobile"))
        assertFalse(line.contains("Media-first phone shell"))
    }

    @Test
    fun onlineFreshAndPassiveCacheStatesDoNotRenderHomeStatusNotices() {
        assertFalse(phoneHomeShouldShowStatusNotices(connectionVisible = false, freshness = LibraryFreshness.Empty))
        assertFalse(phoneHomeShouldShowStatusNotices(connectionVisible = false, freshness = LibraryFreshness.Syncing))
        assertFalse(
            phoneHomeShouldShowStatusNotices(
                connectionVisible = false,
                freshness = LibraryFreshness.Fresh(itemCount = 12, syncedAtMillis = 1L),
            ),
        )
    }

    @Test
    fun recoveryStatesStillRenderHomeStatusNotices() {
        assertTrue(
            phoneHomeShouldShowStatusNotices(
                connectionVisible = true,
                freshness = LibraryFreshness.Fresh(itemCount = 12, syncedAtMillis = 1L),
            ),
        )
        assertTrue(
            phoneHomeShouldShowStatusNotices(
                connectionVisible = false,
                freshness = LibraryFreshness.StaleOffline("offline", itemCount = 4, lastSyncedAtMillis = 1L),
            ),
        )
        assertTrue(
            phoneHomeShouldShowStatusNotices(
                connectionVisible = false,
                freshness = LibraryFreshness.CorruptRebuilding("quarantined stale payload", quarantinedFiles = 1),
            ),
        )
        assertTrue(
            phoneHomeShouldShowStatusNotices(
                connectionVisible = false,
                freshness = LibraryFreshness.ErrorRetryable("server unavailable", RetryClassification.Retryable),
            ),
        )
    }
}
