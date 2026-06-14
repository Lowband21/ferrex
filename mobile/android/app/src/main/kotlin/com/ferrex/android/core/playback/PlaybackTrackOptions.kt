@file:androidx.annotation.OptIn(androidx.media3.common.util.UnstableApi::class)

package com.ferrex.android.core.playback

import androidx.media3.common.C
import androidx.media3.common.Format
import java.util.Locale

/** Snapshot of one Media3 track group with only metadata needed by track picker UI/tests. */
data class PlaybackTrackGroupSnapshot(
    val groupIndex: Int,
    val groupKey: String,
    val type: Int,
    val tracks: List<PlaybackTrackSnapshot>,
)

/** Snapshot of one track from Player.currentTracks. */
data class PlaybackTrackSnapshot(
    val trackIndex: Int,
    val formatId: String? = null,
    val label: String? = null,
    val language: String? = null,
    val sampleMimeType: String? = null,
    val containerMimeType: String? = null,
    val codecs: String? = null,
    val channelCount: Int = Format.NO_VALUE,
    val sampleRate: Int = Format.NO_VALUE,
    val bitrate: Int = Format.NO_VALUE,
    val averageBitrate: Int = Format.NO_VALUE,
    val peakBitrate: Int = Format.NO_VALUE,
    val roleFlags: Int = 0,
    val selectionFlags: Int = 0,
    val support: Int = C.FORMAT_UNSUPPORTED_TYPE,
    val selected: Boolean = false,
)

data class PlaybackTrackOption(
    val key: String,
    val type: Int,
    val title: String,
    val details: String?,
    val groupIndex: Int?,
    val trackIndex: Int?,
    val selected: Boolean,
    val supported: Boolean,
    val selectable: Boolean,
    val isOff: Boolean = false,
)

object PlaybackTrackOptions {
    fun buildOptions(
        groups: List<PlaybackTrackGroupSnapshot>,
        trackType: Int,
        disabledTrackTypes: Set<Int> = emptySet(),
    ): List<PlaybackTrackOption> {
        val typeDisabled = disabledTrackTypes.contains(trackType)
        val options = mutableListOf<PlaybackTrackOption>()

        if (trackType == C.TRACK_TYPE_TEXT) {
            val anySelectedText = groups
                .asSequence()
                .filter { it.type == C.TRACK_TYPE_TEXT }
                .flatMap { it.tracks.asSequence() }
                .any { it.selected }
            options += PlaybackTrackOption(
                key = "text-off",
                type = C.TRACK_TYPE_TEXT,
                title = "Off",
                details = "Disable subtitle and text tracks",
                groupIndex = null,
                trackIndex = null,
                selected = typeDisabled || !anySelectedText,
                supported = true,
                selectable = true,
                isOff = true,
            )
        }

        var fallbackIndex = 0
        groups.forEach { group ->
            if (group.type != trackType) return@forEach
            group.tracks.forEach { track ->
                val selected = !typeDisabled && track.selected
                val supported = track.support == C.FORMAT_HANDLED
                val selectable = selected || supported || track.isExceedsCapabilitiesAudio(trackType)
                options += PlaybackTrackOption(
                    key = buildTrackOptionKey(trackType, group.groupIndex, track.trackIndex, group.groupKey, track.formatId),
                    type = trackType,
                    title = formatTrackTitle(track, trackType, fallbackIndex),
                    details = formatTrackDetails(track, trackType),
                    groupIndex = group.groupIndex,
                    trackIndex = track.trackIndex,
                    selected = selected,
                    supported = supported,
                    selectable = selectable,
                )
                fallbackIndex += 1
            }
        }

        return options
    }

    fun formatTrackTitle(track: PlaybackTrackSnapshot, trackType: Int, fallbackIndex: Int): String {
        val language = displayLanguage(track.language)
        val label = track.label?.trim()?.takeIf { it.isNotBlank() }
        return when {
            language != null && label != null && !label.contains(language, ignoreCase = true) -> "$language • $label"
            label != null -> label
            language != null -> language
            trackType == C.TRACK_TYPE_AUDIO -> "Audio ${fallbackIndex + 1}"
            else -> "Subtitle ${fallbackIndex + 1}"
        }
    }

