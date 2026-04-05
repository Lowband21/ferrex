package com.ferrex.android.core.api

import com.google.flatbuffers.FlatBufferBuilder
import ferrex.auth.AuthToken
import ferrex.auth.LoginRequest
import ferrex.auth.SetupStatus
import ferrex.library.LibraryList
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import okhttp3.MediaType
import okhttp3.MediaType.Companion.toMediaType
import okhttp3.OkHttpClient
import okhttp3.Request
import okhttp3.RequestBody.Companion.toRequestBody
import java.nio.ByteBuffer
import java.nio.ByteOrder
import javax.inject.Inject
import javax.inject.Singleton

/**
 * Low-level HTTP client for the Ferrex server API.
 *
 * All responses are FlatBuffers byte arrays; callers use the generated
 * accessor types to read fields zero-copy from the returned ByteBuffer.
 *
 * This class is intentionally NOT aware of auth tokens — the [AuthInterceptor]
 * handles token injection at the OkHttp layer.
 */
@Singleton
class FerrexApiClient @Inject constructor(
    private val httpClient: OkHttpClient,
    private val serverConfig: ServerConfig,
) {
    companion object {
        val FLATBUFFERS_MEDIA_TYPE: MediaType = "application/x-flatbuffers".toMediaType()
        private const val ACCEPT_HEADER = "application/x-flatbuffers"

        // API routes (mirrors ferrex-core/src/api/routes.rs)
        object Routes {
            const val SETUP_STATUS = "/api/v1/setup/status"
            const val AUTH_LOGIN = "/api/v1/auth/login"
            const val AUTH_REGISTER = "/api/v1/auth/register"
            const val AUTH_REFRESH = "/api/v1/auth/refresh"
            const val AUTH_LOGOUT = "/api/v1/auth/logout"
            const val USERS_ME = "/api/v1/users/me"
            const val LIBRARIES = "/api/v1/libraries"

            fun movieBatchesBundle(libraryId: String) =
                "/api/v1/libraries/$libraryId/movie-batches"
            fun movieBatchesSync(libraryId: String) =
                "/api/v1/libraries/$libraryId/movie-batches:sync"
            fun movieBatchesFetch(libraryId: String) =
                "/api/v1/libraries/$libraryId/movie-batches:fetch"
            fun seriesBundle(libraryId: String, seriesId: String) =
                "/api/v1/libraries/$libraryId/series-bundles/$seriesId"
            fun seriesBundlesSync(libraryId: String) =
                "/api/v1/libraries/$libraryId/series-bundles:sync"
            fun seriesBundlesFetch(libraryId: String) =
                "/api/v1/libraries/$libraryId/series-bundles:fetch"
            fun sortedIndices(libraryId: String) =
                "/api/v1/libraries/$libraryId/indices/sorted"
            fun filteredIndices(libraryId: String) =
                "/api/v1/libraries/$libraryId/indices/filter"

            const val MEDIA_QUERY = "/api/v1/media/query"
            const val IMAGES_MANIFEST = "/api/v1/images/manifest"
            fun imageBlob(token: String) = "/api/v1/images/blob/$token"
            fun imageByIid(uuid: String) = "/api/v1/images/iid/$uuid"
            const val IMAGES_EVENTS = "/api/v1/images/events"

            const val WATCH_PROGRESS = "/api/v1/watch/progress"
            const val WATCH_STATE = "/api/v1/watch/state"
            const val WATCH_CONTINUE = "/api/v1/watch/continue"
            fun seriesWatchState(tmdbSeriesId: Long) =
                "/api/v1/watch/series/$tmdbSeriesId"
            fun seriesNext(tmdbSeriesId: Long) =
                "/api/v1/watch/series/$tmdbSeriesId/next"

            fun streamPlay(id: String) = "/api/v1/stream/$id"
            fun streamTicket(id: String) = "/api/v1/stream/$id/ticket"
        }
    }

    // ── Setup / connectivity ────────────────────────────────────────

    /**
     * Check server setup status. This is the first call made to verify
     * the server URL is valid and reachable.
     *
     * Does NOT require authentication.
     */
    suspend fun getSetupStatus(): ApiResult<SetupStatus> = get(Routes.SETUP_STATUS) { bytes ->
        SetupStatus.getRootAsSetupStatus(ByteBuffer.wrap(bytes).order(ByteOrder.LITTLE_ENDIAN))
    }

    // ── Auth ────────────────────────────────────────────────────────

    /**
     * Log in with username/password. Returns access + refresh tokens.
     */
    suspend fun login(username: String, password: String): ApiResult<AuthToken> {
        val builder = FlatBufferBuilder(256)
        val usernameOff = builder.createString(username)
        val passwordOff = builder.createString(password)
        val deviceNameOff = builder.createString("Android")
        val root = LoginRequest.createLoginRequest(builder, usernameOff, passwordOff, deviceNameOff)
        builder.finish(root)

        return post(Routes.AUTH_LOGIN, builder.sizedByteArray()) { bytes ->
            AuthToken.getRootAsAuthToken(ByteBuffer.wrap(bytes).order(ByteOrder.LITTLE_ENDIAN))
        }
    }

    /**
     * Refresh an expired access token.
     */
    suspend fun refreshToken(refreshToken: String): ApiResult<AuthToken> {
        val builder = FlatBufferBuilder(256)
        val tokenOff = builder.createString(refreshToken)
        ferrex.auth.RefreshRequest.startRefreshRequest(builder)
        ferrex.auth.RefreshRequest.addRefreshToken(builder, tokenOff)
        val root = ferrex.auth.RefreshRequest.endRefreshRequest(builder)
        builder.finish(root)

        return post(Routes.AUTH_REFRESH, builder.sizedByteArray()) { bytes ->
            AuthToken.getRootAsAuthToken(ByteBuffer.wrap(bytes).order(ByteOrder.LITTLE_ENDIAN))
        }
    }

    // ── Libraries ───────────────────────────────────────────────────

    /**
     * Fetch all libraries the current user has access to.
     */
    suspend fun getLibraries(): ApiResult<LibraryList> = get(Routes.LIBRARIES) { bytes ->
        LibraryList.getRootAsLibraryList(ByteBuffer.wrap(bytes).order(ByteOrder.LITTLE_ENDIAN))
    }

    // ── Image URLs ──────────────────────────────────────────────────

    /**
     * Build the full URL for a content-addressed image blob by token.
     * These URLs are immutable — Coil can cache them forever.
     */
    fun imageBlobUrl(token: String): String =
        "${serverConfig.serverUrl}${Routes.imageBlob(token)}"

    /**
     * Build the URL to serve an image by its IID (image instance UUID).
     * The server resolves the IID → blob token internally.
     */
    fun imageIidUrl(uuid: String): String =
        "${serverConfig.serverUrl}${Routes.imageByIid(uuid)}"

    // ── HTTP primitives ─────────────────────────────────────────────

    private suspend fun <T> get(
        path: String,
        parse: (ByteArray) -> T,
    ): ApiResult<T> = withContext(Dispatchers.IO) {
        try {
            val request = Request.Builder()
                .url("${serverConfig.serverUrl}$path")
                .addHeader("Accept", ACCEPT_HEADER)
                .get()
                .build()

            val response = httpClient.newCall(request).execute()
            if (!response.isSuccessful) {
                return@withContext ApiResult.HttpError(response.code, response.message)
            }

            val bytes = response.body?.bytes()
                ?: return@withContext ApiResult.HttpError(response.code, "Empty response body")

            ApiResult.Success(parse(bytes))
        } catch (e: Exception) {
            ApiResult.NetworkError(e)
        }
    }

    private suspend fun <T> post(
        path: String,
        body: ByteArray,
        parse: (ByteArray) -> T,
    ): ApiResult<T> = withContext(Dispatchers.IO) {
        try {
            val request = Request.Builder()
                .url("${serverConfig.serverUrl}$path")
                .addHeader("Accept", ACCEPT_HEADER)
                .post(body.toRequestBody(FLATBUFFERS_MEDIA_TYPE))
                .build()

            val response = httpClient.newCall(request).execute()
            if (!response.isSuccessful) {
                return@withContext ApiResult.HttpError(response.code, response.message)
            }

            val bytes = response.body?.bytes()
                ?: return@withContext ApiResult.HttpError(response.code, "Empty response body")

            ApiResult.Success(parse(bytes))
        } catch (e: Exception) {
            ApiResult.NetworkError(e)
        }
    }
}

/**
 * Typed result wrapper for API calls.
 */
sealed interface ApiResult<out T> {
    data class Success<T>(val data: T) : ApiResult<T>
    data class HttpError(val code: Int, val message: String) : ApiResult<Nothing>
    data class NetworkError(val exception: Exception) : ApiResult<Nothing>
}

/** Map the success value, pass through errors. */
inline fun <T, R> ApiResult<T>.map(transform: (T) -> R): ApiResult<R> = when (this) {
    is ApiResult.Success -> ApiResult.Success(transform(data))
    is ApiResult.HttpError -> this
    is ApiResult.NetworkError -> this
}
