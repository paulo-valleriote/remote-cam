package remotecam.protocol

import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.doubleOrNull
import kotlinx.serialization.json.longOrNull

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
data class Stats(
    val fps: Double? = null,
    val videoBitrateKbps: Double? = null,
    val rttMs: Double? = null,
    val lossPct: Double? = null,
    val thermalStatus: String? = null,
)

/** A validated signaling message. */
sealed class SignalingMessage {
    data class Pair(
        val token: ByteArray,
        val dtlsFingerprint: ByteArray,
        val limits: Limits,
    ) : SignalingMessage() {
        override fun equals(other: Any?): Boolean =
            other is Pair && token.contentEquals(other.token) &&
                dtlsFingerprint.contentEquals(other.dtlsFingerprint) && limits == other.limits

        override fun hashCode(): Int = 31 * (31 * token.contentHashCode() + dtlsFingerprint.contentHashCode()) + limits.hashCode()
    }

    data class Paired(
        val sessionId: ByteArray,
        val epoch: Long,
        val dtlsFingerprint: ByteArray,
        val acceptedLimits: Limits,
    ) : SignalingMessage() {
        override fun equals(other: Any?): Boolean =
            other is Paired && sessionId.contentEquals(other.sessionId) && epoch == other.epoch &&
                dtlsFingerprint.contentEquals(other.dtlsFingerprint) &&
                acceptedLimits == other.acceptedLimits

        override fun hashCode(): Int = 31 * (31 * (31 * sessionId.contentHashCode() + epoch.hashCode()) + dtlsFingerprint.contentHashCode()) + acceptedLimits.hashCode()
    }

    data class Offer(val epoch: Long, val sdp: String) : SignalingMessage()
    data class Answer(val epoch: Long, val sdp: String) : SignalingMessage()
    data class Ice(val epoch: Long, val candidates: List<Candidate>) : SignalingMessage()
    data class StatsMessage(val stats: Stats) : SignalingMessage()
    data class Close(val code: ReasonCode) : SignalingMessage()
    data class Error(val code: ReasonCode, val message: String?) : SignalingMessage()
    data object Ping : SignalingMessage()
    data object Pong : SignalingMessage()
}

/** Result of parsing one signaling frame. */
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
                if (!root.keysAllowed("version", "type", "token", "dtls_fingerprint", "limits")) return rejected()
                val token = root.fixedHex("token", 32) ?: return rejected()
                val fingerprint = root.fixedHex("dtls_fingerprint", 32) ?: return rejected()
                val limits = parseLimits(root["limits"] ?: return rejected()) ?: return rejected()
                SignalingParseResult.Accepted(SignalingMessage.Pair(token, fingerprint, limits))
            }
            "paired" -> {
                if (!root.keysAllowed(
                        "version", "type", "session_id", "epoch", "dtls_fingerprint", "accepted_limits",
                    )
                ) {
                    return rejected()
                }
                val sessionId = root.fixedHex("session_id", 16) ?: return rejected()
                val epoch = root.requiredEpoch() ?: return rejected()
                val fingerprint = root.fixedHex("dtls_fingerprint", 32) ?: return rejected()
                val limits = parseLimits(root["accepted_limits"] ?: return rejected()) ?: return rejected()
                SignalingParseResult.Accepted(SignalingMessage.Paired(sessionId, epoch, fingerprint, limits))
            }
            "offer", "answer" -> {
                if (!root.keysAllowed("version", "type", "epoch", "sdp")) return rejected()
                val epoch = root.requiredEpoch() ?: return rejected()
                val sdp = root.string("sdp") ?: return rejected()
                val length = sdp.codePointCount()
                if (length !in 1..8192) return rejected()
                val message = if (type == "offer") {
                    SignalingMessage.Offer(epoch, sdp)
                } else {
                    SignalingMessage.Answer(epoch, sdp)
                }
                SignalingParseResult.Accepted(message)
            }
            "ice" -> {
                if (!root.keysAllowed("version", "type", "epoch", "candidates")) return rejected()
                val epoch = root.requiredEpoch() ?: return rejected()
                val candidates = parseCandidates(root["candidates"] ?: return rejected()) ?: return rejected()
                SignalingParseResult.Accepted(SignalingMessage.Ice(epoch, candidates))
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

    private fun parseLimits(element: JsonElement): Limits? {
        val map = element as? JsonObject ?: return null
        if (!map.keysAllowed(
                "max_width", "max_height", "max_frame_rate", "max_video_bitrate_kbps", "max_audio_bitrate_kbps",
            )
        ) {
            return null
        }
        return try {
            Limits(
                maxWidth = map.optionalInt("max_width", 160, 3840),
                maxHeight = map.optionalInt("max_height", 120, 2160),
                maxFrameRate = map.optionalInt("max_frame_rate", 1, 60),
                maxVideoBitrateKbps = map.optionalInt("max_video_bitrate_kbps", 200, 8000),
                maxAudioBitrateKbps = map.optionalInt("max_audio_bitrate_kbps", 8, 128),
            )
        } catch (_: InvalidField) {
            null
        }
    }

    private fun parseCandidates(element: JsonElement): List<Candidate>? {
        val array = element as? JsonArray ?: return null
        if (array.size > 64) return null
        return array.map { item ->
            val map = item as? JsonObject ?: return null
            if (!map.keysAllowed("address", "port", "component", "protocol")) return null
            val address = map.string("address") ?: return null
            if (Addresses.parsePrivate(address) == null) return null
            val port = map.requiredLong("port")?.takeIf { it in 1..65535 }?.toInt() ?: return null
            val component = map.requiredLong("component")?.takeIf { it in 1..2 }?.toInt() ?: return null
            val protocol = map["protocol"]?.let { value ->
                val primitive = value as? JsonPrimitive ?: return null
                if (!primitive.isString) return null
                val content = primitive.content
                if (content != "udp") return null
                content
            }
            Candidate(address, port, component, protocol)
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

    private fun JsonObject.requiredEpoch(): Long? {
        val value = requiredLong("epoch") ?: return null
        return if (value >= 0) value else null
    }

    private fun JsonObject.fixedHex(key: String, byteLength: Int): ByteArray? {
        val text = string(key) ?: return null
        return Hex.decodeExact(text, byteLength)
    }

    private fun JsonObject.optionalInt(key: String, minimum: Int, maximum: Int): Int? {
        if (!containsKey(key)) return null
        val value = requiredLong(key) ?: throw InvalidField()
        if (value < minimum || value > maximum) throw InvalidField()
        return value.toInt()
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