    fun formatTrackDetails(track: PlaybackTrackSnapshot, trackType: Int): String? {
        val parts = mutableListOf<String>()

        when (trackType) {
            C.TRACK_TYPE_AUDIO -> {
                audioChannelLabel(track.channelCount)?.let(parts::add)
                sampleRateLabel(track.sampleRate)?.let(parts::add)
                codecLabel(track)?.let(parts::add)
                bitrateLabel(track)?.let(parts::add)
                parts += roleLabels(track, includeAudioRoles = true)
            }
            C.TRACK_TYPE_TEXT -> {
                parts += roleLabels(track, includeAudioRoles = false)
                codecLabel(track)?.let(parts::add)
            }
        }

        supportLabel(track.support)?.let(parts::add)
        return parts.distinct().joinToString(" • ").takeIf { it.isNotBlank() }
    }

    fun describeTracksForDiagnostics(groups: List<PlaybackTrackGroupSnapshot>): String {
        if (groups.isEmpty()) return "Tracks changed: no audio/text/video groups discovered"

        fun groupCount(type: Int): Int = groups.count { it.type == type }
        fun trackCount(type: Int): Int = groups.filter { it.type == type }.sumOf { it.tracks.size }
        fun supportedTrackCount(type: Int): Int = groups
            .filter { it.type == type }
            .sumOf { group -> group.tracks.count { it.support == C.FORMAT_HANDLED } }
        fun exceedsCount(type: Int): Int = groups
            .filter { it.type == type }
            .sumOf { group -> group.tracks.count { it.support == C.FORMAT_EXCEEDS_CAPABILITIES } }
        fun selectedTrackCount(type: Int): Int = groups
            .filter { it.type == type }
            .sumOf { group -> group.tracks.count { it.selected } }

        return buildString {
            append("Tracks changed:")
            append(" videoGroups=${groupCount(C.TRACK_TYPE_VIDEO)} video=${supportedTrackCount(C.TRACK_TYPE_VIDEO)}/${trackCount(C.TRACK_TYPE_VIDEO)} supported selected=${selectedTrackCount(C.TRACK_TYPE_VIDEO)}")
            append(" audioGroups=${groupCount(C.TRACK_TYPE_AUDIO)} audio=${supportedTrackCount(C.TRACK_TYPE_AUDIO)}/${trackCount(C.TRACK_TYPE_AUDIO)} supported")
            val audioExceeds = exceedsCount(C.TRACK_TYPE_AUDIO)
            if (audioExceeds > 0) append(" exceeds=$audioExceeds")
            append(" selected=${selectedTrackCount(C.TRACK_TYPE_AUDIO)}")
            append(" textGroups=${groupCount(C.TRACK_TYPE_TEXT)} text=${supportedTrackCount(C.TRACK_TYPE_TEXT)}/${trackCount(C.TRACK_TYPE_TEXT)} supported selected=${selectedTrackCount(C.TRACK_TYPE_TEXT)}")
        }
    }

    private fun PlaybackTrackSnapshot.isExceedsCapabilitiesAudio(trackType: Int): Boolean =
        trackType == C.TRACK_TYPE_AUDIO && support == C.FORMAT_EXCEEDS_CAPABILITIES

    private fun buildTrackOptionKey(
        trackType: Int,
        groupIndex: Int,
        trackIndex: Int,
        groupKey: String,
        formatId: String?,
    ): String = buildString {
        append(trackType)
        append(':')
        append(groupIndex)
        append(':')
        append(trackIndex)
        append(':')
        append(groupKey)
        append(':')
        append(formatId.orEmpty())
    }

    private fun displayLanguage(language: String?): String? {
        val raw = language
            ?.trim()
            ?.takeIf { it.isNotBlank() }
            ?.takeUnless { it.equals(C.LANGUAGE_UNDETERMINED, ignoreCase = true) }
            ?.takeUnless { it.equals("und", ignoreCase = true) }
            ?: return null

        ISO_639_2_LANGUAGE_NAMES[raw.lowercase(Locale.US)]?.let { return it }

        val normalized = raw.replace('_', '-')
        val locale = Locale.forLanguageTag(normalized)
        val displayName = locale.getDisplayName(Locale.getDefault())
            .takeIf { it.isNotBlank() }
            ?.takeUnless { it.equals(normalized, ignoreCase = true) }
            ?.takeUnless { it.equals(raw, ignoreCase = true) }

        return displayName?.titleCased() ?: raw.uppercase(Locale.US)
    }

    private fun String.titleCased(): String = replaceFirstChar { char ->
        if (char.isLowerCase()) char.titlecase(Locale.getDefault()) else char.toString()
    }

    private fun audioChannelLabel(channelCount: Int): String? = when (channelCount) {
        Format.NO_VALUE, 0 -> null
        1 -> "Mono"
        2 -> "Stereo"
        6 -> "5.1"
        8 -> "7.1"
        else -> "$channelCount ch"
    }

