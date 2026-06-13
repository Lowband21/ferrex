package com.ferrex.android.core.library

import com.ferrex.android.core.image.BrowseImageCategory
import com.ferrex.android.core.image.ImageRequestKey
import com.google.flatbuffers.FlatBufferBuilder
import ferrex.library.BatchFetchRequest
import ferrex.library.BatchFetchResponse
import ferrex.library.BatchSyncRequest
import ferrex.library.BatchSyncResponse
import ferrex.library.BatchVersion
import ferrex.library.LibraryList
import ferrex.library.MediaBatchData
import ferrex.library.SeriesBundleData
import ferrex.library.SeriesBundleFetchRequest
import ferrex.library.SeriesBundleFetchResponse
import ferrex.library.SeriesBundleSyncRequest
import ferrex.library.SeriesBundleSyncResponse
import ferrex.library.SeriesBundleVersion
import ferrex.media.EpisodeReference
import ferrex.media.MediaVariant
import ferrex.media.MovieReference
import ferrex.media.SeriesReference
import java.nio.ByteBuffer
import java.nio.ByteOrder
import java.util.UUID

@OptIn(ExperimentalUnsignedTypes::class)
object LibraryFlatBuffers {
    fun buildBatchSyncRequest(cachedVersions: Map<Int, Long>): ByteArray {
        val builder = FlatBufferBuilder(64 + cachedVersions.size * 16)
        val versionOffsets = cachedVersions.toSortedMap().map { (batchId, version) ->
            BatchVersion.createBatchVersion(builder, batchId.toUInt(), version.toULong())
        }.toIntArray()
        val versions = BatchSyncRequest.createCachedVersionsVector(builder, versionOffsets)
        val root = BatchSyncRequest.createBatchSyncRequest(builder, versions)
        builder.finish(root)
        return builder.sizedByteArray()
    }

    fun parseBatchSyncResponse(bytes: ByteArray): MovieBatchSyncPlan {
        val response = BatchSyncResponse.getRootAsBatchSyncResponse(bytes.asFlatBuffer())
        val stale = (0 until response.staleBatchIdsLength).map { response.staleBatchIds(it).toInt() }
        val deleted = (0 until response.deletedBatchIdsLength).map { response.deletedBatchIds(it).toInt() }
        val versions = (0 until response.serverVersionsLength).mapNotNull { index ->
            val version = response.serverVersions(index) ?: return@mapNotNull null
            version.batchId.toInt() to version.version.toLong()
        }.toMap()
        return MovieBatchSyncPlan(
            staleBatchIds = stale,
            deletedBatchIds = deleted,
            serverVersions = versions,
        )
    }

    fun buildBatchFetchRequest(batchIds: List<Int>): ByteArray {
        val builder = FlatBufferBuilder(64 + batchIds.size * 4)
        val ids = BatchFetchRequest.createBatchIdsVector(builder, batchIds.map { it.toUInt() }.toUIntArray())
        val root = BatchFetchRequest.createBatchFetchRequest(builder, ids)
        builder.finish(root)
        return builder.sizedByteArray()
    }

    fun buildSeriesBundleSyncRequest(cachedVersions: Map<String, Long>): ByteArray {
        val builder = FlatBufferBuilder(64 + cachedVersions.size * 32)
        val versionOffsets = cachedVersions.toSortedMap().mapNotNull { (seriesId, version) ->
            val uuid = seriesId.toJavaUuidOrNull() ?: return@mapNotNull null
            SeriesBundleVersion.startSeriesBundleVersion(builder)
            SeriesBundleVersion.addVersion(builder, version.toULong())
            SeriesBundleVersion.addSeriesId(builder, uuid.toFlatBufferUuid(builder))
            SeriesBundleVersion.endSeriesBundleVersion(builder)
        }.toIntArray()
        val versions = SeriesBundleSyncRequest.createCachedVersionsVector(builder, versionOffsets)
        val root = SeriesBundleSyncRequest.createSeriesBundleSyncRequest(builder, versions)
        builder.finish(root)
        return builder.sizedByteArray()
    }

