package com.ferrex.android.core.playback

import androidx.media3.common.Tracks

@androidx.annotation.OptIn(androidx.media3.common.util.UnstableApi::class)
fun Tracks.toPlaybackTrackGroupSnapshots(): List<PlaybackTrackGroupSnapshot> = groups.mapIndexed { groupIndex, group ->
    PlaybackTrackGroupSnapshot(
        groupIndex = groupIndex,
        groupKey = group.mediaTrackGroup.id,
        type = group.type,
        tracks = (0 until group.length).map { trackIndex ->
            val format = group.getTrackFormat(trackIndex)
            PlaybackTrackSnapshot(
                trackIndex = trackIndex,
                formatId = format.id,
                label = format.label,
                language = format.language,
                sampleMimeType = format.sampleMimeType,
                containerMimeType = format.containerMimeType,
                codecs = format.codecs,
                channelCount = format.channelCount,
                sampleRate = format.sampleRate,
                bitrate = format.bitrate,
                averageBitrate = format.averageBitrate,
                peakBitrate = format.peakBitrate,
                roleFlags = format.roleFlags,
                selectionFlags = format.selectionFlags,
                support = group.getTrackSupport(trackIndex),
                selected = group.isTrackSelected(trackIndex),
            )
        },
    )
}
