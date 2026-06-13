package com.ferrex.android.core.api

import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import kotlinx.serialization.SerializationException
import kotlinx.serialization.encodeToString
import kotlinx.serialization.json.Json
import okhttp3.MediaType.Companion.toMediaType
import okhttp3.OkHttpClient
import okhttp3.Request
import okhttp3.RequestBody.Companion.toRequestBody
import java.io.IOException

interface FerrexApi {
    suspend fun getSetupStatus(serverUrl: String): ApiResult<SetupStatus>
    suspend fun knownDeviceUsers(deviceInfo: DeviceInfo): ApiResult<KnownDeviceProfilesResponse>
    suspend fun devicePasswordLogin(
        username: String,
        password: String,
        deviceInfo: DeviceInfo,
        rememberDevice: Boolean,
    ): ApiResult<AuthTokens>
    suspend fun requestPinChallenge(deviceId: String): ApiResult<PinChallengeResponse>
    suspend fun pinLogin(request: PinLoginRequest): ApiResult<AuthTokens>
    suspend fun refreshToken(refreshToken: String): ApiResult<AuthTokens>
    suspend fun currentUser(): ApiResult<CurrentUser>
}

class FerrexApiClient(
    private val httpClient: OkHttpClient,
    private val serverConfig: ServerConfig,
    private val json: Json = DefaultJson,
) : FerrexApi {
    override suspend fun getSetupStatus(serverUrl: String): ApiResult<SetupStatus> =
        get(serverUrl, Routes.SETUP_STATUS)

    override suspend fun knownDeviceUsers(deviceInfo: DeviceInfo): ApiResult<KnownDeviceProfilesResponse> =
        post(
            serverConfig.requireUrl(),
            Routes.DEVICE_USERS,
            KnownDeviceProfilesRequest(deviceInfo),
        )

    override suspend fun devicePasswordLogin(
        username: String,
        password: String,
        deviceInfo: DeviceInfo,
        rememberDevice: Boolean,
    ): ApiResult<AuthTokens> = post(
        serverConfig.requireUrl(),
        Routes.DEVICE_LOGIN,
        DevicePasswordLoginRequest(
            username = username,
            password = password,
            deviceInfo = deviceInfo,
            rememberDevice = rememberDevice,
        ),
    )

    override suspend fun requestPinChallenge(deviceId: String): ApiResult<PinChallengeResponse> =
        post(serverConfig.requireUrl(), Routes.PIN_CHALLENGE, PinChallengeRequest(deviceId))

    override suspend fun pinLogin(request: PinLoginRequest): ApiResult<AuthTokens> =
        post(serverConfig.requireUrl(), Routes.PIN_LOGIN, request)

    override suspend fun refreshToken(refreshToken: String): ApiResult<AuthTokens> =
        post(serverConfig.requireUrl(), Routes.REFRESH, RefreshRequest(refreshToken))

    override suspend fun currentUser(): ApiResult<CurrentUser> =
        get(serverConfig.requireUrl(), Routes.USERS_ME)

    private suspend inline fun <reified T> get(
        baseUrl: String,
        path: String,
    ): ApiResult<T> = execute(baseUrl, path) { builder -> builder.get() }

    private suspend inline fun <reified RequestBodyT, reified ResponseT> post(
        baseUrl: String,
        path: String,
        requestBody: RequestBodyT,
    ): ApiResult<ResponseT> = execute(baseUrl, path) { builder ->
        builder.post(json.encodeToString(requestBody).toRequestBody(JSON_MEDIA_TYPE))
    }

    private suspend inline fun <reified T> execute(
        baseUrl: String,
        path: String,
        crossinline configure: (Request.Builder) -> Request.Builder,
    ): ApiResult<T> = withContext(Dispatchers.IO) {
        val normalizedBaseUrl = ServerConfig.normalize(baseUrl)
        if (normalizedBaseUrl.isBlank()) {
            return@withContext ApiResult.NetworkError("Server URL is not configured")
        }

        val request = configure(
            Request.Builder()
                .url("$normalizedBaseUrl$path")
                .header("Accept", JSON_MEDIA_TYPE.toString()),
        ).build()

        try {
            httpClient.newCall(request).execute().use { response ->
                if (!response.isSuccessful) {
                    return@withContext ApiResult.HttpError(
                        response.code,
                        response.message.ifBlank { "HTTP ${response.code}" },
                    )
                }

                val body = response.body?.string() ?: return@withContext ApiResult.EmptyBody
                if (body.isBlank()) return@withContext ApiResult.EmptyBody

                val envelope = try {
                    json.decodeFromString<ApiEnvelope<T>>(body)
                } catch (e: SerializationException) {
                    return@withContext ApiResult.ParseError(e.message ?: "Invalid JSON")
                } catch (e: IllegalArgumentException) {
                    return@withContext ApiResult.ParseError(e.message ?: "Invalid JSON")
                }

                val data = envelope.data
                if (envelope.status != null && envelope.status != "success") {
                    return@withContext ApiResult.ServerError(
                        envelope.error ?: envelope.message ?: "Server reported an error",
                    )
                }
                if (data == null) {
                    return@withContext ApiResult.ParseError("Response did not include data")
                }
                ApiResult.Success(data)
            }
        } catch (e: IOException) {
            ApiResult.NetworkError(e.localizedMessage ?: "Network unavailable")
        } catch (e: IllegalArgumentException) {
            ApiResult.NetworkError(e.localizedMessage ?: "Invalid server URL")
        } catch (e: IllegalStateException) {
            ApiResult.NetworkError(e.localizedMessage ?: "Server URL is not configured")
        }
    }

    object Routes {
        const val SETUP_STATUS = "/api/v1/setup/status"
        const val DEVICE_LOGIN = "/api/v1/auth/device/login"
        const val DEVICE_USERS = "/api/v1/auth/device/users"
        const val PIN_CHALLENGE = "/api/v1/auth/device/pin/challenge"
        const val PIN_LOGIN = "/api/v1/auth/device/pin"
        const val REFRESH = "/api/v1/auth/refresh"
        const val USERS_ME = "/api/v1/users/me"
    }

    companion object {
        private val JSON_MEDIA_TYPE = "application/json; charset=utf-8".toMediaType()
        val DefaultJson: Json = Json {
            ignoreUnknownKeys = true
            explicitNulls = false
            encodeDefaults = false
        }
    }
}
