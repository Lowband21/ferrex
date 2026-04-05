package com.ferrex.android.core.image

import okhttp3.Interceptor
import okhttp3.Response

/**
 * OkHttp interceptor that retries HTTP 202 responses for image requests.
 *
 * The server returns 202 Accepted when an image is being cached (e.g.,
 * the poster hasn't been downloaded/resized yet). This interceptor
 * waits briefly and retries, giving the server time to process the image.
 *
 * Only applies to requests targeting /images/iid/ endpoints. Other
 * requests pass through unchanged.
 */
class ImageRetryInterceptor : Interceptor {

    companion object {
        private const val MAX_RETRIES = 2
        private const val RETRY_DELAY_MS = 1500L
    }

    override fun intercept(chain: Interceptor.Chain): Response {
        val request = chain.request()

        // Only retry image IID requests
        if (!request.url.encodedPath.contains("/images/iid/")) {
            return chain.proceed(request)
        }

        var response = chain.proceed(request)
        var retries = 0

        while (response.code == 202 && retries < MAX_RETRIES) {
            response.close()
            Thread.sleep(RETRY_DELAY_MS)
            retries++
            response = chain.proceed(request)
        }

        return response
    }
}
