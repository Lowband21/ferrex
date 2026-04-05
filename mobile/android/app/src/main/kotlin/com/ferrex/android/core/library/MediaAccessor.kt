package com.ferrex.android.core.library

import ferrex.library.BatchFetchResponse
import ferrex.media.EpisodeReference
import ferrex.media.Media
import ferrex.media.MediaVariant
import ferrex.media.MovieReference
import ferrex.media.SeasonReference
import ferrex.media.SeriesReference
import java.nio.ByteBuffer

/**
 * Zero-copy accessor for media items from a batch fetch response.
 *
 * Wraps the raw FlatBuffer [ByteBuffer] (typically memory-mapped from disk)
 * and provides indexed access to media items without allocating Kotlin objects
 * per item. This is critical for 60fps scroll in LazyVerticalGrid — the
 * generated FlatBuffer accessor types read directly from the buffer via
 * pointer offset arithmetic, no GC pressure.
 *
 * Usage in Compose:
 * ```
 * val accessor = MediaAccessor(cachedByteBuffer)
 * LazyVerticalGrid(...) {
 *     items(accessor.movieCount) { index ->
 *         val movie = accessor.movieAt(index)
 *         PosterCard(title = movie.title, ...)
 *     }
 * }
 * ```
 */
class MediaAccessor(buffer: ByteBuffer) {

    private val response: BatchFetchResponse =
        BatchFetchResponse.getRootAsBatchFetchResponse(buffer)

    /** Total number of batches in this response. */
    val batchCount: Int get() = response.batchesLength

    /**
     * Collects all movies across all batches.
     * Returns a list of (batchIndex, itemIndex) pairs for indexed access.
     */
    private val movieIndices: List<Pair<Int, Int>> by lazy {
        buildList {
            for (b in 0 until response.batchesLength) {
                val batch = response.batches(b) ?: continue
                for (i in 0 until batch.itemsLength) {
                    val item = batch.items(i) ?: continue
                    if (item.variantType == MediaVariant.MovieReference) {
                        add(b to i)
                    }
                }
            }
        }
    }

    private val seriesIndices: List<Pair<Int, Int>> by lazy {
        buildList {
            for (b in 0 until response.batchesLength) {
                val batch = response.batches(b) ?: continue
                for (i in 0 until batch.itemsLength) {
                    val item = batch.items(i) ?: continue
                    if (item.variantType == MediaVariant.SeriesReference) {
                        add(b to i)
                    }
                }
            }
        }
    }

    /** Number of movies across all batches. */
    val movieCount: Int get() = movieIndices.size

    private val seasonIndices: List<Pair<Int, Int>> by lazy {
        buildList {
            for (b in 0 until response.batchesLength) {
                val batch = response.batches(b) ?: continue
                for (i in 0 until batch.itemsLength) {
                    val item = batch.items(i) ?: continue
                    if (item.variantType == MediaVariant.SeasonReference) {
                        add(b to i)
                    }
                }
            }
        }
    }

    private val episodeIndices: List<Pair<Int, Int>> by lazy {
        buildList {
            for (b in 0 until response.batchesLength) {
                val batch = response.batches(b) ?: continue
                for (i in 0 until batch.itemsLength) {
                    val item = batch.items(i) ?: continue
                    if (item.variantType == MediaVariant.EpisodeReference) {
                        add(b to i)
                    }
                }
            }
        }
    }

    /** Number of series across all batches. */
    val seriesCount: Int get() = seriesIndices.size

    /** Number of seasons across all batches. */
    val seasonCount: Int get() = seasonIndices.size

    /** Number of episodes across all batches. */
    val episodeCount: Int get() = episodeIndices.size

    /**
     * Access movie at the given index (across all batches).
     * Returns a FlatBuffer accessor — reads from the underlying ByteBuffer
     * without allocating a new object.
     */
    fun movieAt(index: Int): MovieReference? {
        val (batchIdx, itemIdx) = movieIndices[index]
        val item = response.batches(batchIdx)?.items(itemIdx) ?: return null
        return item.variant(MovieReference()) as? MovieReference
    }

