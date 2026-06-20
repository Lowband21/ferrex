package com.ferrex.android.core.image

import com.ferrex.android.core.library.LibrarySyncFailure
import com.ferrex.android.core.library.LibrarySyncResult
import com.ferrex.android.core.library.ServerCacheScope
import com.ferrex.android.core.library.toFlatBufferUuid
import com.ferrex.android.core.library.toJavaUuidOrNull
import com.ferrex.android.core.library.toUuidString
import com.google.flatbuffers.FlatBufferBuilder
import ferrex.image.ImageManifestEntry
import ferrex.image.ImageManifestRequest
import ferrex.image.ImageManifestResponse
import ferrex.image.ImageStatus
import kotlinx.coroutines.test.runTest
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.rules.TemporaryFolder
import java.nio.ByteBuffer
import java.nio.ByteOrder
import java.util.UUID

@OptIn(ExperimentalUnsignedTypes::class)
class ImageRepositoryTest {
    @get:Rule
    val temporaryFolder = TemporaryFolder()

    @Test
    fun flatBuffersRoundTripPreservesCategoryStatusAndRetryTiming() {
        val poster = key(1, BrowseImageCategory.Poster)
        val backdrop = key(1, BrowseImageCategory.Backdrop)
        val profile = key(2, BrowseImageCategory.Profile)

        val request = ImageManifestRequest.getRootAsImageManifestRequest(
            ImageFlatBuffers.buildManifestRequest(listOf(backdrop, poster, profile)).asFlatBuffer(),
        )

        assertEquals(3, request.queriesLength)
        assertEquals(backdrop.category.flatBufferValue, request.queries(0)?.category)
        assertEquals(backdrop.iid, request.queries(0)?.iid?.toUuidString())
        assertEquals(poster.category.flatBufferValue, request.queries(1)?.category)
        assertEquals(profile.category.flatBufferValue, request.queries(2)?.category)

        val records = ImageFlatBuffers.parseManifestResponse(
            manifestResponse(
                ImageManifestRecord(poster, ManifestImageStatus.Ready("poster-token")),
                ImageManifestRecord(backdrop, ManifestImageStatus.Pending(2_500)),
                ImageManifestRecord(profile, ManifestImageStatus.Failed("profile missing")),
            ),
        )

        assertEquals(3, records.size)
        assertEquals(ManifestImageStatus.Ready("poster-token"), records[0].status)
        assertEquals(ManifestImageStatus.Pending(2_500), records[1].status)
        assertEquals(ManifestImageStatus.Failed("profile missing"), records[2].status)
        assertEquals(BrowseImageCategory.Backdrop, records[1].key.category)
    }

    @Test
    fun resolveMapsEveryCategoryToImmutableBlobUrlsAndPosterOnlyIidFallback() = runTest {
        val fixture = Fixture()
        val keys = BrowseImageCategory.entries.mapIndexed { index, category -> key(10 + index, category) }
        fixture.transport.result = LibrarySyncResult.Success(
            keys.mapIndexed { index, requestKey ->
                ImageManifestRecord(requestKey, ManifestImageStatus.Ready("token-$index"))
            },
        )

        val resolved = fixture.repository.resolveImages(fixture.scope, keys)

        assertEquals(keys, fixture.transport.requested.single())
        keys.forEachIndexed { index, requestKey ->
            val ready = resolved[requestKey] as ImageResolution.Ready
            assertEquals("http://ferrex.local/api/v1/images/blob/token-$index", ready.url)
            assertFalse(ready.stale)
        }
        assertNotNull(PosterOnlyIidFallback.url(fixture.scope.canonicalServerUrl, keys.first { it.category == BrowseImageCategory.Poster }))
        assertNull(PosterOnlyIidFallback.url(fixture.scope.canonicalServerUrl, keys.first { it.category == BrowseImageCategory.Backdrop }))
        assertNull(PosterOnlyIidFallback.url(fixture.scope.canonicalServerUrl, keys.first { it.category == BrowseImageCategory.Profile }))
        assertNull(PosterOnlyIidFallback.url(fixture.scope.canonicalServerUrl, keys.first { it.category == BrowseImageCategory.Episode }))
    }

