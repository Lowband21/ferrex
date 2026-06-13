package com.ferrex.android.core.api

class ServerConfig {
    @Volatile
    var serverUrl: String = ""
        private set

    val isConfigured: Boolean get() = serverUrl.isNotBlank()

    fun setUrl(url: String) {
        serverUrl = normalize(url)
    }

    fun clear() {
        serverUrl = ""
    }

    fun requireUrl(): String {
        check(serverUrl.isNotBlank()) { "Server URL is not configured" }
        return serverUrl
    }

    companion object {
        fun normalize(url: String): String = url.trim().trimEnd('/')
    }
}
