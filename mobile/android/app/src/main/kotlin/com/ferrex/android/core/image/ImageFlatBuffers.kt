package com.ferrex.android.core.image

import com.ferrex.android.core.library.toFlatBufferUuid
import com.ferrex.android.core.library.toJavaUuidOrNull
import com.ferrex.android.core.library.toUuidString
import com.google.flatbuffers.FlatBufferBuilder
import ferrex.image.ImageManifestRequest
import ferrex.image.ImageManifestResponse
import ferrex.image.ImageQuery
import ferrex.image.ImageStatus
import java.nio.ByteBuffer
import java.nio.ByteOrder

@OptIn(ExperimentalUnsignedTypes::class)
object ImageFlatBuffers {
    fun validKeys(keys: Collection<ImageRequestKey>): List<ImageRequestKey> =
        keys.distinct().filter { it.iid.toJavaUuidOrNull() != null }

    fun buildManifestRequest(keys: Collection<ImageRequestKey>): ByteArray {
        val validKeys = validKeys(keys)
        val builder = FlatBufferBuilder(64 + validKeys.size * 32)
        val queryOffsets = validKeys.map { key ->
            val uuid = key.iid.toJavaUuidOrNull() ?: error("Invalid image iid: ${key.iid}")
            ImageQuery.startImageQuery(builder)
            ImageQuery.addCategory(builder, key.category.flatBufferValue)
            ImageQuery.addIid(builder, uuid.toFlatBufferUuid(builder))
            ImageQuery.endImageQuery(builder)
        }.toIntArray()
        val queries = ImageManifestRequest.createQueriesVector(builder, queryOffsets)
        val root = ImageManifestRequest.createImageManifestRequest(builder, queries)
        builder.finish(root)
        return builder.sizedByteArray()
    }

    fun parseManifestResponse(bytes: ByteArray): List<ImageManifestRecord> {
        val response = ImageManifestResponse.getRootAsImageManifestResponse(bytes.asFlatBuffer())
        return (0 until response.entriesLength).mapNotNull { index ->
            val entry = response.entries(index) ?: return@mapNotNull null
            val category = BrowseImageCategory.fromFlatBuffer(entry.category) ?: BrowseImageCategory.Poster
            val key = ImageRequestKey(entry.iid.toUuidString(), category)
            val status = when (entry.status) {
                ImageStatus.Ready -> {
                    val token = entry.token?.takeIf { it.isNotBlank() }
                        ?: return@mapNotNull ImageManifestRecord(
                            key,
                            ManifestImageStatus.Failed("Ready manifest entry did not include a blob token"),
                        )
                    ManifestImageStatus.Ready(token)
                }
                ImageStatus.Pending -> ManifestImageStatus.Pending(entry.retryAfterMillis.toLong())
                ImageStatus.Failed -> ManifestImageStatus.Failed(entry.failureReason ?: "Image is not available")
                else -> ManifestImageStatus.Failed("Unknown image manifest status ${entry.status}")
            }
            ImageManifestRecord(key, status)
        }
    }

    private fun ByteArray.asFlatBuffer(): ByteBuffer = ByteBuffer.wrap(this).order(ByteOrder.LITTLE_ENDIAN)
}
