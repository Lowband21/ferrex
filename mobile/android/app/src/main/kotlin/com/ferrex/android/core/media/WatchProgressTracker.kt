package com.ferrex.android.core.media

import com.ferrex.android.core.api.ServerConfig
import com.google.flatbuffers.FlatBufferBuilder
import ferrex.common.Timestamp
import ferrex.watch.WatchProgressUpdate
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import okhttp3.MediaType.Companion.toMediaType
import okhttp3.OkHttpClient
import okhttp3.Request
import okhttp3.RequestBody.Companion.toRequestBody
import javax.inject.Inject
import javax.inject.Singleton

/**
 * Coroutine-based watch progress reporter.
 *
 * Called from the player's LaunchedEffect loop every ~10 seconds,
 * and immediately on pause/stop. Sends POST /watch/progress with
 * the current playback position.
 */
@Singleton
class WatchProgressTracker @Inject constructor(
    private val httpClient: OkHttpClient,
    private val serverConfig: ServerConfig,
) {
    /**
     * Report playback progress to the server.
     *
     * @param mediaId UUID string of the media being played
     * @param positionSeconds Current playback position in seconds
     * @param durationSeconds Total duration in seconds
     */
    suspend fun reportProgress(
        mediaId: String,
        positionSeconds: Double,
        durationSeconds: Double,
    ) = withContext(Dispatchers.IO) {
        try {
            val builder = FlatBufferBuilder(256)

            // Build UUID bytes from the string
            val uuid = java.util.UUID.fromString(mediaId)
            val msb = uuid.mostSignificantBits
            val lsb = uuid.leastSignificantBits

            val uuidOffset = ferrex.ids.Uuid.createUuid(
                builder,
                (msb ushr 56).toUByte(), (msb ushr 48).toUByte(),
                (msb ushr 40).toUByte(), (msb ushr 32).toUByte(),
                (msb ushr 24).toUByte(), (msb ushr 16).toUByte(),
                (msb ushr 8).toUByte(), msb.toUByte(),
                (lsb ushr 56).toUByte(), (lsb ushr 48).toUByte(),
                (lsb ushr 40).toUByte(), (lsb ushr 32).toUByte(),
                (lsb ushr 24).toUByte(), (lsb ushr 16).toUByte(),
                (lsb ushr 8).toUByte(), lsb.toUByte(),
            )

            val timestampOffset = Timestamp.createTimestamp(
                builder, System.currentTimeMillis()
            )

            WatchProgressUpdate.startWatchProgressUpdate(builder)
            WatchProgressUpdate.addMediaId(builder, uuidOffset)
            WatchProgressUpdate.addPosition(builder, positionSeconds)
            WatchProgressUpdate.addDuration(builder, durationSeconds)
            WatchProgressUpdate.addTimestamp(builder, timestampOffset)
            val root = WatchProgressUpdate.endWatchProgressUpdate(builder)
            builder.finish(root)

            val request = Request.Builder()
                .url("${serverConfig.serverUrl}/api/v1/watch/progress")
                .addHeader("Accept", "application/x-flatbuffers")
                .post(
                    builder.sizedByteArray()
                        .toRequestBody("application/x-flatbuffers".toMediaType())
                )
                .build()

            httpClient.newCall(request).execute().close()
        } catch (_: Exception) {
            // Progress reporting is best-effort; don't crash the player
        }
    }
}
