package remotecam.protocol

import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.doubleOrNull
import kotlinx.serialization.json.longOrNull
import kotlinx.serialization.builtins.encodeToString
import kotlinx.serialization.builtins.decodeFromString
import kotlinx.serialization.Serializable

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
    // Simplified API for the F0 spike
    data class Pair(
        val token: String,
        val clientFingerprint: String,
    ) : SignalingMessage() {
        override val type = "pair"
    }

    data class Paired(
        val sessionId: String,
    ) : SignalingMessage() {
        override val type = "paired"
    }

    data class Offer(
        val sdp: String,
    ) : SignalingMessage() {
        override val type = "offer"
    }

    data class Answer(
        val sdp: String,
    ) : SignalingMessage() {
        override val type = "answer"
    }

    data class Ice(
        val candidate: String,
        val sdpMid: String,
        val sdpMLineIndex: Int,
    ) : SignalingMessage() {
        override val type = "ice"
    }

    data class StatsMessage(val stats: Stats) : SignalingMessage() {
        override val type = "stats"
    }

    data class Close(val code: ReasonCode) : SignalingMessage() {
        override val type = "close"
    }

    data class Error(val code: ReasonCode, val message: String?) : SignalingMessage() {
        override val type = "error"
    }

    data object Ping : SignalingMessage() {
        override val type = "ping"
    }

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