package com.ferrex.android.core.browse

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class LibraryIndexTransportTest {
    @Test
    fun decodesRkyvArchivedIndicesResponseShape() {
        val bytes = byteArrayOf(
            0x01, 0x00, 0x00, 0x00,
            0x02, 0x00, 0x00, 0x00,
            0x03, 0x00, 0x00, 0x00,
            0x70, 0x11, 0x01, 0x00,
            0x01, 0x00, 0x00, 0x00,
            0xec.toByte(), 0xff.toByte(), 0xff.toByte(), 0xff.toByte(),
            0x04, 0x00, 0x00, 0x00,
        )

        val indices = OkHttpLibraryIndexTransport.decodeArchivedIndices(bytes).getOrThrow()

        assertEquals(listOf(1, 2, 3, 70_000), indices)
    }

    @Test
    fun rejectsOutOfBoundsArchivedIndicesResponse() {
        val corrupt = byteArrayOf(
            0x01, 0x00, 0x00, 0x00,
            0x7f, 0x00, 0x00, 0x00,
            0x03, 0x00, 0x00, 0x00,
        )

        assertTrue(OkHttpLibraryIndexTransport.decodeArchivedIndices(corrupt).isFailure)
    }
}
