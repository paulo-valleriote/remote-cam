package remotecam.protocol

import java.nio.file.Files
import java.nio.file.Path
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonNull
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.buildJsonArray
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.boolean
import kotlinx.serialization.json.int
import kotlinx.serialization.json.jsonArray
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.long

/** Observed outcome for one QR vector. */
data class QrResult(
    val id: String,
    val accepted: Boolean,
    val reasonCode: String?,
    val decodedPayloadHex: String?,
    val encodedPayloadHex: String?,
)

/** Observed outcome for one signaling scenario. */
data class SignalingResult(
    val id: String,
    val kind: String,
    val accepted: Boolean,
    val reasonCode: String?,
)

/** Observed state of the closed reason-code enumeration. */
data class ReasonCodeResult(val count: Int, val ok: Boolean)

/** Canonical result document, byte-comparable with the Rust implementation. */
data class ConformanceReport(
    val qr: List<QrResult>,
    val signaling: List<SignalingResult>,
    val reasonCodes: ReasonCodeResult,
) {
    fun passed(): Boolean = failures().isEmpty()

    fun failures(): List<String> {
        val failures = mutableListOf<String>()
        for (result in qr) {
            if (!result.accepted && result.reasonCode == null) {
                failures.add("${result.id}: rejected without a reason code")
            }
        }
        for (result in signaling) {
            if (!result.accepted && result.reasonCode == null) {
                failures.add("${result.id}: rejected without a reason code")
            }
        }
        if (!reasonCodes.ok) {
            failures.add("reason-code enumeration does not match the contract")
        }
        return failures
    }

    fun toJson(): String = buildJsonObject {
        put("qr", buildJsonArray { qr.forEach { add(it.toJson()) } })
        put("signaling", buildJsonArray { signaling.forEach { add(it.toJson()) } })
        put(
            "reason_codes",
            buildJsonObject {
                put("count", JsonPrimitive(reasonCodes.count))
                put("ok", JsonPrimitive(reasonCodes.ok))
            },
        )
    }.toString()

    private fun QrResult.toJson(): JsonObject = buildJsonObject {
        put("id", JsonPrimitive(id))
        put("accepted", JsonPrimitive(accepted))
        put("reason_code", reasonCode?.let(::JsonPrimitive) ?: JsonNull)
        decodedPayloadHex?.let { put("decoded_payload_hex", JsonPrimitive(it)) }
        encodedPayloadHex?.let { put("encoded_payload_hex", JsonPrimitive(it)) }
    }

    private fun SignalingResult.toJson(): JsonObject = buildJsonObject {
        put("id", JsonPrimitive(id))
        put("kind", JsonPrimitive(kind))
        put("accepted", JsonPrimitive(accepted))
        put("reason_code", reasonCode?.let(::JsonPrimitive) ?: JsonNull)
    }
}

/** Executes the pinned vectors exactly as the Rust `conformance` binary does. */
object Conformance {
    fun repositoryRoot(): Path {
        System.getenv("REMOTECAM_REPO_ROOT")?.let { return Path.of(it) }
        var current: Path? = Path.of(System.getProperty("user.dir")).toAbsolutePath()
        while (current != null) {
            if (Files.isDirectory(current.resolve("protocol/vectors"))) return current
            current = current.parent
        }
        error("cannot locate the repository root; set REMOTECAM_REPO_ROOT")
    }

    fun run(vectorsDirectory: Path): ConformanceReport {
        val qr = runQrVectors(vectorsDirectory.resolve("qr"))
        val signaling = runSignalingScenarios(vectorsDirectory.resolve("signaling/scenarios.json"))
        val reasonCodes = checkReasonCodes(vectorsDirectory.parent.resolve("reason-codes.json"))
        return ConformanceReport(qr, signaling, reasonCodes)
    }

