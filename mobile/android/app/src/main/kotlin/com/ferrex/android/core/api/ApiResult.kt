package com.ferrex.android.core.api

sealed interface ApiResult<out T> {
    data class Success<T>(val data: T) : ApiResult<T>
    data class HttpError(val code: Int, val message: String) : ApiResult<Nothing>
    data class ServerError(val message: String) : ApiResult<Nothing>
    data object EmptyBody : ApiResult<Nothing>
    data class ParseError(val message: String) : ApiResult<Nothing>
    data class NetworkError(val message: String) : ApiResult<Nothing>
}

inline fun <T, R> ApiResult<T>.mapSuccess(transform: (T) -> R): ApiResult<R> = when (this) {
    is ApiResult.Success -> ApiResult.Success(transform(data))
    is ApiResult.HttpError -> this
    is ApiResult.ServerError -> this
    ApiResult.EmptyBody -> ApiResult.EmptyBody
    is ApiResult.ParseError -> this
    is ApiResult.NetworkError -> this
}

fun ApiResult<*>.messageOrFallback(fallback: String): String = when (this) {
    is ApiResult.HttpError -> "Server returned $code: $message"
    is ApiResult.ServerError -> message
    ApiResult.EmptyBody -> "Server returned an empty response"
    is ApiResult.ParseError -> "Server response was not understood"
    is ApiResult.NetworkError -> message.ifBlank { fallback }
    is ApiResult.Success -> fallback
}