    @Test
    fun pendingEntriesExposeExactRetryTimingWithoutBlockingInterceptor() = runTest {
        val fixture = Fixture(clockMillis = { 10_000L })
        val image = key(20, BrowseImageCategory.Poster)
        fixture.transport.result = LibrarySyncResult.Success(
            listOf(ImageManifestRecord(image, ManifestImageStatus.Pending(2_750))),
        )

        val pending = fixture.repository.resolveImages(fixture.scope, listOf(image))[image] as ImageResolution.Pending

        assertEquals(2_750, pending.retryAfterMillis)
        assertEquals(12_750, pending.retryAtMillis)
        assertEquals("pending", pending.label)
    }

    @Test
    fun failedMissingAndInvalidImagesProduceDeterministicStates() = runTest {
        val fixture = Fixture()
        val failed = key(30, BrowseImageCategory.Profile)
        val invalid = ImageRequestKey("not-a-uuid", BrowseImageCategory.Poster)
        fixture.transport.result = LibrarySyncResult.Success(
            listOf(ImageManifestRecord(failed, ManifestImageStatus.Failed("profile missing"))),
        )

        val resolved = fixture.repository.resolveImages(fixture.scope, listOf(failed, invalid))

        val failedState = resolved[failed] as ImageResolution.Failed
        assertEquals("profile missing", failedState.reason)
        assertEquals("failed", failedState.label)
        val placeholder = resolved[invalid] as ImageResolution.Placeholder
        assertEquals("placeholder", placeholder.label)
        assertTrue(placeholder.reason.contains("valid UUID"))
    }

    @Test
    fun offlineFailureUsesStaleManifestWithoutBlockingGridState() = runTest {
        val fixture = Fixture()
        val image = key(40, BrowseImageCategory.Backdrop)
        fixture.cache.writeManifestEntry(fixture.scope, ImageManifestRecord(image, ManifestImageStatus.Ready("cached-token")))
        fixture.transport.result = LibrarySyncResult.Failure(LibrarySyncFailure.Network("offline"))

        val ready = fixture.repository.resolveImages(fixture.scope, listOf(image))[image] as ImageResolution.Ready

        assertTrue(ready.stale)
        assertEquals("stale-offline-ready", ready.label)
        assertEquals("offline", ready.offlineMessage)
        assertEquals("http://ferrex.local/api/v1/images/blob/cached-token", ready.url)
    }

    @Test
    fun retryRefreshesOnlyVisiblePendingAndFailedImagesAndKeepsReadyResolutions() = runTest {
        val fixture = Fixture()
        val ready = key(50, BrowseImageCategory.Poster)
        val pending = key(51, BrowseImageCategory.Poster)
        val failed = key(52, BrowseImageCategory.Episode)
        fixture.cache.writeManifestEntry(fixture.scope, ImageManifestRecord(ready, ManifestImageStatus.Ready("ready-token")))
        fixture.cache.writeManifestEntry(fixture.scope, ImageManifestRecord(pending, ManifestImageStatus.Pending(1_000)))
        fixture.cache.writeManifestEntry(fixture.scope, ImageManifestRecord(failed, ManifestImageStatus.Failed("missing")))
        fixture.transport.result = LibrarySyncResult.Success(
            listOf(
                ImageManifestRecord(pending, ManifestImageStatus.Ready("pending-token")),
                ImageManifestRecord(failed, ManifestImageStatus.Pending(3_000)),
            ),
        )

        val resolved = fixture.repository.retryPendingOrFailed(fixture.scope, listOf(ready, pending, failed))

        assertEquals(listOf(pending, failed), fixture.transport.requested.single())
        assertEquals("ready-token", (resolved[ready] as ImageResolution.Ready).token)
        assertEquals("pending-token", (resolved[pending] as ImageResolution.Ready).token)
        assertEquals(3_000, (resolved[failed] as ImageResolution.Pending).retryAfterMillis)
    }

