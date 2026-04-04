package com.ferrex.android.core.api

import javax.inject.Inject
import javax.inject.Singleton

/**
 * Holds the current server URL. Populated from EncryptedSharedPreferences
 * on launch, updated when the user enters a new server URL.
 */
@Singleton
class ServerConfig @Inject constructor() {

    @Volatile
    var serverUrl: String = ""
        private set

    /** Returns true if a server URL has been configured. */
    val isConfigured: Boolean get() = serverUrl.isNotBlank()

    /**
     * Set the server URL. Normalizes by stripping trailing slashes.
     */
    fun setUrl(url: String) {
        serverUrl = url.trimEnd('/')
    }
}
