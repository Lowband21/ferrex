package com.ferrex.android.core.library

import ferrex.ids.Uuid
import java.util.UUID

/**
 * Extension functions for converting between FlatBuffers [Uuid] struct
 * and Java [UUID]. The FlatBuffers UUID is stored as 16 raw bytes (b0–b15)
 * in big-endian (RFC 4122) byte order.
 */

/**
 * Convert a FlatBuffers Uuid struct to a Java UUID.
 */
fun Uuid.toJavaUuid(): UUID {
    val msb = (b0.toLong() and 0xFF shl 56) or
        (b1.toLong() and 0xFF shl 48) or
        (b2.toLong() and 0xFF shl 40) or
        (b3.toLong() and 0xFF shl 32) or
        (b4.toLong() and 0xFF shl 24) or
        (b5.toLong() and 0xFF shl 16) or
        (b6.toLong() and 0xFF shl 8) or
        (b7.toLong() and 0xFF)

    val lsb = (b8.toLong() and 0xFF shl 56) or
        (b9.toLong() and 0xFF shl 48) or
        (b10.toLong() and 0xFF shl 40) or
        (b11.toLong() and 0xFF shl 32) or
        (b12.toLong() and 0xFF shl 24) or
        (b13.toLong() and 0xFF shl 16) or
        (b14.toLong() and 0xFF shl 8) or
        (b15.toLong() and 0xFF)

    return UUID(msb, lsb)
}

/**
 * Convert a FlatBuffers Uuid struct to its string representation.
 * More convenient than toJavaUuid() when you just need the string.
 */
fun Uuid.toUuidString(): String = toJavaUuid().toString()
