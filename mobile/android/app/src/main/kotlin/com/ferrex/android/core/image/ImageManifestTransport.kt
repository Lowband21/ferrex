package com.ferrex.android.core.image

import com.ferrex.android.core.api.ServerConfig
import com.ferrex.android.core.library.LibrarySyncFailure
import com.ferrex.android.core.library.LibrarySyncResult
import kotlinx.coroutines.CoroutineDispatcher
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import okhttp3.MediaType.Companion.toMediaType
import okhttp3.OkHttpClient
import okhttp3.Request
import okhttp3.RequestBody.Companion.toRequestBody
import java.io.IOException

interface ImageManifestTransport {
    suspend fun fetchManifest(keys: Collection<ImageRequestKey>): LibrarySyncResult<List<ImageManifestRecord>>
}

class OkHttpImageManifestTransport(
    private val httpClient: OkHttpClient,
    private val serverConfig: ServerConfig,
    private val ioDispatcher: CoroutineDispatcher = Dispatchers.IO,
) : ImageManifestTransport {
    override suspend fun fetchManifest(keys: Collection<ImageRequestKey>): LibrarySyncResult<List<ImageManifestRecord>> =
        withContext(ioDispatcher) {
            val validKeys = ImageFlatBuffers.validKeys(keys)
            if (validKeys.isEmpty()) return@withContext LibrarySyncResult.Success(emptyList())
            try {
                val request = Request.Builder()
                    .url("${serverConfig.requireUrl()}${Routes.MANIFEST}")
                    .post(ImageFlatBuffers.buildManifestRequest(validKeys).toRequestBody(FLATBUFFERS_MEDIA_TYPE))
                    .header("Accept", FLATBUFFERS_MIME)
                    .header("Content-Type", FLATBUFFERS_MIME)
                    .build()

                httpClient.newCall(request).execute().use { response ->
                    if (!response.isSuccessful) {
                        return@withContext LibrarySyncResult.Failure(
                            LibrarySyncFailure.Http(
                                code = response.code,
                                message = response.message.ifBlank { "HTTP ${response.code}" },
                            ),
                        )
                    }
                    val bytes = response.body?.bytes() ?: return@withContext LibrarySyncResult.Failure(LibrarySyncFailure.EmptyBody)
                    if (bytes.isEmpty()) return@withContext LibrarySyncResult.Failure(LibrarySyncFailure.EmptyBody)
                    runCatching { ImageFlatBuffers.parseManifestResponse(bytes) }
                        .fold(
                            onSuccess = { LibrarySyncResult.Success(it) },
                            onFailure = { LibrarySyncResult.Failure(LibrarySyncFailure.Parse(it.message ?: "Invalid image manifest")) },
                        )
                }
            } catch (e: IOException) {
                LibrarySyncResult.Failure(LibrarySyncFailure.Network(e.localizedMessage ?: "Network unavailable"))
            } catch (e: IllegalArgumentException) {
                LibrarySyncResult.Failure(LibrarySyncFailure.Network(e.localizedMessage ?: "Invalid server URL"))
            } catch (e: IllegalStateException) {
                LibrarySyncResult.Failure(LibrarySyncFailure.Network(e.localizedMessage ?: "Server URL is not configured"))
            }
        }

    object Routes {
        const val MANIFEST = "/api/v1/images/manifest"
        fun blob(token: String): String = "/api/v1/images/blob/$token"
    }

    companion object {
        const val FLATBUFFERS_MIME = "application/x-flatbuffers"
        private val FLATBUFFERS_MEDIA_TYPE = FLATBUFFERS_MIME.toMediaType()
    }
}