    fun parseSeriesBundleSyncResponse(bytes: ByteArray): SeriesBundleSyncPlan {
        val response = SeriesBundleSyncResponse.getRootAsSeriesBundleSyncResponse(bytes.asFlatBuffer())
        val stale = (0 until response.staleSeriesIdsLength).mapNotNull { response.staleSeriesIds(it)?.toUuidString() }
        val deleted = (0 until response.deletedSeriesIdsLength).mapNotNull { response.deletedSeriesIds(it)?.toUuidString() }
        val versions = (0 until response.serverVersionsLength).mapNotNull { index ->
            val version = response.serverVersions(index) ?: return@mapNotNull null
            version.seriesId.toUuidString() to version.version.toLong()
        }.toMap()
        return SeriesBundleSyncPlan(
            staleSeriesIds = stale,
            deletedSeriesIds = deleted,
            serverVersions = versions,
        )
    }

    fun buildSeriesBundleFetchRequest(seriesIds: List<String>): ByteArray {
        val uuids = seriesIds.mapNotNull { it.toJavaUuidOrNull() }
        val builder = FlatBufferBuilder(64 + uuids.size * 16)
        val seriesVector = createUuidVector(builder, uuids) { b, count ->
            SeriesBundleFetchRequest.startSeriesIdsVector(b, count)
        }
        val root = SeriesBundleFetchRequest.createSeriesBundleFetchRequest(builder, seriesVector)
        builder.finish(root)
        return builder.sizedByteArray()
    }

    fun parseLibraryList(bytes: ByteArray): List<LibraryInfo> = parseLibraryList(bytes.asFlatBuffer())

    fun parseLibraryList(buffer: ByteBuffer): List<LibraryInfo> {
        val list = LibraryList.getRootAsLibraryList(buffer.asFlatBuffer())
        return (0 until list.itemsLength).mapNotNull { index ->
            val library = list.items(index) ?: return@mapNotNull null
            LibraryInfo(
                id = library.id.toUuidString(),
                name = library.name,
                kind = library.libraryType.toLibraryKind(),
            )
        }
    }

    fun parseMoviePayload(buffer: ByteBuffer, expectedBatchId: Int? = null): Result<List<ParsedMovieBatch>> =
        parseRoot(
            responseParser = {
                val response = BatchFetchResponse.getRootAsBatchFetchResponse(buffer.asFlatBuffer())
                val batches = (0 until response.batchesLength).mapNotNull { index ->
                    response.batches(MediaBatchData(), index)?.toParsedMovieBatch(MoviePayloadRoot.BatchFetchResponse)
                }
                if (batches.isEmpty()) error("BatchFetchResponse did not contain batches")
                batches
            },
            singleParser = {
                val batch = MediaBatchData.getRootAsMediaBatchData(buffer.asFlatBuffer())
                listOf(batch.toParsedMovieBatch(MoviePayloadRoot.MediaBatchData))
            },
            expectedMatches = { batches -> expectedBatchId == null || batches.any { it.batchId == expectedBatchId } },
            filterExpected = { batches -> if (expectedBatchId == null) batches else batches.filter { it.batchId == expectedBatchId } },
            mismatchMessage = "Movie batch payload did not contain expected batch $expectedBatchId",
        )

    fun parseSeriesPayload(buffer: ByteBuffer, expectedSeriesId: String? = null): Result<List<ParsedSeriesBundle>> =
        parseRoot(
            responseParser = {
                val response = SeriesBundleFetchResponse.getRootAsSeriesBundleFetchResponse(buffer.asFlatBuffer())
                val bundles = (0 until response.bundlesLength).mapNotNull { index ->
                    response.bundles(SeriesBundleData(), index)?.toParsedSeriesBundle(SeriesPayloadRoot.SeriesBundleFetchResponse)
                }
                if (bundles.isEmpty()) error("SeriesBundleFetchResponse did not contain bundles")
                bundles
            },
            singleParser = {
                val bundle = SeriesBundleData.getRootAsSeriesBundleData(buffer.asFlatBuffer())
                listOf(bundle.toParsedSeriesBundle(SeriesPayloadRoot.SeriesBundleData))
            },
            expectedMatches = { bundles -> expectedSeriesId == null || bundles.any { it.seriesId == expectedSeriesId } },
            filterExpected = { bundles -> if (expectedSeriesId == null) bundles else bundles.filter { it.seriesId == expectedSeriesId } },
            mismatchMessage = "Series bundle payload did not contain expected series $expectedSeriesId",
        )

