package com.ferrex.android.core.playback

import androidx.media3.common.C
import androidx.media3.common.Format
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class PlaybackTrackOptionsTest {
    @Test
    fun audioOptionsFormatLabelsLanguagesAndCapabilityWarningsAcrossGroups() {
        val groups = listOf(
            trackGroup(
                groupIndex = 0,
                type = C.TRACK_TYPE_AUDIO,
                tracks = listOf(
                    track(
                        trackIndex = 0,
                        label = "Director Commentary",
                        language = "eng",
                        sampleMimeType = "audio/eac3",
                        channelCount = 6,
                        sampleRate = 48_000,
                        bitrate = 768_000,
                        roleFlags = C.ROLE_FLAG_COMMENTARY,
                        support = C.FORMAT_HANDLED,
                        selected = true,
                    ),
                ),
            ),
            trackGroup(
                groupIndex = 1,
                type = C.TRACK_TYPE_AUDIO,
                tracks = listOf(
                    track(
                        trackIndex = 0,
                        language = "und",
                        sampleMimeType = "audio/ac3",
                        channelCount = 8,
                        support = C.FORMAT_EXCEEDS_CAPABILITIES,
                    ),
                    track(
                        trackIndex = 1,
                        label = "Legacy DTS",
                        sampleMimeType = "audio/vnd.dts",
                        support = C.FORMAT_UNSUPPORTED_SUBTYPE,
                    ),
                ),
            ),
        )

        val options = PlaybackTrackOptions.buildOptions(groups, C.TRACK_TYPE_AUDIO)

        assertEquals(3, options.size)
        assertEquals("English • Director Commentary", options[0].title)
        assertTrue(options[0].details!!.contains("5.1"))
        assertTrue(options[0].details!!.contains("E-AC-3"))
        assertTrue(options[0].details!!.contains("Commentary"))
        assertTrue(options[0].selected)
        assertTrue(options[0].selectable)

        assertEquals("Audio 2", options[1].title)
        assertFalse("exceeds-capability audio is warned but not marked fully supported", options[1].supported)
        assertTrue("exceeds-capability audio remains selectable for device downmix/fallback", options[1].selectable)
        assertTrue(options[1].details!!.contains("May exceed device capabilities"))

        assertEquals("Legacy DTS", options[2].title)
        assertFalse(options[2].supported)
        assertFalse(options[2].selectable)
        assertTrue(options[2].details!!.contains("Unsupported format"))
    }

    @Test
    fun audioOptionsAreEmptyWhenNoAudioTracksExist() {
        val options = PlaybackTrackOptions.buildOptions(emptyList(), C.TRACK_TYPE_AUDIO)

        assertTrue(options.isEmpty())
    }

    @Test
    fun subtitleOptionsAlwaysOfferOffAndFormatRolesAndFallbacks() {
        val groups = listOf(
            trackGroup(
                groupIndex = 0,
                type = C.TRACK_TYPE_TEXT,
                tracks = listOf(
                    track(
                        trackIndex = 0,
                        language = "spa",
                        sampleMimeType = "application/x-subrip",
                        selectionFlags = C.SELECTION_FLAG_DEFAULT or C.SELECTION_FLAG_FORCED,
                        roleFlags = C.ROLE_FLAG_CAPTION,
                        support = C.FORMAT_HANDLED,
                        selected = true,
                    ),
                    track(
                        trackIndex = 1,
                        language = "und",
                        sampleMimeType = "application/pgs",
                        support = C.FORMAT_UNSUPPORTED_TYPE,
                    ),
                ),
            ),
        )

        val options = PlaybackTrackOptions.buildOptions(groups, C.TRACK_TYPE_TEXT)

        assertEquals("Off", options[0].title)
        assertTrue(options[0].isOff)
        assertFalse(options[0].selected)
        assertEquals("Spanish", options[1].title)
        assertTrue(options[1].selected)
        assertTrue(options[1].details!!.contains("Default"))
        assertTrue(options[1].details!!.contains("Forced"))
        assertTrue(options[1].details!!.contains("Captions"))
        assertEquals("Subtitle 2", options[2].title)
        assertFalse(options[2].selectable)
        assertTrue(options[2].details!!.contains("Unsupported type"))
    }

    @Test
    fun subtitleOffIsSelectedWhenTextDisabledOrNoTextTracksExist() {
        val selectedWhenDisabled = PlaybackTrackOptions.buildOptions(
            groups = listOf(
                trackGroup(
                    groupIndex = 0,
                    type = C.TRACK_TYPE_TEXT,
                    tracks = listOf(track(trackIndex = 0, support = C.FORMAT_HANDLED, selected = true)),
                ),
            ),
            trackType = C.TRACK_TYPE_TEXT,
            disabledTrackTypes = setOf(C.TRACK_TYPE_TEXT),
        ).single { it.isOff }

        val onlyOff = PlaybackTrackOptions.buildOptions(emptyList(), C.TRACK_TYPE_TEXT)

        assertTrue(selectedWhenDisabled.selected)
        assertEquals(1, onlyOff.size)
        assertTrue(onlyOff.single().isOff)
        assertTrue(onlyOff.single().selected)
    }

    @Test
    fun diagnosticsSummarizeTrackGroupsWithoutUrls() {
        val summary = PlaybackTrackOptions.describeTracksForDiagnostics(
            listOf(
                trackGroup(0, C.TRACK_TYPE_VIDEO, listOf(track(0, support = C.FORMAT_HANDLED, selected = true))),
                trackGroup(1, C.TRACK_TYPE_AUDIO, listOf(track(0, support = C.FORMAT_EXCEEDS_CAPABILITIES))),
                trackGroup(2, C.TRACK_TYPE_TEXT, listOf(track(0, support = C.FORMAT_HANDLED))),
            ),
        )

        assertTrue(summary.contains("videoGroups=1"))
        assertTrue(summary.contains("audioGroups=1"))
        assertTrue(summary.contains("exceeds=1"))
        assertTrue(summary.contains("textGroups=1"))
        assertFalse(summary.contains("http://"))
        assertFalse(summary.contains("access_token"))
    }

    private fun trackGroup(
        groupIndex: Int,
        type: Int,
        tracks: List<PlaybackTrackSnapshot>,
    ): PlaybackTrackGroupSnapshot = PlaybackTrackGroupSnapshot(
        groupIndex = groupIndex,
        groupKey = "group-$groupIndex",
        type = type,
        tracks = tracks,
    )

    private fun track(
        trackIndex: Int,
        label: String? = null,
        language: String? = null,
        sampleMimeType: String? = null,
        channelCount: Int = Format.NO_VALUE,
        sampleRate: Int = Format.NO_VALUE,
        bitrate: Int = Format.NO_VALUE,
        roleFlags: Int = 0,
        selectionFlags: Int = 0,
        support: Int = C.FORMAT_HANDLED,
        selected: Boolean = false,
    ): PlaybackTrackSnapshot = PlaybackTrackSnapshot(
        trackIndex = trackIndex,
        label = label,
        language = language,
        sampleMimeType = sampleMimeType,
        channelCount = channelCount,
        sampleRate = sampleRate,
        bitrate = bitrate,
        roleFlags = roleFlags,
        selectionFlags = selectionFlags,
        support = support,
        selected = selected,
    )
}
