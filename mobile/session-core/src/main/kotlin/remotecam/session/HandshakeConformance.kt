package remotecam.session

import java.nio.file.Files
import java.nio.file.Path
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonNull
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.buildJsonArray
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.jsonArray
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.long
import remotecam.protocol.Conformance
import remotecam.protocol.Limits
import remotecam.protocol.QrDecodeResult
import remotecam.protocol.QrPayload
import remotecam.protocol.ReasonCode
import remotecam.protocol.SignalingEncoder
import remotecam.protocol.SignalingMessage

/**
 * Observed outcome for one handshake scenario. Every field a layer does not
 * decide is null, and the document always carries the full key set so it is
 * byte-comparable with the Rust `HandshakeResult`.
 */
data class HandshakeResult(
    val id: String,
    val layer: String,
    val executed: Boolean,
    val accepted: Boolean?,
    val reasonCode: String?,
    val state: String?,
    val transition: String?,
    val epoch: Long?,
    val protocolViolations: Long?,
)

/**
 * Replays the pinned handshake scenarios on the Kotlin session core, exactly as
 * the Rust `conformance` module does (ADR-0018).
 */
object HandshakeConformance {
    private val DESKTOP_FINGERPRINT = ByteArray(32) { 0x44 }
    private val PEER_FINGERPRINT = ByteArray(32) { 0x55 }
    private val FORGED_TOKEN = ByteArray(32) { 0xee.toByte() }
    private const val SEED = 7L
    private const val MAX_EPOCH = 0xFFFF_FFFFL

    fun repositoryRoot(): Path = Conformance.repositoryRoot()

    /**
     * Replays every `kind = handshake` scenario under [vectorsDirectory]. A
     * scenario that diverges from its pinned expectation, or that names an
     * unknown layer or step, fails the run: nothing is skipped.
     */
    fun run(vectorsDirectory: Path): List<HandshakeResult> {
        val file = Json.parseToJsonElement(
            Files.readAllBytes(vectorsDirectory.resolve("signaling/scenarios.json")).decodeToString(),
        ).jsonObject
        return file["scenarios"]!!.jsonArray
            .map { it.jsonObject }
            .filter { it.string("kind") == "handshake" }
            .map { scenario ->
                val id = scenario.string("id")
                val result = when (val layer = scenario.optionalString("layer")) {
                    "core" -> replayCore(scenario)
                    "qr" -> replayQr(scenario, vectorsDirectory)
                    "transport" -> HandshakeResult(id, "transport", false, null, null, null, null, null, null)
                    null -> error("$id: scenario has no layer")
                    else -> error("$id: unknown layer $layer")
                }
                checkExpectation(scenario, result)
                result
            }
    }

    fun toJson(results: List<HandshakeResult>): String = buildJsonObject {
        put(
            "handshake",
            buildJsonArray {
                results.forEach { add(it.toJson()) }
            },
        )
    }.toString()

    private fun HandshakeResult.toJson(): JsonObject = buildJsonObject {
        put("id", JsonPrimitive(id))
        put("layer", JsonPrimitive(layer))
        put("executed", JsonPrimitive(executed))
        put("accepted", accepted?.let(::JsonPrimitive) ?: JsonNull)
        put("reason_code", reasonCode?.let(::JsonPrimitive) ?: JsonNull)
        put("state", state?.let(::JsonPrimitive) ?: JsonNull)
        put("transition", transition?.let(::JsonPrimitive) ?: JsonNull)
        put("epoch", epoch?.let(::JsonPrimitive) ?: JsonNull)
        put("protocol_violations", protocolViolations?.let(::JsonPrimitive) ?: JsonNull)
    }

    private fun checkExpectation(scenario: JsonObject, result: HandshakeResult) {
        if (!result.executed) return
        val id = scenario.string("id")
        val mismatches = mutableListOf<String>()
        val expectedAccepted = scenario.string("expected_outcome") == "accept"
        if (result.accepted != expectedAccepted) {
            mismatches.add("accepted ${result.accepted} != $expectedAccepted")
        }
        compare(mismatches, "reason_code", result.reasonCode, scenario.optionalString("expected_reason_code"))
        if (result.layer == "core") {
            compare(mismatches, "state", result.state, scenario.optionalString("expected_state"))
            compare(mismatches, "transition", result.transition, scenario.optionalString("expected_transition"))
            compare(mismatches, "epoch", result.epoch, scenario.optionalLong("expected_epoch"))
            compare(
                mismatches,
                "protocol_violations",
                result.protocolViolations,
                scenario.optionalLong("expected_protocol_violations"),
            )
        }
        check(mismatches.isEmpty()) { "$id: ${mismatches.joinToString("; ")}" }
    }

    private fun <T> compare(mismatches: MutableList<String>, field: String, observed: T?, expected: T?) {
        if (observed != expected) mismatches.add("$field $observed != $expected")
    }

