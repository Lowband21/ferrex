package com.ferrex.android.core.auth

import com.ferrex.android.core.api.AuthTokens

interface AuthStorage {
    var serverUrl: String?
    var accessToken: String?
    var refreshToken: String?
    var username: String?
    var userId: String?
    var userDisplayName: String?
    var userAvatarUrl: String?
    var sessionId: String?
    var deviceSessionId: String?
    var localDeviceId: String?
    var requiresPinSetup: Boolean

    fun storeTokens(tokens: AuthTokens, username: String?, userId: String?) {
        accessToken = tokens.accessToken
        refreshToken = tokens.refreshToken
        this.username = username
        this.userId = userId ?: tokens.userId
        sessionId = tokens.sessionId
        deviceSessionId = tokens.deviceSessionId
        requiresPinSetup = tokens.requiresPinSetup
    }

    fun clearTokens() {
        accessToken = null
        refreshToken = null
        username = null
        userId = null
        userDisplayName = null
        userAvatarUrl = null
        sessionId = null
        deviceSessionId = null
        requiresPinSetup = false
    }

    fun clearConnectionData() {
        serverUrl = null
        clearTokens()
        localDeviceId = null
    }
}
