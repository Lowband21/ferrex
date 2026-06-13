package com.ferrex.android.core.di

import android.content.Context
import com.ferrex.android.BuildConfig
import com.ferrex.android.core.api.AuthInterceptor
import com.ferrex.android.core.api.FerrexApiClient
import com.ferrex.android.core.api.ServerConfig
import com.ferrex.android.core.api.TokenRefreshAuthenticator
import com.ferrex.android.core.auth.AuthManager
import com.ferrex.android.core.auth.EncryptedAuthStorage
import okhttp3.OkHttpClient
import java.util.concurrent.TimeUnit

class AndroidAuthDependencies(
    context: Context,
    deviceName: String,
) {
    val serverConfig = ServerConfig()
    val authInterceptor = AuthInterceptor()
    val tokenRefreshAuthenticator = TokenRefreshAuthenticator(serverConfig, authInterceptor)

    private val httpClient = OkHttpClient.Builder()
        .connectTimeout(10, TimeUnit.SECONDS)
        .readTimeout(20, TimeUnit.SECONDS)
        .writeTimeout(20, TimeUnit.SECONDS)
        .addInterceptor(authInterceptor)
        .authenticator(tokenRefreshAuthenticator)
        .build()

    private val storage = EncryptedAuthStorage(context)
    private val apiClient = FerrexApiClient(httpClient, serverConfig)

    val authManager = AuthManager(
        api = apiClient,
        storage = storage,
        serverConfig = serverConfig,
        authInterceptor = authInterceptor,
        tokenRefreshAuthenticator = tokenRefreshAuthenticator,
        deviceName = deviceName,
        appVersion = BuildConfig.VERSION_NAME,
    )
}
