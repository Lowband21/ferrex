package com.ferrex.android.core.auth

import android.content.Context
import android.content.SharedPreferences
import androidx.security.crypto.EncryptedSharedPreferences
import androidx.security.crypto.MasterKey

class EncryptedAuthStorage(context: Context) : AuthStorage {
    private val appContext = context.applicationContext

    private val prefs: SharedPreferences by lazy {
        val masterKey = MasterKey.Builder(appContext)
            .setKeyScheme(MasterKey.KeyScheme.AES256_GCM)
            .build()

        EncryptedSharedPreferences.create(
            appContext,
            PREFS_NAME,
            masterKey,
            EncryptedSharedPreferences.PrefKeyEncryptionScheme.AES256_SIV,
            EncryptedSharedPreferences.PrefValueEncryptionScheme.AES256_GCM,
        )
    }

    override var serverUrl: String?
        get() = prefs.getString(KEY_SERVER_URL, null)
        set(value) = putNullable(KEY_SERVER_URL, value)

    override var accessToken: String?
        get() = prefs.getString(KEY_ACCESS_TOKEN, null)
        set(value) = putNullable(KEY_ACCESS_TOKEN, value)

    override var refreshToken: String?
        get() = prefs.getString(KEY_REFRESH_TOKEN, null)
        set(value) = putNullable(KEY_REFRESH_TOKEN, value)

    override var username: String?
        get() = prefs.getString(KEY_USERNAME, null)
        set(value) = putNullable(KEY_USERNAME, value)

    override var userId: String?
        get() = prefs.getString(KEY_USER_ID, null)
        set(value) = putNullable(KEY_USER_ID, value)

    override var userDisplayName: String?
        get() = prefs.getString(KEY_USER_DISPLAY_NAME, null)
        set(value) = putNullable(KEY_USER_DISPLAY_NAME, value)

    override var userAvatarUrl: String?
        get() = prefs.getString(KEY_USER_AVATAR_URL, null)
        set(value) = putNullable(KEY_USER_AVATAR_URL, value)

    override var sessionId: String?
        get() = prefs.getString(KEY_SESSION_ID, null)
        set(value) = putNullable(KEY_SESSION_ID, value)

    override var deviceSessionId: String?
        get() = prefs.getString(KEY_DEVICE_SESSION_ID, null)
        set(value) = putNullable(KEY_DEVICE_SESSION_ID, value)

    override var localDeviceId: String?
        get() = prefs.getString(KEY_LOCAL_DEVICE_ID, null)
        set(value) = putNullable(KEY_LOCAL_DEVICE_ID, value)

    override var requiresPinSetup: Boolean
        get() = prefs.getBoolean(KEY_REQUIRES_PIN_SETUP, false)
        set(value) = prefs.edit().putBoolean(KEY_REQUIRES_PIN_SETUP, value).apply()

    override fun clearTokens() {
        prefs.edit()
            .remove(KEY_ACCESS_TOKEN)
            .remove(KEY_REFRESH_TOKEN)
            .remove(KEY_USERNAME)
            .remove(KEY_USER_ID)
            .remove(KEY_USER_DISPLAY_NAME)
            .remove(KEY_USER_AVATAR_URL)
            .remove(KEY_SESSION_ID)
            .remove(KEY_DEVICE_SESSION_ID)
            .remove(KEY_REQUIRES_PIN_SETUP)
            .apply()
    }

    override fun clearConnectionData() {
        prefs.edit()
            .remove(KEY_SERVER_URL)
            .remove(KEY_ACCESS_TOKEN)
            .remove(KEY_REFRESH_TOKEN)
            .remove(KEY_USERNAME)
            .remove(KEY_USER_ID)
            .remove(KEY_USER_DISPLAY_NAME)
            .remove(KEY_USER_AVATAR_URL)
            .remove(KEY_SESSION_ID)
            .remove(KEY_DEVICE_SESSION_ID)
            .remove(KEY_LOCAL_DEVICE_ID)
            .remove(KEY_REQUIRES_PIN_SETUP)
            .apply()
    }

    private fun putNullable(key: String, value: String?) {
        prefs.edit().apply {
            if (value == null) remove(key) else putString(key, value)
        }.apply()
    }

    companion object {
        private const val PREFS_NAME = "ferrex_secure_auth"
        private const val KEY_SERVER_URL = "server_url"
        private const val KEY_ACCESS_TOKEN = "access_token"
        private const val KEY_REFRESH_TOKEN = "refresh_token"
        private const val KEY_USERNAME = "username"
        private const val KEY_USER_ID = "user_id"
        private const val KEY_USER_DISPLAY_NAME = "user_display_name"
        private const val KEY_USER_AVATAR_URL = "user_avatar_url"
        private const val KEY_SESSION_ID = "session_id"
        private const val KEY_DEVICE_SESSION_ID = "device_session_id"
        private const val KEY_LOCAL_DEVICE_ID = "local_device_id"
        private const val KEY_REQUIRES_PIN_SETUP = "requires_pin_setup"
    }
}