    private fun sampleRateLabel(sampleRate: Int): String? = sampleRate
        .takeIf { it != Format.NO_VALUE && it > 0 }
        ?.let { "${it / 1000} kHz" }

    private fun bitrateLabel(track: PlaybackTrackSnapshot): String? {
        val bitrate = listOf(track.bitrate, track.averageBitrate, track.peakBitrate)
            .firstOrNull { it != Format.NO_VALUE && it > 0 }
            ?: return null
        return "${bitrate / 1000} kbps"
    }

    private fun codecLabel(track: PlaybackTrackSnapshot): String? {
        val raw = track.sampleMimeType ?: track.codecs ?: track.containerMimeType ?: return null
        return when (raw.lowercase(Locale.US)) {
            "audio/ac3" -> "AC-3"
            "audio/eac3" -> "E-AC-3"
            "audio/eac3-joc" -> "Dolby Atmos"
            "audio/mp4a-latm" -> "AAC"
            "audio/mpeg" -> "MP3"
            "audio/opus" -> "Opus"
            "audio/flac" -> "FLAC"
            "audio/vorbis" -> "Vorbis"
            "audio/vnd.dts" -> "DTS"
            "audio/vnd.dts.hd" -> "DTS-HD"
            "text/vtt" -> "WebVTT"
            "application/x-subrip" -> "SRT"
            "text/x-ssa", "application/ssa" -> "ASS/SSA"
            "application/pgs" -> "PGS"
            "application/dvbsubs" -> "DVB subtitles"
            else -> raw.substringAfter('/').uppercase(Locale.US)
        }
    }

    private fun roleLabels(track: PlaybackTrackSnapshot, includeAudioRoles: Boolean): List<String> {
        val labels = mutableListOf<String>()
        val roles = track.roleFlags
        val selection = track.selectionFlags

        if (selection and C.SELECTION_FLAG_DEFAULT != 0) labels += "Default"
        if (selection and C.SELECTION_FLAG_FORCED != 0) labels += "Forced"

        if (includeAudioRoles) {
            if (roles and C.ROLE_FLAG_COMMENTARY != 0) labels += "Commentary"
            if (roles and C.ROLE_FLAG_DUB != 0) labels += "Dub"
            if (roles and C.ROLE_FLAG_DESCRIBES_VIDEO != 0) labels += "Audio description"
            if (roles and C.ROLE_FLAG_ALTERNATE != 0) labels += "Alternate"
        } else {
            if (roles and C.ROLE_FLAG_CAPTION != 0) labels += "Captions"
            if (roles and C.ROLE_FLAG_DESCRIBES_MUSIC_AND_SOUND != 0) labels += "SDH"
            if (roles and C.ROLE_FLAG_TRANSCRIBES_DIALOG != 0) labels += "Dialog"
            if (roles and C.ROLE_FLAG_COMMENTARY != 0) labels += "Commentary"
        }

        return labels
    }

    private fun supportLabel(support: Int): String? = when (support) {
        C.FORMAT_HANDLED -> null
        C.FORMAT_EXCEEDS_CAPABILITIES -> "May exceed device capabilities"
        C.FORMAT_UNSUPPORTED_DRM -> "Unsupported DRM"
        C.FORMAT_UNSUPPORTED_SUBTYPE -> "Unsupported format"
        C.FORMAT_UNSUPPORTED_TYPE -> "Unsupported type"
        else -> "Unsupported"
    }

    private val ISO_639_2_LANGUAGE_NAMES = mapOf(
        "ara" to "Arabic",
        "ces" to "Czech",
        "chi" to "Chinese",
        "cze" to "Czech",
        "dan" to "Danish",
        "deu" to "German",
        "dut" to "Dutch",
        "ell" to "Greek",
        "eng" to "English",
        "fin" to "Finnish",
        "fra" to "French",
        "fre" to "French",
        "ger" to "German",
        "gre" to "Greek",
        "heb" to "Hebrew",
        "hin" to "Hindi",
        "hun" to "Hungarian",
        "ita" to "Italian",
        "jpn" to "Japanese",
        "kor" to "Korean",
        "nld" to "Dutch",
        "nor" to "Norwegian",
        "pol" to "Polish",
        "por" to "Portuguese",
        "ron" to "Romanian",
        "rum" to "Romanian",
        "rus" to "Russian",
        "spa" to "Spanish",
        "swe" to "Swedish",
        "tha" to "Thai",
        "tur" to "Turkish",
        "vie" to "Vietnamese",
        "zho" to "Chinese",
    )
}
