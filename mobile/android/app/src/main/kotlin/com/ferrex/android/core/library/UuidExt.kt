package com.ferrex.android.core.library

import com.google.flatbuffers.FlatBufferBuilder
import ferrex.ids.Uuid
import java.util.UUID

fun Uuid.toJavaUuid(): UUID {
    val msb = (b0.toLongByte() shl 56) or
        (b1.toLongByte() shl 48) or
        (b2.toLongByte() shl 40) or
        (b3.toLongByte() shl 32) or
        (b4.toLongByte() shl 24) or
        (b5.toLongByte() shl 16) or
        (b6.toLongByte() shl 8) or
        b7.toLongByte()

    val lsb = (b8.toLongByte() shl 56) or
        (b9.toLongByte() shl 48) or
        (b10.toLongByte() shl 40) or
        (b11.toLongByte() shl 32) or
        (b12.toLongByte() shl 24) or
        (b13.toLongByte() shl 16) or
        (b14.toLongByte() shl 8) or
        b15.toLongByte()

    return UUID(msb, lsb)
}

fun Uuid.toUuidString(): String = toJavaUuid().toString()

fun String.toJavaUuidOrNull(): UUID? = runCatching { UUID.fromString(trim()) }.getOrNull()

fun UUID.toFlatBufferUuid(builder: FlatBufferBuilder): Int = Uuid.createUuid(
    builder,
    byteAt(56),
    byteAt(48),
    byteAt(40),
    byteAt(32),
    byteAt(24),
    byteAt(16),
    byteAt(8),
    byteAt(0),
    leastByteAt(56),
    leastByteAt(48),
    leastByteAt(40),
    leastByteAt(32),
    leastByteAt(24),
    leastByteAt(16),
    leastByteAt(8),
    leastByteAt(0),
)

private fun UByte.toLongByte(): Long = toLong() and 0xFFL

private fun UUID.byteAt(shift: Int): UByte = ((mostSignificantBits ushr shift) and 0xFFL).toUByte()

private fun UUID.leastByteAt(shift: Int): UByte = ((leastSignificantBits ushr shift) and 0xFFL).toUByte()