    /**
     * Access series at the given index.
     */
    fun seriesAt(index: Int): SeriesReference? {
        val (batchIdx, itemIdx) = seriesIndices[index]
        val item = response.batches(batchIdx)?.items(itemIdx) ?: return null
        return item.variant(SeriesReference()) as? SeriesReference
    }

    /**
     * Access season at the given index.
     */
    fun seasonAt(index: Int): SeasonReference? {
        val (batchIdx, itemIdx) = seasonIndices[index]
        val item = response.batches(batchIdx)?.items(itemIdx) ?: return null
        return item.variant(SeasonReference()) as? SeasonReference
    }

    /**
     * Access episode at the given index.
     */
    fun episodeAt(index: Int): EpisodeReference? {
        val (batchIdx, itemIdx) = episodeIndices[index]
        val item = response.batches(batchIdx)?.items(itemIdx) ?: return null
        return item.variant(EpisodeReference()) as? EpisodeReference
    }

    /**
     * Find all seasons belonging to a series (by series UUID string).
     * Returns seasons sorted by season number.
     */
    fun seasonsForSeries(seriesUuid: String): List<SeasonReference> {
        return (0 until seasonCount).mapNotNull { i ->
            val season = seasonAt(i) ?: return@mapNotNull null
            if (season.seriesId?.toUuidString() == seriesUuid) season else null
        }.sortedBy { it.seasonNumber.toInt() }
    }

    /**
     * Find all episodes belonging to a specific season of a series.
     * Returns episodes sorted by episode number.
     */
    fun episodesForSeason(seriesUuid: String, seasonNumber: Int): List<EpisodeReference> {
        return (0 until episodeCount).mapNotNull { i ->
            val episode = episodeAt(i) ?: return@mapNotNull null
            if (episode.seriesId?.toUuidString() == seriesUuid &&
                episode.seasonNumber.toInt() == seasonNumber
            ) episode else null
        }.sortedBy { it.episodeNumber.toInt() }
    }

    /**
     * Access any media item at a given batch + item index.
     */
    fun mediaAt(batchIndex: Int, itemIndex: Int): Media? {
        return response.batches(batchIndex)?.items(itemIndex)
    }

    /**
     * Find a movie by its UUID string (linear scan across all batches).
     * Returns null if not found.
     */
    fun findMovieByUuid(uuidString: String): MovieReference? {
        for (b in 0 until response.batchesLength) {
            val batch = response.batches(b) ?: continue
            for (i in 0 until batch.itemsLength) {
                val item = batch.items(i) ?: continue
                if (item.variantType != MediaVariant.MovieReference) continue
                val movie = item.variant(MovieReference()) as? MovieReference ?: continue
                val id = movie.id ?: continue
                if (id.toUuidString() == uuidString) return movie
            }
        }
        return null
    }

    /**
     * Find a series by its UUID string (linear scan across all batches).
     * Returns null if not found.
     */
    fun findSeriesByUuid(uuidString: String): SeriesReference? {
        for (b in 0 until response.batchesLength) {
            val batch = response.batches(b) ?: continue
            for (i in 0 until batch.itemsLength) {
                val item = batch.items(i) ?: continue
                if (item.variantType != MediaVariant.SeriesReference) continue
                val series = item.variant(SeriesReference()) as? SeriesReference ?: continue
                val id = series.id ?: continue
                if (id.toUuidString() == uuidString) return series
            }
        }
        return null
    }
}

/**
 * Lightweight holder for a single batch's data.
 * Used when we load individual batches (e.g., from versioned cache).
 */
class SingleBatchAccessor(buffer: ByteBuffer) {
    private val response: BatchFetchResponse =
        BatchFetchResponse.getRootAsBatchFetchResponse(buffer)

    val itemCount: Int
        get() = response.batches(0)?.itemsLength ?: 0

    fun itemAt(index: Int): Media? =
        response.batches(0)?.items(index)

    fun movieAt(index: Int): MovieReference? {
        val item = itemAt(index) ?: return null
        if (item.variantType != MediaVariant.MovieReference) return null
        return item.variant(MovieReference()) as? MovieReference
    }
}
