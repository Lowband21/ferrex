package com.ferrex.android.core.image

import com.ferrex.android.core.library.ServerCacheScope
import com.ferrex.android.core.library.toJavaUuidOrNull
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch

/**
 * Pure Kotlin state exposed to phone and TV surfaces that render manifest-resolved artwork.
 *
 * The controller owns the retry loop for visible artwork so composables only need to publish
 * the current visible key window and collect this snapshot. Delays happen in this coroutine
 * layer, never inside OkHttp calls or interceptors.
 */
data class ImageResolutionControllerState(
    val scope: ServerCacheScope? = null,
    val visibleKeys: Set<ImageRequestKey> = emptySet(),
    val resolutions: Map<ImageRequestKey, ImageResolution> = emptyMap(),
    val resolving: Boolean = false,
    val scheduledRetryAtMillis: Long? = null,
) {
    fun resolutionFor(key: ImageRequestKey): ImageResolution? =
        resolutions[key] ?: resolutions.entries.firstOrNull { it.key.cacheKey == key.cacheKey }?.value
}

data class ImageResolutionRetryPlan(
    val delayMillis: Long,
    val keys: List<ImageRequestKey>,
)

data class ImageResolutionRetryPolicy(
    val failedOrMissingRetryDelayMillis: Long = DEFAULT_FAILED_OR_MISSING_RETRY_DELAY_MILLIS,
    val minimumPendingRetryDelayMillis: Long = DEFAULT_MINIMUM_PENDING_RETRY_DELAY_MILLIS,
) {
    init {
        require(failedOrMissingRetryDelayMillis >= 0L) { "failedOrMissingRetryDelayMillis must be non-negative" }
        require(minimumPendingRetryDelayMillis >= 0L) { "minimumPendingRetryDelayMillis must be non-negative" }
    }

    fun nextRetryDelayMillis(
        visibleKeys: Collection<ImageRequestKey>,
        resolutions: Map<ImageRequestKey, ImageResolution>,
        nowMillis: Long,
    ): Long? = nextRetryPlan(visibleKeys, resolutions, nowMillis)?.delayMillis

    fun nextRetryPlan(
        visibleKeys: Collection<ImageRequestKey>,
        resolutions: Map<ImageRequestKey, ImageResolution>,
        nowMillis: Long,
    ): ImageResolutionRetryPlan? {
        val candidates = visibleKeys.mapNotNull { key ->
            val resolution = resolutions[key] ?: resolutions.entries.firstOrNull { it.key.cacheKey == key.cacheKey }?.value
            retryDelayFor(key, resolution, nowMillis)?.let { delayMillis -> key to delayMillis }
        }
        val nextDelay = candidates.minOfOrNull { it.second } ?: return null
        val dueKeys = candidates.filter { it.second == nextDelay }.map { it.first }
        return ImageResolutionRetryPlan(delayMillis = nextDelay, keys = dueKeys)
    }

    private fun retryDelayFor(key: ImageRequestKey, resolution: ImageResolution?, nowMillis: Long): Long? {
        if (key.iid.toJavaUuidOrNull() == null) return null
        return when (resolution) {
            is ImageResolution.Pending -> maxOf(
                minimumPendingRetryDelayMillis,
                (resolution.retryAtMillis - nowMillis).coerceAtLeast(0L),
            )
            is ImageResolution.Failed -> if (resolution.retryable) failedOrMissingRetryDelayMillis else null
            is ImageResolution.Placeholder -> null
            is ImageResolution.Ready -> null
            null -> failedOrMissingRetryDelayMillis
        }
    }

    companion object {
        const val DEFAULT_FAILED_OR_MISSING_RETRY_DELAY_MILLIS: Long = 5_000L
        const val DEFAULT_MINIMUM_PENDING_RETRY_DELAY_MILLIS: Long = 250L
    }
}