    @Test
    fun retryFetchesMissingVisibleImagesAfterManifestTransportFailure() = runTest {
        val fixture = Fixture()
        val image = key(53, BrowseImageCategory.Backdrop)
        fixture.transport.result = LibrarySyncResult.Failure(LibrarySyncFailure.Network("offline"))

        val failed = fixture.repository.resolveImages(fixture.scope, listOf(image))[image] as ImageResolution.Failed

        assertTrue(failed.retryable)
        assertTrue(fixture.cache.readManifestEntry(fixture.scope, image) is ManifestCacheRead.Missing)
        fixture.transport.result = LibrarySyncResult.Success(
            listOf(ImageManifestRecord(image, ManifestImageStatus.Ready("recovered-token"))),
        )

        val retried = fixture.repository.retryPendingOrFailed(fixture.scope, listOf(image))

        assertEquals(listOf(listOf(image), listOf(image)), fixture.transport.requested)
        assertEquals("recovered-token", (retried[image] as ImageResolution.Ready).token)
    }

    @Test
    fun retryQuarantinesCorruptMetadataAndRefreshesReadyManifest() = runTest {
        val fixture = Fixture()
        val image = key(54, BrowseImageCategory.Poster)
        val manifestFile = fixture.cache.debugManifestFile(fixture.scope, image)
        manifestFile.parentFile?.mkdirs()
        manifestFile.writeText("iid=${image.iid}\ncategory=poster\nstatus=ready\n")
        fixture.transport.result = LibrarySyncResult.Success(
            listOf(ImageManifestRecord(image, ManifestImageStatus.Ready("recovered-token"))),
        )

        val retried = fixture.repository.retryPendingOrFailed(fixture.scope, listOf(image))

        assertEquals(listOf(listOf(image)), fixture.transport.requested)
        assertEquals("recovered-token", (retried[image] as ImageResolution.Ready).token)
        assertTrue(fixture.cache.quarantinedManifestFiles(fixture.scope).isNotEmpty())
        val healed = fixture.cache.readManifestEntry(fixture.scope, image) as ManifestCacheRead.Valid
        assertEquals(ManifestImageStatus.Ready("recovered-token"), healed.record.status)
    }

