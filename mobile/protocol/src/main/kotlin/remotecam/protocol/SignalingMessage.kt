package remotecam.protocol

import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.doubleOrNull
import kotlinx.serialization.json.longOrNull
import kotlinx.serialization.encodeToString
import kotlinx.serialization.decodeFromString
import kotlinx.serialization.Serializable
import kotlinx.serialization.serializer

/** Requested capture limits. Every field is optional; supplied values are range-checked. */
data class Limits(
    val maxWidth: Int? = null,
    val maxHeight: Int? = null,
    val maxFrameRate: Int? = null,
    val maxVideoBitrateKbps: Int? = null,
    val maxAudioBitrateKbps: Int? = null,
)

/** One ICE candidate. Only UDP host candidates with a private address are valid. */
data class Candidate(
    val address: String,
    val port: Int,
    val component: Int,
    val protocol: String?,
)

/** Optional client metrics. Carries no identifiers. */
@Serializable
data class Stats(
    val fps: Double? = null,
    val videoBitrateKbps: Double? = null,
    val rttMs: Double? = null,
    val lossPct: Double? = null,
    val thermalStatus: String? = null,
)

/** A validated signaling message. */
sealed class SignalingMessage {
    // Simplified API for the F0 spike
    @Serializable
    data class Pair(
        val token: String,
        val clientFingerprint: String,
    ) : SignalingMessage() {
        override val type = "pair"
    }

    @Serializable
    data class Paired(
        val sessionId: String,
    ) : SignalingMessage() {
        override val type = "paired"
    }

    @Serializable
    data class Offer(
        val sdp: String,
    ) : SignalingMessage() {
        override val type = "offer"
    }

    @Serializable
    data class Answer(
        val sdp: String,
    ) : SignalingMessage() {
        override val type = "answer"
    }

    @Serializable
    data class Ice(
        val candidate: String,
        val sdpMid: String,
        val sdpMLineIndex: Int,
    ) : SignalingMessage() {
        override val type = "ice"
    }

    @Serializable
    data class StatsMessage(val stats: Stats) : SignalingMessage() {
        override val type = "stats"
    }

    @Serializable
    data class Close(val code: ReasonCode) : SignalingMessage() {
        override val type = "close"
    }

    @Serializable
    data class Error(val code: ReasonCode, val message: String?) : SignalingMessage() {
        override val type = "error"
    }

    @Serializable
    data object Ping : SignalingMessage() {
        override val type = "ping"
    }

    @Serializable
    data object Pong : SignalingMessage() {
        override val type = "pong"
    }

    // Abstract property for message type
    abstract val type: String

    /** Serializes this message as a JSON string. */
    fun toJson(): String = encodeToString(this)

    /** Parses a JSON string into a SignalingMessage. */
    companion object {
        fun parse(json: String): SignalingMessage = decodeFromString(json)
    }
}

/** Result of parsing one signaling frame for conformance testing. */
sealed interface SignalingParseResult {
    data class Accepted(val message: SignalingMessage) : SignalingParseResult
    data class Rejected(val reason: ReasonCode) : SignalingParseResult
}

/** Strict parser for the signaling allowlist. Unknown fields are rejected, never ignored. */
object SignalingParser {
    const val MAX_MESSAGE_BYTES: Int = 16 * 1024

    private val messageTypes = setOf(
        "pair", "paired", "offer", "answer", "ice", "stats", "close", "error", "ping", "pong",
    )
    private val closeCodes = setOf(
        ReasonCode.ENDED_BY_MOBILE,
        ReasonCode.ENDED_BY_DESKTOP,
        ReasonCode.SUPERSEDED,
        ReasonCode.TIMEOUT,
        ReasonCode.MEDIA_FAILURE,
        ReasonCode.SECURITY_VIOLATION,
        ReasonCode.INTERNAL_ERROR,
    )
    private val errorCodes = setOf(
        ReasonCode.INVALID_MESSAGE,
        ReasonCode.UNSUPPORTED_VERSION,
        ReasonCode.RATE_LIMITED,
        ReasonCode.INTERNAL_ERROR,
    )
    private val thermalStatuses = setOf(
        "none", "light", "moderate", "severe", "critical", "emergency", "shutdown",
    )