    private fun <T> parseRoot(
        responseParser: () -> List<T>,
        singleParser: () -> List<T>,
        expectedMatches: (List<T>) -> Boolean,
        filterExpected: (List<T>) -> List<T>,
        mismatchMessage: String,
    ): Result<List<T>> {
        val response = runCatching(responseParser)
            .getOrNull()
            ?.takeIf(expectedMatches)
            ?.let(filterExpected)
            ?.takeIf { it.isNotEmpty() }
        if (response != null) return Result.success(response)

        val single = runCatching(singleParser)
            .getOrElse { return Result.failure(it) }
        if (!expectedMatches(single)) return Result.failure(IllegalArgumentException(mismatchMessage))
        return Result.success(filterExpected(single))
    }

    private fun ByteArray.asFlatBuffer(): ByteBuffer = ByteBuffer.wrap(this).order(ByteOrder.LITTLE_ENDIAN)

    private fun ByteBuffer.asFlatBuffer(): ByteBuffer = asReadOnlyBuffer().order(ByteOrder.LITTLE_ENDIAN)

    private fun MediaBatchData.toParsedMovieBatch(root: MoviePayloadRoot): ParsedMovieBatch {
        val itemCount = itemsLength
        return ParsedMovieBatch(
            batchId = batchId.toInt(),
            version = version.toLong(),
            data = this,
            root = root,
            itemCount = itemCount,
        )
    }

    private fun SeriesBundleData.toParsedSeriesBundle(root: SeriesPayloadRoot): ParsedSeriesBundle {
        val itemCount = itemsLength
        return ParsedSeriesBundle(
            seriesId = seriesId.toUuidString(),
            version = version.toLong(),
            data = this,
            root = root,
            itemCount = itemCount,
        )
    }

    private fun createUuidVector(
        builder: FlatBufferBuilder,
        uuids: List<UUID>,
        startVector: (FlatBufferBuilder, Int) -> Unit,
    ): Int {
        startVector(builder, uuids.size)
        uuids.asReversed().forEach { uuid -> uuid.toFlatBufferUuid(builder) }
        return builder.endVector()
    }
}

data class ParsedMovieBatch(
    val batchId: Int,
    val version: Long,
    val data: MediaBatchData,
    val root: MoviePayloadRoot,
    val itemCount: Int,
)

enum class MoviePayloadRoot {
    MediaBatchData,
    BatchFetchResponse,
}

data class BrowseImageCard(
    val key: ImageRequestKey,
    val title: String,
    val subtitle: String,
    val publicFallbackPath: String?,
)

class MovieLibraryAccessor internal constructor(
    batches: List<ParsedMovieBatch>,
) {
    private val batches: List<ParsedMovieBatch> = batches.sortedBy { it.batchId }

    val batchIds: List<Int> = this.batches.map { it.batchId }
    val batchCount: Int get() = batches.size
    val itemCount: Int get() = batches.sumOf { it.itemCount }

    private val movieReferences: List<MovieReference> by lazy {
        buildList {
            batches.forEach { batch ->
                for (itemIndex in 0 until batch.data.itemsLength) {
                    val item = batch.data.items(itemIndex) ?: continue
                    if (item.variantType == MediaVariant.MovieReference) {
                        (item.variant(MovieReference()) as? MovieReference)?.let(::add)
                    }
                }
            }
        }
    }

    val movieCount: Int get() = movieReferences.size

    fun movieAt(index: Int): MovieReference? = movieReferences.getOrNull(index)

    fun primaryImageKeys(): Set<ImageRequestKey> = buildSet {
        for (index in movieReferences.indices) {
            val details = movieAt(index)?.details ?: continue
            details.primaryPosterIid?.let { add(ImageRequestKey(it.toUuidString(), BrowseImageCategory.Poster)) }
            details.primaryBackdropIid?.let { add(ImageRequestKey(it.toUuidString(), BrowseImageCategory.Backdrop)) }
        }
    }

    fun primaryImageCards(limit: Int): List<BrowseImageCard> = buildList {
        if (limit <= 0) return@buildList
        for (index in movieReferences.indices) {
            val movie = movieAt(index) ?: continue
            val details = movie.details ?: continue
            details.primaryPosterIid?.let {
                add(
                    BrowseImageCard(
                        key = ImageRequestKey(it.toUuidString(), BrowseImageCategory.Poster),
                        title = movie.title,
                        subtitle = "Movie poster",
                        publicFallbackPath = details.posterPath,
                    ),
                )
            }
            if (size >= limit) return@buildList
            details.primaryBackdropIid?.let {
                add(
                    BrowseImageCard(
                        key = ImageRequestKey(it.toUuidString(), BrowseImageCategory.Backdrop),
                        title = movie.title,
                        subtitle = "Movie backdrop",
                        publicFallbackPath = details.backdropPath,
                    ),
                )
            }
            if (size >= limit) return@buildList
        }
    }
}

