package remotecam.session

import java.nio.file.Files
import java.nio.file.Path
import kotlin.io.path.createDirectories
import kotlin.io.path.writeText
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFailsWith
import kotlin.test.assertFalse
import kotlin.test.assertNull
import kotlin.test.assertTrue
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.jsonArray
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import remotecam.protocol.Conformance

class HandshakeConformanceTest {
    private val vectors = Conformance.repositoryRoot().resolve("protocol/vectors")
    private val results by lazy { HandshakeConformance.run(vectors) }

    private fun result(id: String) = results.single { it.id == id }

    @Test
    fun everyDeclaredHandshakeScenarioIsReportedExactlyOnce() {
        val document = Json.parseToJsonElement(
            Files.readString(vectors.resolve("signaling/scenarios.json")),
        ).jsonObject
        val declared = document["scenarios"]!!.jsonArray
            .map { it.jsonObject }
            .filter { it["kind"]!!.jsonPrimitive.content == "handshake" }
            .map { it["id"]!!.jsonPrimitive.content }
        assertEquals(declared, results.map { it.id })
    }

    @Test
    fun aValidPairingEndsNegotiatingOnTheBaselineEpoch() {
        val valid = result("hs-valid-pairing")
        assertTrue(valid.executed)
        assertEquals(true, valid.accepted)
        assertEquals("negotiating", valid.state)
        assertEquals("handshaking->negotiating", valid.transition)
        assertEquals(1L, valid.epoch)
        assertNull(valid.reasonCode)
    }

    @Test
    fun aLateTokenIsRefusedAndAFreshQrIsIssued() {
        val expired = result("hs-token-expired")
        assertEquals("token_expired", expired.reasonCode)
        assertEquals("pairing_ready", expired.state)
        assertEquals(2L, expired.epoch)
    }

    @Test
    fun aStaleEpochIsDiscardedAndCounted() {
        val stale = result("hs-protocol-violation")
        assertEquals("protocol_violation", stale.reasonCode)
        assertEquals(1L, stale.protocolViolations)
    }

    @Test
    fun qrScenariosReportTheDecoderReason() {
        assertEquals("unsupported_qr_version", result("hs-qr-unsupported-version").reasonCode)
        assertNull(result("hs-qr-expired").state)
    }

    @Test
    fun transportScenariosAreReportedAsNotExecuted() {
        val proxy = result("hs-interception-proxy")
        assertEquals("transport", proxy.layer)
        assertFalse(proxy.executed)
        assertNull(proxy.accepted)
        assertNull(proxy.reasonCode)
    }

    @Test
    fun theReportSerializesEveryKeyEvenWhenNull() {
        val json = Json.parseToJsonElement(HandshakeConformance.toJson(results)).jsonObject
        val first = json["handshake"]!!.jsonArray.first().jsonObject
        assertEquals(
            setOf(
                "id", "layer", "executed", "accepted", "reason_code", "state", "transition", "epoch",
                "protocol_violations",
            ),
            first.keys,
        )
        val transport = json["handshake"]!!.jsonArray.map { it.jsonObject }
            .first { it["id"]!!.jsonPrimitive.content == "hs-wrong-fingerprint" }
        assertEquals("null", transport["accepted"].toString())
    }

    @Test
    fun writesTheCanonicalDocumentWhenAskedTo() {
        System.getenv("CONFORMANCE_HANDSHAKE_OUTPUT")?.let { output ->
            Files.write(Path.of(output), (HandshakeConformance.toJson(results) + "\n").toByteArray())
        }
    }

    private fun scratch(scenarios: String): Path {
        val root = Files.createTempDirectory("remotecam-hs")
        root.resolve("signaling").createDirectories()
        root.resolve("qr").createDirectories()
        root.resolve("signaling/scenarios.json").writeText(scenarios)
        return root
    }

    private fun scenario(extra: String) =
        """{"version":1,"scenarios":[{"id":"hs-x","kind":"handshake","expected_outcome":"accept","expected_reason_code":null,$extra}]}"""

    private fun failure(extra: String): String =
        assertFailsWith<IllegalStateException> {
            HandshakeConformance.run(scratch(scenario(extra)))
        }.message.orEmpty()

    @Test
    fun aDivergingExpectationFailsTheRun() {
        val message = failure(
            """"layer":"core","steps":[{"action":"start_pairing"}],"expected_state":"idle",""" +
                """"expected_transition":"idle->pairing_ready","expected_epoch":1,"expected_protocol_violations":0""",
        )
        assertTrue(message.contains("hs-x") && message.contains("state"), message)
    }

    @Test
    fun anUnknownLayerFailsInsteadOfBeingSkipped() {
        assertTrue(failure(""""layer":"mystery"""").contains("mystery"))
    }

    @Test
    fun aScenarioWithoutALayerFails() {
        assertTrue(failure(""""note":"none"""").contains("layer"))
    }

    @Test
    fun anUnknownStepActionFails() {
        assertTrue(failure(""""layer":"core","steps":[{"action":"teleport"}]""").contains("teleport"))
    }

    @Test
    fun aCoreScenarioWithoutStepsFails() {
        assertTrue(failure(""""layer":"core","steps":[]""").contains("steps"))
    }

    @Test
    fun aStepMissingItsArgumentsFails() {
        assertTrue(failure(""""layer":"core","steps":[{"action":"advance_clock"}]""").contains("millis"))
        assertTrue(failure(""""layer":"core","steps":[{"action":"admit_epoch"}]""").contains("epoch"))
    }

    @Test
    fun anEpochOutsideTheUnsigned32BitRangeFailsLikeTheRustParser() {
        for (epoch in listOf("-1", "4294967296")) {
            val message = failure(""""layer":"core","steps":[{"action":"admit_epoch","epoch":$epoch}]""")
            assertTrue(message.contains("u32"), message)
        }
    }

    @Test
    fun aTokenReferenceThatDoesNotExistFails() {
        assertTrue(
            failure(""""layer":"core","steps":[{"action":"pair","token":"first"}]""")
                .contains("no pairing token"),
        )
        assertTrue(
            failure(""""layer":"core","steps":[{"action":"pair","token":"nope"}]""")
                .contains("unknown token"),
        )
    }

    @Test
    fun aQrScenarioNeedsItsVector() {
        assertTrue(failure(""""layer":"qr"""").contains("qr_vector"))
    }

    @Test
    fun everyStepActionIsReplayable() {
        val steps = listOf(
            "start_pairing", "peer_connected", "negotiation_completed", "first_media_frame",
            "end_session", "first_frame_timed_out",
        ).joinToString(",") { """{"action":"$it"}""" }
        // The replay itself must not fail; the divergence check then reports the
        // observed outcome, which proves every action was understood.
        val message = failure(""""layer":"core","steps":[$steps]""")
        assertFalse(message.contains("unknown step"), message)
    }

    @Test
    fun aMissingScenarioFileIsAnError() {
        assertFailsWith<java.nio.file.NoSuchFileException> {
            HandshakeConformance.run(Path.of("/nonexistent/remotecam"))
        }
    }
}
