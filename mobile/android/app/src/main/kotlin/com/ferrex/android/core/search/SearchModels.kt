package com.ferrex.android.core.search

import kotlinx.serialization.KSerializer
import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable
import kotlinx.serialization.SerializationException
import kotlinx.serialization.descriptors.SerialDescriptor
import kotlinx.serialization.descriptors.buildClassSerialDescriptor
import kotlinx.serialization.encoding.Decoder
import kotlinx.serialization.encoding.Encoder
import kotlinx.serialization.json.JsonDecoder
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonEncoder
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.put

@Serializable
data class SearchMediaQuery(
    val filters: SearchMediaFilters = SearchMediaFilters(),
    val sort: SearchSortCriteria = SearchSortCriteria(),
    val search: SearchQuery,
    val pagination: SearchPagination,
    @SerialName("user_context") val userContext: String? = null,
)

@Serializable
data class SearchMediaFilters(
    @SerialName("media_type") val mediaType: String? = null,
    @SerialName("watch_status") val watchStatus: JsonElement? = null,
    val genres: List<String> = emptyList(),
    @SerialName("year_range") val yearRange: JsonElement? = null,
    @SerialName("rating_range") val ratingRange: JsonElement? = null,
    @SerialName("resolution_range") val resolutionRange: JsonElement? = null,
    @SerialName("library_ids") val libraryIds: List<String> = emptyList(),
)

@Serializable
data class SearchSortCriteria(
    val primary: String = "title",
    val order: String = "ascending",
    val secondary: String? = null,
)

@Serializable
data class SearchQuery(
    val text: String,
    val fields: List<String> = listOf("all"),
    val fuzzy: Boolean = true,
)

@Serializable
data class SearchPagination(
    val offset: Int = 0,
    val limit: Int = SearchLimits.DEFAULT,
)

@Serializable
data class SearchMediaWithStatus(
    val id: SearchMediaId,
    @SerialName("watch_status") val watchStatus: JsonElement? = null,
)

@Serializable(with = SearchMediaIdSerializer::class)
data class SearchMediaId(
    val type: SearchMediaType,
    val id: String,
)

enum class SearchMediaType(
    val jsonVariant: String,
    val routeSegment: String,
) {
    Movie("Movie", "movie"),
    Series("Series", "series"),
    Season("Season", "season"),
    Episode("Episode", "episode"),
    ;

    companion object {
        fun fromJsonVariant(value: String): SearchMediaType? = entries.firstOrNull {
            it.jsonVariant.equals(value, ignoreCase = true) || it.routeSegment.equals(value, ignoreCase = true)
        }

        fun fromRouteSegment(value: String?): SearchMediaType? = entries.firstOrNull {
            it.routeSegment.equals(value, ignoreCase = true) || it.jsonVariant.equals(value, ignoreCase = true)
        }
    }
}

object SearchLimits {
    const val DEFAULT = 50
    const val MAX = 100

    fun normalize(limit: Int): Int = when {
        limit <= 0 -> DEFAULT
        limit > MAX -> MAX
        else -> limit
    }
}

object SearchMediaIdSerializer : KSerializer<SearchMediaId> {
    override val descriptor: SerialDescriptor = buildClassSerialDescriptor("SearchMediaId")

    override fun deserialize(decoder: Decoder): SearchMediaId {
        val jsonDecoder = decoder as? JsonDecoder
            ?: throw SerializationException("SearchMediaId requires JSON decoding")
        return parse(jsonDecoder.decodeJsonElement())
            ?: throw SerializationException("MediaID must be an enum object with Movie, Series, Season, or Episode")
    }

    override fun serialize(encoder: Encoder, value: SearchMediaId) {
        val jsonEncoder = encoder as? JsonEncoder
            ?: throw SerializationException("SearchMediaId requires JSON encoding")
        jsonEncoder.encodeJsonElement(
            buildJsonObject {
                put(value.type.jsonVariant, value.id)
            },
        )
    }

    private fun parse(element: JsonElement): SearchMediaId? {
        val obj = element as? JsonObject ?: return null
        parseInternallyTagged(obj)?.let { return it }
        obj.entries.forEach { (variant, value) ->
            val type = SearchMediaType.fromJsonVariant(variant) ?: return@forEach
            val id = value.uuidStringOrNull() ?: return@forEach
            return SearchMediaId(type, id)
        }
        return null
    }

    private fun parseInternallyTagged(obj: JsonObject): SearchMediaId? {
        val typeName = obj["type"]?.uuidStringOrNull()
            ?: obj["media_type"]?.uuidStringOrNull()
            ?: obj["variant"]?.uuidStringOrNull()
            ?: return null
        val type = SearchMediaType.fromJsonVariant(typeName) ?: return null
        val id = obj["id"]?.uuidStringOrNull()
            ?: obj["uuid"]?.uuidStringOrNull()
            ?: obj["media_id"]?.uuidStringOrNull()
            ?: return null
        return SearchMediaId(type, id)
    }

    private fun JsonElement.uuidStringOrNull(): String? {
        (this as? JsonPrimitive)?.let { primitive ->
            return primitive.content.trim().takeIf { it.isNotEmpty() }
        }
        val obj = runCatching { jsonObject }.getOrNull() ?: return null
        return obj["id"]?.uuidStringOrNull()
            ?: obj["uuid"]?.uuidStringOrNull()
            ?: obj["value"]?.uuidStringOrNull()
            ?: obj["0"]?.uuidStringOrNull()
    }
}
