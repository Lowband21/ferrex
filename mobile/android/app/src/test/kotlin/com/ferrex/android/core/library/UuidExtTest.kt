package com.ferrex.android.core.library

import com.google.flatbuffers.FlatBufferBuilder
import ferrex.ids.Uuid
import ferrex.watch.WatchProgressUpdate
import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Test
import java.nio.ByteBuffer
import java.nio.ByteOrder
import java.util.UUID

class UuidExtTest {

    @Test
    fun `flatbuffer uuid converts to java uuid and string`() {
        val expected = UUID.fromString("00112233-4455-6677-8899-aabbccddeeff")
        val update = watchProgressUpdateFor(expected)

        val mediaId = update.mediaId

        assertEquals(expected, mediaId.toJavaUuid())
        assertEquals(expected.toString(), mediaId.toUuidString())
    }

    @Test
    fun `watch progress update uuid struct can be built inline safely`() {
        val expected = UUID.fromString("f81d4fae-7dec-11d0-a765-00a0c91e6bf6")
        val update = watchProgressUpdateFor(
            mediaId = expected,
            position = 42.5,
            duration = 3600.0,
        )

        assertEquals(expected.toString(), update.mediaId.toUuidString())
        assertEquals(42.5, update.position)
        assertEquals(3600.0, update.duration)
    }

    private fun watchProgressUpdateFor(
        mediaId: UUID,
        position: Double = 0.0,
        duration: Double = 0.0,
    ): WatchProgressUpdate {
        val builder = FlatBufferBuilder(128)
        WatchProgressUpdate.startWatchProgressUpdate(builder)
        WatchProgressUpdate.addMediaId(builder, createUuid(builder, mediaId))
        WatchProgressUpdate.addPosition(builder, position)
        WatchProgressUpdate.addDuration(builder, duration)
        val root = WatchProgressUpdate.endWatchProgressUpdate(builder)
        builder.finish(root)

        val buffer = ByteBuffer.wrap(builder.sizedByteArray()).order(ByteOrder.LITTLE_ENDIAN)
        return WatchProgressUpdate.getRootAsWatchProgressUpdate(buffer)
    }

    private fun createUuid(builder: FlatBufferBuilder, uuid: UUID): Int {
        val msb = uuid.mostSignificantBits
        val lsb = uuid.leastSignificantBits
        return Uuid.createUuid(
            builder,
            (msb ushr 56).toUByte(), (msb ushr 48).toUByte(),
            (msb ushr 40).toUByte(), (msb ushr 32).toUByte(),
            (msb ushr 24).toUByte(), (msb ushr 16).toUByte(),
            (msb ushr 8).toUByte(), msb.toUByte(),
            (lsb ushr 56).toUByte(), (lsb ushr 48).toUByte(),
            (lsb ushr 40).toUByte(), (lsb ushr 32).toUByte(),
            (lsb ushr 24).toUByte(), (lsb ushr 16).toUByte(),
            (lsb ushr 8).toUByte(), lsb.toUByte(),
        )
    }
}
