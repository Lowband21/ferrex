package com.ferrex.android.core.api

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable

@Serializable
data class ApiEnvelope<T>(
    val status: String? = null,
    val data: T? = null,
    val error: String? = null,
    val message: String? = null,
)

@Serializable
data class SetupStatus(
    @SerialName("needs_setup") val needsSetup: Boolean = false,
    @SerialName("has_admin") val hasAdmin: Boolean = true,
    @SerialName("requires_setup_token") val requiresSetupToken: Boolean = false,
    @SerialName("pin_policy") val pinPolicy: PinPolicy = PinPolicy(),
    @SerialName("device_trust_policy") val deviceTrustPolicy: DeviceTrustPolicy = DeviceTrustPolicy(),
) {
    val canUsePasswordLogin: Boolean get() = hasAdmin && !needsSetup
}

@Serializable
data class PinPolicy(
    @SerialName("min_length") val minLength: Int = 4,
    @SerialName("max_length") val maxLength: Int = 12,
    @SerialName("require_numeric") val requireNumeric: Boolean = true,
    @SerialName("reject_repeated_digits") val rejectRepeatedDigits: Boolean = false,
    @SerialName("max_consecutive_identical") val maxConsecutiveIdentical: Int = 0,
    @SerialName("reject_sequential_digits") val rejectSequentialDigits: Boolean = false,
)

@Serializable
data class DeviceTrustPolicy(
    @SerialName("remember_device_default") val rememberDeviceDefault: Boolean = false,
    @SerialName("trust_duration_days") val trustDurationDays: Int = 0,
    @SerialName("pin_max_attempts") val pinMaxAttempts: Int = 0,
    @SerialName("pin_lockout_minutes") val pinLockoutMinutes: Int = 0,
    @SerialName("admin_pin_unlock_enabled") val adminPinUnlockEnabled: Boolean = false,
)

@Serializable
data class DeviceInfo(
    @SerialName("device_id") val deviceId: String,
    @SerialName("device_name") val deviceName: String,
    val platform: String = "android",
    @SerialName("app_version") val appVersion: String,
    @SerialName("hardware_id") val hardwareId: String? = null,
)

@Serializable
data class DevicePasswordLoginRequest(
    val username: String,
    val password: String,
    @SerialName("device_info") val deviceInfo: DeviceInfo,
    @SerialName("remember_device") val rememberDevice: Boolean = false,
)

@Serializable
data class KnownDeviceProfilesRequest(
    @SerialName("device_info") val deviceInfo: DeviceInfo,
)

@Serializable
data class KnownDeviceProfilesResponse(
    @SerialName("known_device") val knownDevice: Boolean = false,
    val users: List<KnownDeviceUserCard> = emptyList(),
)

@Serializable
data class KnownDeviceUserCard(
    val id: String,
    val username: String,
    @SerialName("display_name") val displayName: String,
    @SerialName("avatar_url") val avatarUrl: String? = null,
    @SerialName("has_pin") val hasPin: Boolean = false,
)

@Serializable
data class PinChallengeRequest(
    @SerialName("device_id") val deviceId: String,
)

@Serializable
data class PinChallengeResponse(
    @SerialName("challenge_id") val challengeId: String,
    val nonce: String,
    @SerialName("expires_in_secs") val expiresInSeconds: Long,
    @SerialName("pin_salt") val pinSalt: String,
)

@Serializable
data class PinLoginRequest(
    @SerialName("device_id") val deviceId: String,
    @SerialName("client_proof") val clientProof: String,
    @SerialName("challenge_id") val challengeId: String,
    @SerialName("device_signature") val deviceSignature: String,
)

@Serializable
data class AuthTokens(
    @SerialName("access_token") val accessToken: String,
    @SerialName("refresh_token") val refreshToken: String,
    @SerialName("expires_in") val expiresIn: Int = 0,
    @SerialName("session_id") val sessionId: String? = null,
    @SerialName("device_session_id") val deviceSessionId: String? = null,
    @SerialName("user_id") val userId: String? = null,
    @SerialName("requires_pin_setup") val requiresPinSetup: Boolean = false,
)

@Serializable
data class RefreshRequest(
    @SerialName("refresh_token") val refreshToken: String,
)

@Serializable
data class CurrentUser(
    val id: String,
    val username: String,
    @SerialName("display_name") val displayName: String? = null,
    @SerialName("avatar_url") val avatarUrl: String? = null,
    val email: String? = null,
)