    fun parse(frame: String): SignalingParseResult {
        if (frame.encodeToByteArray().size > MAX_MESSAGE_BYTES) return rejected()
        val element = try {
            Json.parseToJsonElement(frame)
        } catch (_: Exception) {
            return rejected()
        }
        val root = element as? JsonObject ?: return rejected()

        val version = root.requiredLong("version") ?: return rejected()
        when {
            version == 1L -> Unit
            version > 1L -> return SignalingParseResult.Rejected(ReasonCode.UNSUPPORTED_VERSION)
            else -> return rejected()
        }
        val type = root.string("type") ?: return rejected()
        if (type !in messageTypes) return rejected()

        return when (type) {
            "pair" -> {
                if (!root.keysAllowed("version", "type", "token", "client_fingerprint")) return rejected()
                val token = root.string("token") ?: return rejected()
                val fingerprint = root.string("client_fingerprint") ?: return rejected()
                SignalingParseResult.Accepted(SignalingMessage.Pair(token, fingerprint))
            }
            "paired" -> {
                if (!root.keysAllowed("version", "type", "session_id")) return rejected()
                val sessionId = root.string("session_id") ?: return rejected()
                SignalingParseResult.Accepted(SignalingMessage.Paired(sessionId))
            }
            "offer", "answer" -> {
                if (!root.keysAllowed("version", "type", "sdp")) return rejected()
                val sdp = root.string("sdp") ?: return rejected()
                val length = sdp.codePointCount()
                if (length !in 1..8192) return rejected()
                val message = if (type == "offer") {
                    SignalingMessage.Offer(sdp)
                } else {
                    SignalingMessage.Answer(sdp)
                }
                SignalingParseResult.Accepted(message)
            }
            "ice" -> {
                if (!root.keysAllowed("version", "type", "candidate", "sdp_mid", "sdp_mline_index")) return rejected()
                val candidate = root.string("candidate") ?: return rejected()
                val sdpMid = root.string("sdp_mid") ?: return rejected()
                val sdpMLineIndex = root.requiredLong("sdp_mline_index")?.takeIf { it in 0..65535 }?.toInt() ?: return rejected()
                SignalingParseResult.Accepted(SignalingMessage.Ice(candidate, sdpMid, sdpMLineIndex))
            }
            "stats" -> {
                if (!root.keysAllowed(
                        "version", "type", "fps", "video_bitrate_kbps", "rtt_ms", "loss_pct", "thermal_status",
                    )
                ) {
                    return rejected()
                }
                val stats = parseStats(root) ?: return rejected()
                SignalingParseResult.Accepted(SignalingMessage.StatsMessage(stats))
            }
            "close" -> {
                if (!root.keysAllowed("version", "type", "code")) return rejected()
                val code = root.string("code")?.let(ReasonCode::fromWire) ?: return rejected()
                if (code !in closeCodes) return rejected()
                SignalingParseResult.Accepted(SignalingMessage.Close(code))
            }
            "error" -> {
                if (!root.keysAllowed("version", "type", "code", "message")) return rejected()
                val code = root.string("code")?.let(ReasonCode::fromWire) ?: return rejected()
                if (code !in errorCodes) return rejected()
                val text = root["message"]?.let { element ->
                    val primitive = element as? JsonPrimitive ?: return rejected()
                    if (!primitive.isString) return rejected()
                    val content = primitive.content
                    if (content.codePointCount() > 200) return rejected()
                    content
                }
                SignalingParseResult.Accepted(SignalingMessage.Error(code, text))
            }
            "ping" -> {
                if (!root.keysAllowed("version", "type")) return rejected()
                SignalingParseResult.Accepted(SignalingMessage.Ping)
            }
            "pong" -> {
                if (!root.keysAllowed("version", "type")) return rejected()
                SignalingParseResult.Accepted(SignalingMessage.Pong)
            }
            else -> rejected()
        }
    }

    private fun parseStats(map: JsonObject): Stats? {
        val thermal = map["thermal_status"]?.let { value ->
            val primitive = value as? JsonPrimitive ?: return null
            if (!primitive.isString || primitive.content !in thermalStatuses) return null
            primitive.content
        }
        return try {
            Stats(
                fps = map.optionalDouble("fps", 0.0, 120.0),
                videoBitrateKbps = map.optionalDouble("video_bitrate_kbps", 0.0, Double.MAX_VALUE),
                rttMs = map.optionalDouble("rtt_ms", 0.0, Double.MAX_VALUE),
                lossPct = map.optionalDouble("loss_pct", 0.0, 100.0),
                thermalStatus = thermal,
            )
        } catch (_: InvalidField) {
            null
        }
    }

    private fun rejected() = SignalingParseResult.Rejected(ReasonCode.INVALID_MESSAGE)

    private class InvalidField : Exception()

    private fun JsonObject.keysAllowed(vararg allowed: String): Boolean =
        keys.all { it in allowed }

    private fun JsonObject.string(key: String): String? {
        val primitive = this[key] as? JsonPrimitive ?: return null
        return if (primitive.isString) primitive.content else null
    }

    private fun JsonObject.requiredLong(key: String): Long? {
        val primitive = this[key] as? JsonPrimitive ?: return null
        if (primitive.isString) return null
        return primitive.longOrNull
    }

    private fun JsonObject.optionalDouble(key: String, minimum: Double, maximum: Double): Double? {
        if (!containsKey(key)) return null
        val primitive = this[key] as? JsonPrimitive ?: throw InvalidField()
        if (primitive.isString) throw InvalidField()
        val value = primitive.doubleOrNull ?: throw InvalidField()
        if (value < minimum || value > maximum) throw InvalidField()
        return value
    }

    private fun String.codePointCount(): Int = codePointCount(0, length)
}