    @Test
    fun diagnosticsCountImageManifestStatusesStaleQuarantineAndRetryBatch() = runTest {
        val fixture = Fixture()
        val ready = key(55, BrowseImageCategory.Poster)
        val pending = key(56, BrowseImageCategory.Backdrop)
        val failed = key(57, BrowseImageCategory.Episode)
        val quarantined = key(58, BrowseImageCategory.Profile)
        val corrupt = key(59, BrowseImageCategory.Poster)
        fixture.cache.writeManifestEntry(fixture.scope, ImageManifestRecord(ready, ManifestImageStatus.Ready("ready-token")))
        fixture.cache.writeManifestEntry(fixture.scope, ImageManifestRecord(pending, ManifestImageStatus.Pending(1_000)))
        fixture.cache.writeManifestEntry(fixture.scope, ImageManifestRecord(failed, ManifestImageStatus.Failed("not available")))
        fixture.cache.debugManifestFile(fixture.scope, quarantined).apply {
            parentFile?.mkdirs()
            writeText("iid=${quarantined.iid}\ncategory=profile\nstatus=ready\n")
        }
        assertTrue(fixture.cache.readManifestEntry(fixture.scope, quarantined) is ManifestCacheRead.Corrupt)
        fixture.cache.debugManifestFile(fixture.scope, corrupt).apply {
            parentFile?.mkdirs()
            writeText("iid=${corrupt.iid}\ncategory=poster\nstatus=ready\n")
        }
        fixture.transport.result = LibrarySyncResult.Failure(LibrarySyncFailure.Network("offline"))

        fixture.repository.resolveImages(fixture.scope, listOf(ready))
        val stale = fixture.cache.diagnosticSnapshot(fixture.scope)

        assertTrue(stale.staleOfflineMarkerPresent)
        assertEquals(1, stale.manifestStatus.readyCount)
        assertEquals(1, stale.manifestStatus.pendingCount)
        assertEquals(1, stale.manifestStatus.failedCount)
        assertEquals(3, stale.manifestStatus.staleCount)
        assertEquals(1, stale.manifestStatus.corruptCount)
        assertEquals(1, stale.quarantineFileCount)
        assertEquals(1, stale.quarantineReasonFileCount)
        assertEquals("failure", stale.lastManifestBatch?.lastOutcome)
        assertEquals("Network", stale.lastManifestBatch?.lastFailureKind)

        fixture.transport.result = LibrarySyncResult.Success(
            listOf(ImageManifestRecord(pending, ManifestImageStatus.Ready("pending-healed"))),
        )
        fixture.repository.retryPendingOrFailed(fixture.scope, listOf(ready, pending))
        val healed = fixture.cache.diagnosticSnapshot(fixture.scope)

        assertFalse(healed.staleOfflineMarkerPresent)
        assertEquals(2, healed.manifestStatus.readyCount)
        assertEquals(0, healed.manifestStatus.pendingCount)
        assertEquals("success", healed.lastManifestBatch?.lastOutcome)
        assertEquals("retry", healed.lastManifestBatch?.lastKind)
        assertNotNull(healed.lastManifestBatch?.lastRetryEpochMs)
        assertEquals(1, healed.lastManifestBatch?.lastRequestedKeyCount)
        assertEquals(1, healed.lastManifestBatch?.lastReadyCount)
    }

    @Test
    fun selectedAndAllImageCacheClearRemoveMetadataAndCoilBlobs() {
        val fixture = Fixture()
        val first = key(60, BrowseImageCategory.Poster)
        val second = key(61, BrowseImageCategory.Backdrop)
        fixture.cache.writeManifestEntry(fixture.scope, ImageManifestRecord(first, ManifestImageStatus.Ready("first")))
        fixture.cache.writeManifestEntry(fixture.scope, ImageManifestRecord(second, ManifestImageStatus.Ready("second")))
        val coilBlob = fixture.cache.coilDiskCacheDir(fixture.scope).resolve("opaque-coil-entry")
        coilBlob.parentFile?.mkdirs()
        coilBlob.writeText("blob")

        fixture.cache.clearManifestEntries(fixture.scope, listOf(first))

        assertTrue(fixture.cache.readManifestEntry(fixture.scope, first) is ManifestCacheRead.Missing)
        assertTrue(fixture.cache.readManifestEntry(fixture.scope, second) is ManifestCacheRead.Valid)
        assertFalse(coilBlob.exists())

        fixture.cache.clearAll(fixture.scope)

        assertTrue(fixture.cache.readManifestEntry(fixture.scope, second) is ManifestCacheRead.Missing)
    }

    @Test
    fun corruptManifestCacheIsQuarantinedAndReportedAsRetryable() = runTest {
        val fixture = Fixture()
        val image = key(70, BrowseImageCategory.Poster)
        val manifestFile = fixture.cache.debugManifestFile(fixture.scope, image)
        manifestFile.parentFile?.mkdirs()
        manifestFile.writeText("iid=${image.iid}\ncategory=poster\nstatus=ready\n")
        fixture.transport.result = LibrarySyncResult.Failure(LibrarySyncFailure.Network("offline"))

        val failed = fixture.repository.resolveImages(fixture.scope, listOf(image))[image] as ImageResolution.Failed

        assertTrue(failed.reason.contains("Corrupt image manifest cache"))
        assertTrue(failed.retryable)
        assertTrue(fixture.cache.quarantinedManifestFiles(fixture.scope).isNotEmpty())
        assertFalse(manifestFile.exists())
    }