    private fun runQrVectors(directory: Path): List<QrResult> {
        val files = Files.list(directory).use { stream ->
            stream.filter { it.fileName.toString().endsWith(".json") }.sorted().toList()
        }
        return files.map { file ->
            val vector = Json.parseToJsonElement(Files.readAllBytes(file).decodeToString()).jsonObject
            val id = vector.string("id")
            val nowUnix = vector.long("now_unix")
            val expected = vector["expected"]!!.jsonObject
            val expectedHex = expected.string("payload_hex")
            val expectedAccepted = expected.boolean("accepted")
            val expectedReason = expected["reason_code"]?.takeIf { it !is JsonNull }?.jsonPrimitive?.content
            val payload = Hex.decode(expectedHex) ?: error("$id: invalid expected payload hex")

            val result = QrPayload.decode(payload, nowUnix)
            val observed = when (result) {
                is QrDecodeResult.Accepted -> {
                    val decodedHex = Hex.encode(result.payload.encode())
                    val encodedHex = vector["encoder_input"]?.jsonObject?.let { input ->
                        Hex.encode(payloadFromInput(id, input).encode())
                    }
                    QrResult(id, true, null, decodedHex, encodedHex)
                }
                is QrDecodeResult.Rejected -> QrResult(id, false, result.reason.wireName, null, null)
            }

            check(
                observed.accepted == expectedAccepted && observed.reasonCode == expectedReason,
            ) {
                "$id: expected accepted=$expectedAccepted reason=$expectedReason, " +
                    "observed accepted=${observed.accepted} reason=${observed.reasonCode}"
            }
            observed.encodedPayloadHex?.let { encoded ->
                check(encoded == expectedHex) { "$id: encoder produced $encoded, expected $expectedHex" }
            }
            observed.decodedPayloadHex?.let { decoded ->
                check(decoded == expectedHex) { "$id: decoded payload re-encodes to $decoded, expected $expectedHex" }
            }
            observed
        }
    }

    private fun payloadFromInput(id: String, input: JsonObject): QrPayload {
        val endpoints = input["endpoints"]!!.jsonArray.map { element ->
            val endpoint = element.jsonObject
            QrEndpoint.fromAddress(
                endpoint.int("family"),
                endpoint.string("address"),
                endpoint.int("port"),
            ) ?: error("$id: invalid endpoint ${endpoint.string("address")}")
        }
        return QrPayload(
            sessionId = Hex.decodeExact(input.string("session_id_hex"), 16) ?: error("$id: session_id"),
            epoch = input.long("epoch"),
            expiresAt = input.long("expires_at"),
            pairingToken = Hex.decodeExact(input.string("pairing_token_hex"), 32) ?: error("$id: token"),
            tlsFingerprint = Hex.decodeExact(input.string("tls_fingerprint_hex"), 32)
                ?: error("$id: fingerprint"),
            capabilities = input.int("capabilities"),
            endpoints = endpoints,
        )
    }

    private fun runSignalingScenarios(path: Path): List<SignalingResult> {
        val file = Json.parseToJsonElement(Files.readAllBytes(path).decodeToString()).jsonObject
        return file["scenarios"]!!.jsonArray.mapNotNull { element ->
            val scenario = element.jsonObject
            if (scenario.string("kind") != "message") return@mapNotNull null
            val id = scenario.string("id")
            val frame = scenario["raw_frame"]?.jsonPrimitive?.content
                ?: scenario["message"]!!.toString()
            val expectedAccepted = scenario.string("expected_outcome") == "accept"
            val expectedReason = scenario["expected_reason_code"]
                ?.takeIf { it !is JsonNull }
                ?.jsonPrimitive
                ?.content

            val observed = when (val result = SignalingParser.parse(frame)) {
                is SignalingParseResult.Accepted -> SignalingResult(id, "message", true, null)
                is SignalingParseResult.Rejected -> SignalingResult(id, "message", false, result.reason.wireName)
            }
            check(observed.accepted == expectedAccepted && observed.reasonCode == expectedReason) {
                "$id: expected accepted=$expectedAccepted reason=$expectedReason, " +
                    "observed accepted=${observed.accepted} reason=${observed.reasonCode}"
            }
            observed
        }
    }

    private fun checkReasonCodes(path: Path): ReasonCodeResult {
        val document = Json.parseToJsonElement(Files.readAllBytes(path).decodeToString()).jsonObject
        val codes = document["codes"]!!.jsonArray
        var ok = codes.size == ReasonCode.entries.size
        for (element in codes) {
            val entry = element.jsonObject
            val code = ReasonCode.fromWire(entry.string("code"))
            if (code == null) {
                ok = false
                continue
            }
            val categories = entry["categories"]!!.jsonArray.map { it.jsonPrimitive.content }
            if (categories != code.categories.map { it.wireName } ||
                entry.boolean("security_event") != code.securityEvent ||
                entry.boolean("user_actionable") != code.userActionable
            ) {
                ok = false
            }
        }
        for (code in ReasonCode.entries) {
            if (codes.none { it.jsonObject.string("code") == code.wireName }) ok = false
        }
        return ReasonCodeResult(codes.size, ok)
    }

    private fun JsonObject.string(key: String): String = this[key]!!.jsonPrimitive.content
    private fun JsonObject.long(key: String): Long = this[key]!!.jsonPrimitive.long
    private fun JsonObject.int(key: String): Int = this[key]!!.jsonPrimitive.int
    private fun JsonObject.boolean(key: String): Boolean = this[key]!!.jsonPrimitive.boolean
}