data class ParsedSeriesBundle(
    val seriesId: String,
    val version: Long,
    val data: SeriesBundleData,
    val root: SeriesPayloadRoot,
    val itemCount: Int,
)

enum class SeriesPayloadRoot {
    SeriesBundleData,
    SeriesBundleFetchResponse,
}

class SeriesLibraryAccessor internal constructor(
    bundles: List<ParsedSeriesBundle>,
) {
    private val bundles: List<ParsedSeriesBundle> = bundles.sortedBy { it.seriesId }

    val seriesIds: List<String> = this.bundles.map { it.seriesId }
    val bundleCount: Int get() = bundles.size
    val itemCount: Int get() = bundles.sumOf { it.itemCount }

    private val seriesReferences: List<SeriesReference> by lazy {
        buildList {
            bundles.forEach { bundle ->
                for (itemIndex in 0 until bundle.data.itemsLength) {
                    val item = bundle.data.items(itemIndex) ?: continue
                    if (item.variantType == MediaVariant.SeriesReference) {
                        (item.variant(SeriesReference()) as? SeriesReference)?.let(::add)
                    }
                }
            }
        }
    }

    private val episodeReferences: List<EpisodeReference> by lazy {
        buildList {
            bundles.forEach { bundle ->
                for (itemIndex in 0 until bundle.data.itemsLength) {
                    val item = bundle.data.items(itemIndex) ?: continue
                    if (item.variantType == MediaVariant.EpisodeReference) {
                        (item.variant(EpisodeReference()) as? EpisodeReference)?.let(::add)
                    }
                }
            }
        }
    }

    val seriesReferenceCount: Int get() = seriesReferences.size
    val episodeCount: Int get() = episodeReferences.size

    fun seriesAt(index: Int): SeriesReference? = seriesReferences.getOrNull(index)

    fun episodeAt(index: Int): EpisodeReference? = episodeReferences.getOrNull(index)

    fun primaryImageKeys(): Set<ImageRequestKey> = buildSet {
        for (index in seriesReferences.indices) {
            val details = seriesAt(index)?.details ?: continue
            details.primaryPosterIid?.let { add(ImageRequestKey(it.toUuidString(), BrowseImageCategory.Poster)) }
            details.primaryBackdropIid?.let { add(ImageRequestKey(it.toUuidString(), BrowseImageCategory.Backdrop)) }
        }
        for (index in episodeReferences.indices) {
            val details = episodeAt(index)?.details ?: continue
            details.primaryStillIid?.let { add(ImageRequestKey(it.toUuidString(), BrowseImageCategory.Episode)) }
        }
    }

    fun primaryImageCards(limit: Int): List<BrowseImageCard> = buildList {
        if (limit <= 0) return@buildList
        for (index in seriesReferences.indices) {
            val series = seriesAt(index) ?: continue
            val details = series.details ?: continue
            details.primaryPosterIid?.let {
                add(
                    BrowseImageCard(
                        key = ImageRequestKey(it.toUuidString(), BrowseImageCategory.Poster),
                        title = series.title,
                        subtitle = "Series poster",
                        publicFallbackPath = details.posterPath,
                    ),
                )
            }
            if (size >= limit) return@buildList
            details.primaryBackdropIid?.let {
                add(
                    BrowseImageCard(
                        key = ImageRequestKey(it.toUuidString(), BrowseImageCategory.Backdrop),
                        title = series.title,
                        subtitle = "Series backdrop",
                        publicFallbackPath = details.backdropPath,
                    ),
                )
            }
            if (size >= limit) return@buildList
        }
        for (index in episodeReferences.indices) {
            val episode = episodeAt(index) ?: continue
            val details = episode.details ?: continue
            details.primaryStillIid?.let {
                add(
                    BrowseImageCard(
                        key = ImageRequestKey(it.toUuidString(), BrowseImageCategory.Episode),
                        title = details.name ?: "Season ${episode.seasonNumber.toInt()}, episode ${episode.episodeNumber.toInt()}",
                        subtitle = "Episode still • S${episode.seasonNumber.toInt()} E${episode.episodeNumber.toInt()}",
                        publicFallbackPath = details.stillPath,
                    ),
                )
            }
            if (size >= limit) return@buildList
        }
    }
}
