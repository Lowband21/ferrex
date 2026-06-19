package com.ferrex.android.navigation

import androidx.compose.runtime.saveable.Saver
import com.ferrex.android.core.browse.MediaRouteArgs
import com.ferrex.android.core.playback.MediaRoutePersistence
import com.ferrex.android.core.playback.PlaybackRouteContract
import com.ferrex.android.core.playback.PlaybackRoutePersistence

internal val MediaRouteArgsSaver: Saver<MediaRouteArgs?, String> = Saver(
    save = { route -> route?.let(MediaRoutePersistence::encode) },
    restore = { encoded -> MediaRoutePersistence.decode(encoded) },
)

internal val PlaybackRouteContractSaver: Saver<PlaybackRouteContract?, String> = Saver(
    save = { route -> route?.let(PlaybackRoutePersistence::encode) },
    restore = { encoded -> PlaybackRoutePersistence.decode(encoded) },
)

internal fun <T : Enum<T>> enumNameSaver(
    values: List<T>,
    defaultValue: T,
): Saver<T, String> = Saver(
    save = { value -> value.name },
    restore = { saved -> values.firstOrNull { it.name == saved } ?: defaultValue },
)