class ImageResolutionController(
    private val resolver: ImageResolver,
    private val coroutineScope: CoroutineScope,
    private val retryPolicy: ImageResolutionRetryPolicy = ImageResolutionRetryPolicy(),
    private val clockMillis: () -> Long = { System.currentTimeMillis() },
) {
    private val _state = MutableStateFlow(ImageResolutionControllerState())
    val state: StateFlow<ImageResolutionControllerState> = _state.asStateFlow()

    private val readyCacheByScope = mutableMapOf<String, MutableMap<String, ImageResolution.Ready>>()
    private var activeRequest: VisibleImageRequest? = null
    private var activeJob: Job? = null

    @Volatile
    private var generation: Long = 0L

    fun setVisibleImages(scope: ServerCacheScope, visibleKeys: Collection<ImageRequestKey>) {
        val distinctKeys = visibleKeys.distinctBy { it.cacheKey }
        val request = VisibleImageRequest(scope, distinctKeys)
        if (request == activeRequest) return

        generation += 1
        val requestGeneration = generation
        activeRequest = request
        activeJob?.cancel()

        val retainedResolutions = mergeVisibleResolutions(
            scope = scope,
            visibleKeys = distinctKeys,
            current = _state.value.resolutions,
            updates = emptyMap(),
        )
        _state.value = ImageResolutionControllerState(
            scope = scope,
            visibleKeys = distinctKeys.toCollection(LinkedHashSet()),
            resolutions = retainedResolutions,
            resolving = distinctKeys.isNotEmpty(),
            scheduledRetryAtMillis = null,
        )

        if (distinctKeys.isEmpty()) {
            _state.value = _state.value.copy(resolving = false)
            return
        }

        activeJob = coroutineScope.launch {
            resolveThenRetry(request, requestGeneration)
        }
    }

    fun retryVisibleNow() {
        val request = activeRequest ?: return
        if (request.keys.isEmpty()) return

        generation += 1
        val requestGeneration = generation
        activeJob?.cancel()
        _state.value = _state.value.copy(resolving = true, scheduledRetryAtMillis = null)
        activeJob = coroutineScope.launch {
            retryThenSchedule(request, requestGeneration)
        }
    }

    fun clearCachedReady(scope: ServerCacheScope? = null) {
        if (scope == null) {
            readyCacheByScope.clear()
        } else {
            readyCacheByScope.remove(scope.directoryName)
        }
        val current = _state.value
        if (scope == null || current.scope?.directoryName == scope.directoryName) {
            _state.value = current.copy(
                resolutions = current.resolutions.filterValues { it !is ImageResolution.Ready },
                scheduledRetryAtMillis = null,
            )
        }
    }

    fun close() {
        generation += 1
        activeJob?.cancel()
        activeJob = null
        activeRequest = null
        readyCacheByScope.clear()
        _state.value = ImageResolutionControllerState()
    }

    private suspend fun resolveThenRetry(request: VisibleImageRequest, requestGeneration: Long) {
        try {
            val resolved = resolver.resolveImages(request.scope, request.keys)
            applyResult(request, requestGeneration, resolved)
            retryLoop(request, requestGeneration)
        } catch (e: CancellationException) {
            throw e
        } catch (_: Throwable) {
            markIdleIfCurrent(request, requestGeneration)
        }
    }

    private suspend fun retryThenSchedule(request: VisibleImageRequest, requestGeneration: Long) {
        try {
            val retried = resolver.retryPendingOrFailed(request.scope, request.keys)
            applyResult(request, requestGeneration, retried)
            retryLoop(request, requestGeneration)
        } catch (e: CancellationException) {
            throw e
        } catch (_: Throwable) {
            markIdleIfCurrent(request, requestGeneration)
        }
    }

    private suspend fun retryLoop(request: VisibleImageRequest, requestGeneration: Long) {
        while (isCurrent(request, requestGeneration)) {
            val nowMillis = clockMillis()
            val retryPlan = retryPolicy.nextRetryPlan(request.keys, _state.value.resolutions, nowMillis)
            if (retryPlan == null) {
                if (isCurrent(request, requestGeneration)) {
                    _state.value = _state.value.copy(resolving = false, scheduledRetryAtMillis = null)
                }
                return
            }

            if (isCurrent(request, requestGeneration)) {
                _state.value = _state.value.copy(
                    resolving = false,
                    scheduledRetryAtMillis = nowMillis + retryPlan.delayMillis,
                )
            }
            delay(retryPlan.delayMillis)
            if (!isCurrent(request, requestGeneration)) return
            _state.value = _state.value.copy(resolving = true, scheduledRetryAtMillis = null)
            val retried = resolver.retryPendingOrFailed(request.scope, retryPlan.keys)
            applyResult(request, requestGeneration, retried)
        }
    }

    private fun applyResult(
        request: VisibleImageRequest,
        requestGeneration: Long,
        result: Map<ImageRequestKey, ImageResolution>,
    ) {
        if (!isCurrent(request, requestGeneration)) return
        rememberReady(request.scope, result)
        _state.value = _state.value.copy(
            resolutions = mergeVisibleResolutions(
                scope = request.scope,
                visibleKeys = request.keys,
                current = _state.value.resolutions,
                updates = result,
            ),
            resolving = false,
            scheduledRetryAtMillis = null,
        )
    }

    private fun markIdleIfCurrent(request: VisibleImageRequest, requestGeneration: Long) {
        if (isCurrent(request, requestGeneration)) {
            _state.value = _state.value.copy(resolving = false, scheduledRetryAtMillis = null)
        }
    }

    private fun isCurrent(request: VisibleImageRequest, requestGeneration: Long): Boolean =
        generation == requestGeneration && activeRequest == request

    private fun rememberReady(scope: ServerCacheScope, result: Map<ImageRequestKey, ImageResolution>) {
        val readyByKey = readyCacheByScope.getOrPut(scope.directoryName) { linkedMapOf() }
        result.values.filterIsInstance<ImageResolution.Ready>().forEach { ready ->
            readyByKey[ready.key.cacheKey] = ready
        }
    }

    private fun mergeVisibleResolutions(
        scope: ServerCacheScope,
        visibleKeys: Collection<ImageRequestKey>,
        current: Map<ImageRequestKey, ImageResolution>,
        updates: Map<ImageRequestKey, ImageResolution>,
    ): Map<ImageRequestKey, ImageResolution> {
        if (visibleKeys.isEmpty()) return emptyMap()
        val currentByCacheKey = current.values.associateBy { it.key.cacheKey }
        val updatesByCacheKey = updates.values.associateBy { it.key.cacheKey }
        val readyByCacheKey = readyCacheByScope[scope.directoryName].orEmpty()
        return visibleKeys.mapNotNull { key ->
            val resolution = updates[key]
                ?: updatesByCacheKey[key.cacheKey]
                ?: current[key]
                ?: currentByCacheKey[key.cacheKey]
                ?: readyByCacheKey[key.cacheKey]
            resolution?.withRequestKey(key)?.let { key to it }
        }.toMap(LinkedHashMap())
    }

    private fun ImageResolution.withRequestKey(key: ImageRequestKey): ImageResolution = when (this) {
        is ImageResolution.Ready -> copy(key = key)
        is ImageResolution.Pending -> copy(key = key)
        is ImageResolution.Failed -> copy(key = key)
        is ImageResolution.Placeholder -> copy(key = key)
    }

    private data class VisibleImageRequest(
        val scope: ServerCacheScope,
        val keys: List<ImageRequestKey>,
    )
}