    @Test
    fun tmdbFallbackRequiresPublicPathAndAllowedProductCopy() {
        assertNull(TmdbImageFallbackPolicy.publicCdnUrl(null, BrowseImageCategory.Poster, productCopyAllowsPublicCdn = true))
        assertNull(TmdbImageFallbackPolicy.publicCdnUrl("/poster.jpg", BrowseImageCategory.Poster, productCopyAllowsPublicCdn = false))
        assertEquals(
            "https://image.tmdb.org/t/p/w342/poster.jpg",
            TmdbImageFallbackPolicy.publicCdnUrl("/poster.jpg", BrowseImageCategory.Poster, productCopyAllowsPublicCdn = true),
        )
    }

    private inner class Fixture(
        clockMillis: () -> Long = { 100L },
    ) {
        val scope = ServerCacheScope.from("http://ferrex.local", "user-1")
        val cache = ImageDiskCache(temporaryFolder.newFolder("image-cache-${System.nanoTime()}"))
        val transport = FakeImageManifestTransport()
        val repository = ImageRepository(
            transport = transport,
            cache = cache,
            clockMillis = clockMillis,
        )
    }

    private class FakeImageManifestTransport : ImageManifestTransport {
        val requested = mutableListOf<List<ImageRequestKey>>()
        var result: LibrarySyncResult<List<ImageManifestRecord>> = LibrarySyncResult.Success(emptyList())

        override suspend fun fetchManifest(keys: Collection<ImageRequestKey>): LibrarySyncResult<List<ImageManifestRecord>> {
            requested += keys.toList()
            return result
        }
    }

    private fun key(seed: Int, category: BrowseImageCategory): ImageRequestKey =
        ImageRequestKey(UUID(0L, seed.toLong()).toString(), category)

    private fun manifestResponse(vararg records: ImageManifestRecord): ByteArray {
        val builder = FlatBufferBuilder(128 + records.size * 48)
        val offsets = records.map { record ->
            val token = (record.status as? ManifestImageStatus.Ready)?.token?.let(builder::createString)
            val failureReason = (record.status as? ManifestImageStatus.Failed)?.reason?.let(builder::createString)
            ImageManifestEntry.startImageManifestEntry(builder)
            ImageManifestEntry.addStatus(
                builder,
                when (record.status) {
                    is ManifestImageStatus.Ready -> ImageStatus.Ready
                    is ManifestImageStatus.Pending -> ImageStatus.Pending
                    is ManifestImageStatus.Failed -> ImageStatus.Failed
                },
            )
            token?.let { ImageManifestEntry.addToken(builder, it) }
            ImageManifestEntry.addCategory(builder, record.key.category.flatBufferValue)
            if (record.status is ManifestImageStatus.Pending) {
                ImageManifestEntry.addRetryAfterMillis(builder, record.status.retryAfterMillis.toULong())
            }
            failureReason?.let { ImageManifestEntry.addFailureReason(builder, it) }
            val uuid = record.key.iid.toJavaUuidOrNull() ?: error("invalid test uuid")
            ImageManifestEntry.addIid(builder, uuid.toFlatBufferUuid(builder))
            ImageManifestEntry.endImageManifestEntry(builder)
        }.toIntArray()
        val entries = ImageManifestResponse.createEntriesVector(builder, offsets)
        val root = ImageManifestResponse.createImageManifestResponse(builder, entries)
        builder.finish(root)
        return builder.sizedByteArray()
    }

    private fun ByteArray.asFlatBuffer(): ByteBuffer = ByteBuffer.wrap(this).order(ByteOrder.LITTLE_ENDIAN)
}
