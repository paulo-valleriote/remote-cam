package remotecam.protocol

import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.buildJsonArray
import kotlinx.serialization.json.buildJsonObject

/**
 * Encodes a validated [SignalingMessage] as the canonical wire JSON.
 *
 * This is the counterpart of [SignalingParser]: every frame it produces must be
 * accepted by the strict parser and match `protocol/signaling.schema.json`.
 */
object SignalingEncoder {

    private const val VERSION: Long = 1

    fun encode(message: SignalingMessage): String = when (message) {
        is SignalingMessage.Pair -> frame("pair") {
            put("token", JsonPrimitive(Hex.encode(message.token)))
            put("dtls_fingerprint", JsonPrimitive(Hex.encode(message.dtlsFingerprint)))
            put("limits", encodeLimits(message.limits))
        }
        is SignalingMessage.Paired -> frame("paired") {
            put("session_id", JsonPrimitive(Hex.encode(message.sessionId)))
            put("epoch", JsonPrimitive(message.epoch))
            put("dtls_fingerprint", JsonPrimitive(Hex.encode(message.dtlsFingerprint)))
            put("accepted_limits", encodeLimits(message.acceptedLimits))
        }
        is SignalingMessage.Offer -> frame("offer") {
            put("epoch", JsonPrimitive(message.epoch))
            put("sdp", JsonPrimitive(message.sdp))
        }
        is SignalingMessage.Answer -> frame("answer") {
            put("epoch", JsonPrimitive(message.epoch))
            put("sdp", JsonPrimitive(message.sdp))
        }
        is SignalingMessage.Ice -> frame("ice") {
            put("epoch", JsonPrimitive(message.epoch))
            put(
                "candidates",
                buildJsonArray {
                    for (candidate in message.candidates) {
                        add(
                            buildJsonObject {
                                put("address", JsonPrimitive(candidate.address))
                                put("port", JsonPrimitive(candidate.port))
                                put("component", JsonPrimitive(candidate.component))
                                candidate.protocol?.let { put("protocol", JsonPrimitive(it)) }
                            },
                        )
                    }
                },
            )
        }
        is SignalingMessage.StatsMessage -> frame("stats") {
            message.stats.fps?.let { put("fps", JsonPrimitive(it)) }
            message.stats.videoBitrateKbps?.let { put("video_bitrate_kbps", JsonPrimitive(it)) }
            message.stats.rttMs?.let { put("rtt_ms", JsonPrimitive(it)) }
            message.stats.lossPct?.let { put("loss_pct", JsonPrimitive(it)) }
            message.stats.thermalStatus?.let { put("thermal_status", JsonPrimitive(it)) }
        }
        is SignalingMessage.Close -> frame("close") {
            put("code", JsonPrimitive(message.code.wireName))
        }
        is SignalingMessage.Error -> frame("error") {
            put("code", JsonPrimitive(message.code.wireName))
            message.message?.let { put("message", JsonPrimitive(it)) }
        }
        SignalingMessage.Ping -> frame("ping") {}
        SignalingMessage.Pong -> frame("pong") {}
    }.toString()

    private fun frame(type: String, fields: kotlinx.serialization.json.JsonObjectBuilder.() -> Unit): JsonObject =
        buildJsonObject {
            put("version", JsonPrimitive(VERSION))
            put("type", JsonPrimitive(type))
            fields()
        }

    private fun encodeLimits(limits: Limits): JsonObject = buildJsonObject {
        limits.maxWidth?.let { put("max_width", JsonPrimitive(it)) }
        limits.maxHeight?.let { put("max_height", JsonPrimitive(it)) }
        limits.maxFrameRate?.let { put("max_frame_rate", JsonPrimitive(it)) }
        limits.maxVideoBitrateKbps?.let { put("max_video_bitrate_kbps", JsonPrimitive(it)) }
        limits.maxAudioBitrateKbps?.let { put("max_audio_bitrate_kbps", JsonPrimitive(it)) }
    }
}
