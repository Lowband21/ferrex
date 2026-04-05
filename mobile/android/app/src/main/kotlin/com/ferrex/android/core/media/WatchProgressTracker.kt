package com.ferrex.android.core.media

import com.ferrex.android.core.api.ServerConfig
import com.ferrex.android.core.diagnostics.DiagnosticLog
import com.google.flatbuffers.FlatBufferBuilder
import ferrex.common.Timestamp
import ferrex.ids.Uuid
import ferrex.watch.WatchProgressUpdate
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import okhttp3.MediaType.Companion.toMediaType
import okhttp3.OkHttpClient
import okhttp3.Request
import okhttp3.RequestBody.Companion.toRequestBody
import javax.inject.Inject
import javax.inject.Singleton

private const val TAG = "WatchProgress"

/**
 * Coroutine-based watch progress reporter.
 *
 * Called from the player's LaunchedEffect loop every ~10 seconds,
 * and immediately on pause/stop. Sends POST /watch/progress with
 * the current playback position as a FlatBuffer payload.
 *
 * ## FlatBuffer struct ordering
 *
 * `Uuid` and `Timestamp` are FlatBuffer **structs** (not tables).
 * Structs must be written inline during the parent table construction —
 * between `start…()` and `end…()`. Creating them before `start` and
 * trying to add the returned offset triggers:
 *
 *     AssertionError: FlatBuffers: struct must be serialized inline.
 *
 * `AssertionError` extends `Error`, not `Exception`, so a bare
 * `catch (Exception)` won't intercept it. The safety-net catch below
 * uses `Throwable` to cover both families.
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

            // Parse UUID string → raw bytes
            val uuid = java.util.UUID.fromString(mediaId)
            val msb = uuid.mostSignificantBits
            val lsb = uuid.leastSignificantBits

            // ── Build the WatchProgressUpdate table ─────────────
            //
            // FlatBuffer rule: strings and sub-tables must be created
            // BEFORE starting the parent table.  Structs (Uuid, Timestamp)
            // must be created INLINE — between start and end.
            //
            // Scalar fields (position, duration) go anywhere between
            // start and end.

            WatchProgressUpdate.startWatchProgressUpdate(builder)

            // Struct field: write Uuid inline, pass returned offset to add
            WatchProgressUpdate.addMediaId(builder, Uuid.createUuid(
                builder,
                (msb ushr 56).toUByte(), (msb ushr 48).toUByte(),
                (msb ushr 40).toUByte(), (msb ushr 32).toUByte(),
                (msb ushr 24).toUByte(), (msb ushr 16).toUByte(),
                (msb ushr 8).toUByte(),  msb.toUByte(),
                (lsb ushr 56).toUByte(), (lsb ushr 48).toUByte(),
                (lsb ushr 40).toUByte(), (lsb ushr 32).toUByte(),
                (lsb ushr 24).toUByte(), (lsb ushr 16).toUByte(),
                (lsb ushr 8).toUByte(),  lsb.toUByte(),
            ))

            WatchProgressUpdate.addPosition(builder, positionSeconds)
            WatchProgressUpdate.addDuration(builder, durationSeconds)

            // Struct field: write Timestamp inline
            WatchProgressUpdate.addTimestamp(builder,
                Timestamp.createTimestamp(builder, System.currentTimeMillis()))

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
        } catch (t: Throwable) {
            // Safety net: catch Throwable (not just Exception) so that
            // AssertionError and other Error subclasses don't kill the
            // player.  Progress reporting is best-effort.
            DiagnosticLog.w(TAG,
                "Progress report failed (pos=${positionSeconds}s): ${t.message}", t)
        }
    }
}