    private fun replayQr(scenario: JsonObject, vectorsDirectory: Path): HandshakeResult {
        val id = scenario.string("id")
        val name = scenario.optionalString("qr_vector") ?: error("$id: qr scenario has no qr_vector")
        val vector = Json.parseToJsonElement(
            Files.readAllBytes(vectorsDirectory.resolve("qr/$name.json")).decodeToString(),
        ).jsonObject
        val payload = hexToBytes(vector["expected"]!!.jsonObject.string("payload_hex"))
            ?: error("$id: invalid payload hex")
        val (accepted, reason) = when (val decoded = QrPayload.decode(payload, vector["now_unix"]!!.jsonPrimitive.long)) {
            is QrDecodeResult.Accepted -> true to null
            is QrDecodeResult.Rejected -> false to decoded.reason.wireName
        }
        return HandshakeResult(id, "qr", true, accepted, reason, null, null, null, null)
    }

    /** What the last replayed step decided: the refusal reason, or null when accepted. */
    private class Replay {
        val clock = ManualClock(Timestamp.ZERO)
        val core = SessionCore(clock, SeededRandomSource(SEED), DESKTOP_FINGERPRINT)
        val tokens = mutableListOf<ByteArray>()

        fun collect(effects: List<Effect>): String? {
            var reason: String? = null
            for (effect in effects) {
                when (effect) {
                    is Effect.PresentPairing -> tokens.add(effect.material.token.bytes)
                    is Effect.RejectPeer -> if (reason == null) reason = effect.reason.wireName
                    else -> Unit
                }
            }
            return reason
        }

        fun handle(event: SessionEvent): String? = when (val result = core.handle(event)) {
            is TransitionResult.Accepted -> collect(result.effects)
            is TransitionResult.Rejected -> result.error.code
        }

        fun token(which: String, id: String): ByteArray = when (which) {
            "wrong" -> FORGED_TOKEN
            "first" -> tokens.firstOrNull() ?: error("$id: no pairing token has been presented yet")
            "latest" -> tokens.lastOrNull() ?: error("$id: no pairing token has been presented yet")
            else -> error("$id: unknown token reference $which")
        }

        fun apply(step: JsonObject, id: String): String? = when (val action = step.string("action")) {
            "start_pairing" -> handle(SessionEvent.StartPairing)
            "peer_connected" -> handle(SessionEvent.PeerConnected)
            "negotiation_completed" -> handle(SessionEvent.NegotiationCompleted)
            "first_media_frame" -> handle(SessionEvent.FirstMediaFrame(MediaFrame(0, clock.now())))
            "first_frame_timed_out" -> handle(SessionEvent.FirstFrameTimedOut)
            "end_session" -> handle(SessionEvent.EndSession)
            "advance_clock" -> {
                clock.advance(step.optionalLong("millis") ?: error("$id: advance_clock needs millis"))
                null
            }
            "pair" -> {
                val token = token(step.optionalString("token") ?: "latest", id)
                val frame = SignalingEncoder.encode(
                    SignalingMessage.Pair(token, PEER_FINGERPRINT, Limits()),
                ).encodeToByteArray()
                when (val result = core.authenticateFirstFrame(frame)) {
                    is PairingResult.Authenticated -> collect(result.effects)
                    is PairingResult.Refused -> result.error.code.wireName
                }
            }
            "admit_epoch" -> {
                val epoch = step.optionalLong("epoch") ?: error("$id: admit_epoch needs epoch")
                check(epoch in 0..MAX_EPOCH) { "$id: admit_epoch epoch $epoch is not a u32" }
                if (core.admitFrameEpoch(epoch)) null else ReasonCode.PROTOCOL_VIOLATION.wireName
            }
            else -> error("$id: unknown step action $action")
        }
    }

    private fun replayCore(scenario: JsonObject): HandshakeResult {
        val id = scenario.string("id")
        val steps = scenario["steps"]?.let { it as? JsonArray }?.map { it.jsonObject }
        if (steps.isNullOrEmpty()) error("$id: core scenario has no steps")
        val replay = Replay()
        var before = replay.core.state
        var reason: String? = null
        for (step in steps) {
            before = replay.core.state
            reason = replay.apply(step, id)
        }
        val after = replay.core.state
        return HandshakeResult(
            id = id,
            layer = "core",
            executed = true,
            accepted = reason == null,
            reasonCode = reason,
            state = after.label,
            transition = "${before.label}->${after.label}",
            epoch = replay.core.epoch,
            protocolViolations = replay.core.protocolViolations,
        )
    }

    private fun hexToBytes(text: String): ByteArray? {
        if (text.length % 2 != 0) return null
        return ByteArray(text.length / 2) { index ->
            val high = Character.digit(text[2 * index], 16)
            val low = Character.digit(text[2 * index + 1], 16)
            if (high < 0 || low < 0) return null
            ((high shl 4) or low).toByte()
        }
    }

    private fun JsonObject.string(key: String): String = this[key]!!.jsonPrimitive.content

    private fun JsonObject.optionalString(key: String): String? =
        this[key]?.takeIf { it !is JsonNull }?.jsonPrimitive?.content

    private fun JsonObject.optionalLong(key: String): Long? =
        this[key]?.takeIf { it !is JsonNull }?.jsonPrimitive?.long
}
