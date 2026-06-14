package com.ferrex.android.core.api

import kotlinx.coroutines.test.runTest
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import okhttp3.OkHttpClient
import okhttp3.mockwebserver.MockResponse
import okhttp3.mockwebserver.MockWebServer
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class FerrexApiClientAuthTest {
    @Test
    fun devicePasswordLoginPostsAndroidPlatformMetadata() = runTest {
        MockWebServer().use { server ->
            server.enqueue(
                MockResponse().setResponseCode(200).setBody(
                    """{"status":"success","data":{"access_token":"access","refresh_token":"refresh"}}""",
                ),
            )
            server.start()

            val result = client(server).devicePasswordLogin(
                username = "grayson",
                password = "password",
                deviceInfo = deviceInfo(),
                rememberDevice = false,
            )

            assertTrue(result is ApiResult.Success)
            val request = server.takeRequest()
            assertEquals("POST", request.method)
            assertEquals("/api/v1/auth/device/login", request.path)
            val postedDeviceInfo = Json.parseToJsonElement(request.body.readUtf8())
                .jsonObject
                .getValue("device_info")
                .jsonObject
            assertEquals(
                "018f5f8d-0000-7000-8000-000000000001",
                postedDeviceInfo.getValue("device_id").jsonPrimitive.content,
            )
            assertEquals("android", postedDeviceInfo.getValue("platform").jsonPrimitive.content)
        }
    }

    @Test
    fun knownDeviceUsersPostsAndroidPlatformMetadata() = runTest {
        MockWebServer().use { server ->
            server.enqueue(
                MockResponse().setResponseCode(200).setBody(
                    """{"status":"success","data":{"known_device":false,"users":[]}}""",
                ),
            )
            server.start()

            val result = client(server).knownDeviceUsers(deviceInfo())

            assertTrue(result is ApiResult.Success)
            val request = server.takeRequest()
            assertEquals("POST", request.method)
            assertEquals("/api/v1/auth/device/users", request.path)
            val postedDeviceInfo = Json.parseToJsonElement(request.body.readUtf8())
                .jsonObject
                .getValue("device_info")
                .jsonObject
            assertEquals("android", postedDeviceInfo.getValue("platform").jsonPrimitive.content)
        }
    }

    private fun client(server: MockWebServer): FerrexApiClient {
        val config = ServerConfig().apply { setUrl(server.url("/").toString()) }
        return FerrexApiClient(OkHttpClient(), config)
    }

    private fun deviceInfo(): DeviceInfo = DeviceInfo(
        deviceId = "018f5f8d-0000-7000-8000-000000000001",
        deviceName = "Test Android",
        appVersion = "test",
    )
}
