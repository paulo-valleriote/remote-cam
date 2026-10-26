package remotecam.session

import remotecam.protocol.Limits
import remotecam.protocol.ReasonCode
import remotecam.protocol.SignalingEncoder
import remotecam.protocol.SignalingMessage
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertIs
import kotlin.test.assertNull
import kotlin.test.assertTrue

private val DESKTOP_FINGERPRINT = ByteArray(32) { 0x44 }
private val PEER_FINGERPRINT = ByteArray(32) { 0x55 }

private class Fixture(seed: Long = 7, start: Long = 0, epochs: EpochSource = EpochSource()) {
    val clock = ManualClock(Timestamp(start))
    val core = SessionCore(clock, SeededRandomSource(seed), DESKTOP_FINGERPRINT, epochs)

    fun handle(event: SessionEvent): List<Effect> =
        assertIs<TransitionResult.Accepted>(core.handle(event)).effects

    fun rejection(event: SessionEvent): TransitionError =
        assertIs<TransitionResult.Rejected>(core.handle(event)).error

    fun start(): PairingMaterial {
        val effects = handle(SessionEvent.StartPairing)
        return assertIs<Effect.PresentPairing>(effects.last()).material
    }

    fun pair(token: ByteArray): PairingResult = core.authenticateFirstFrame(pairFrame(token))

    fun authenticate(token: ByteArray): List<Effect> =
        assertIs<PairingResult.Authenticated>(pair(token)).effects

    fun refusal(frame: ByteArray): PairingError =
        assertIs<PairingResult.Refused>(core.authenticateFirstFrame(frame)).error

    fun handshaking(): PairingMaterial {
        val material = start()
        handle(SessionEvent.PeerConnected)
        return material
    }

    fun negotiating(): PairingMaterial {
        val material = handshaking()
        authenticate(material.token.bytes)
        return material
    }

    fun connected(): PairingMaterial {
        val material = negotiating()
        handle(SessionEvent.NegotiationCompleted)
        handle(SessionEvent.FirstMediaFrame(MediaFrame(0, clock.now())))
        return material
    }
}

private fun pairFrame(token: ByteArray): ByteArray =
    SignalingEncoder.encode(SignalingMessage.Pair(token, PEER_FINGERPRINT, Limits())).encodeToByteArray()

private fun ping(padding: Int = 0): ByteArray =
    ("""{"version":1,"type":"ping"}""" + " ".repeat(padding)).encodeToByteArray()

private fun offer(): ByteArray = SignalingEncoder.encode(
    SignalingMessage.Offer(1, "v=0\r\no=- 1 1 IN IP4 0.0.0.0\r\nm=video 9 UDP/TLS/RTP/SAVPF 96\r\na=sendonly\r\n"),
).encodeToByteArray()

class SessionStateMachineTest {
    @Test
    fun aNewCoreRestsInIdleWithoutASession() {
        val core = Fixture().core
        assertEquals(SessionState.IDLE, core.state)
        assertNull(core.sessionId)
        assertNull(core.epoch)
        assertNull(core.retiredState)
        assertEquals(Timestamp.ZERO, core.lastTransitionAt)
    }

    @Test
    fun everyStateHasAStableReportLabel() {
        assertEquals(
            listOf("idle", "pairing_ready", "handshaking", "negotiating", "connected", "ended", "superseded"),
            SessionState.entries.map { it.label },
        )
    }

    @Test
    fun startingAPairingPresentsTheMaterialOnTheBaselineEpoch() {
        val fixture = Fixture(start = 500)
        val material = fixture.start()
        assertEquals(SessionState.PAIRING_READY, fixture.core.state)
        assertEquals(1L, material.epoch)
        assertEquals(1L, fixture.core.epoch)
        assertEquals(material.sessionId, fixture.core.sessionId)
        assertEquals(Timestamp(500), fixture.core.lastTransitionAt)
    }

    @Test
    fun theSameSeedAssignsTheSameSessionIdAndTheSeedChangesIt() {
        val first = Fixture(seed = 7).start()
        val again = Fixture(seed = 7).start()
        val other = Fixture(seed = 8).start()
        assertEquals(first.sessionId, again.sessionId)
        assertEquals("d70d3259e4e1cb631c663cf4d73c4c04", first.sessionId.hex())
        assertTrue(first.sessionId != other.sessionId)
    }

    @Test
    fun aPeerConnectionMovesToHandshakingWithoutAnEffect() {
        val fixture = Fixture()
        fixture.start()
        assertEquals(emptyList(), fixture.handle(SessionEvent.PeerConnected))
        assertEquals(SessionState.HANDSHAKING, fixture.core.state)
    }

    @Test
    fun anAuthenticatedPairSendsPairedAndMovesToNegotiating() {
        val fixture = Fixture()
        fixture.handshaking().also { material ->
            val effects = fixture.authenticate(material.token.bytes)
            val paired = assertIs<Effect.SendPaired>(effects.single())
            assertEquals(material.sessionId, paired.sessionId)
            assertEquals(1L, paired.epoch)
            assertTrue(DESKTOP_FINGERPRINT.contentEquals(paired.dtlsFingerprint))
        }
        assertEquals(SessionState.NEGOTIATING, fixture.core.state)
    }

    @Test
    fun negotiationStartsMediaOnceAndTheFirstFrameConnects() {
        val fixture = Fixture()
        fixture.negotiating()
        assertEquals(listOf(Effect.StartMedia), fixture.handle(SessionEvent.NegotiationCompleted))
        val error = fixture.rejection(SessionEvent.NegotiationCompleted)
        assertEquals("invalid_transition", error.code)
        fixture.clock.advance(FIRST_MEDIA_FRAME_TIMEOUT_MILLIS)
        fixture.handle(SessionEvent.FirstMediaFrame(MediaFrame(0, fixture.clock.now())))
        assertEquals(SessionState.CONNECTED, fixture.core.state)
    }

    @Test
    fun aFirstFrameAfterTheDeadlineIsRejected() {
        val fixture = Fixture()
        fixture.negotiating()
        fixture.handle(SessionEvent.NegotiationCompleted)
        fixture.clock.advance(FIRST_MEDIA_FRAME_TIMEOUT_MILLIS + 1)
        fixture.rejection(SessionEvent.FirstMediaFrame(MediaFrame(0, fixture.clock.now())))
        assertEquals(SessionState.NEGOTIATING, fixture.core.state)
    }

    @Test
    fun aFirstFrameStampedAfterTheDeadlineIsRejectedEvenIfTheClockIsOnTime() {
        val fixture = Fixture()
        fixture.negotiating()
        fixture.handle(SessionEvent.NegotiationCompleted)
        fixture.rejection(
            SessionEvent.FirstMediaFrame(MediaFrame(0, Timestamp(FIRST_MEDIA_FRAME_TIMEOUT_MILLIS + 1))),
        )
    }

    @Test
    fun aFirstFrameBeforeMediaStartedIsRejected() {
        val fixture = Fixture()
        fixture.negotiating()
        fixture.rejection(SessionEvent.FirstMediaFrame(MediaFrame(0, Timestamp.ZERO)))
    }

    @Test
    fun aTimeoutAtTheDeadlineIsRejectedAndAfterItResetsToIdle() {
        val fixture = Fixture()
        fixture.negotiating()
        fixture.handle(SessionEvent.NegotiationCompleted)
        fixture.clock.advance(FIRST_MEDIA_FRAME_TIMEOUT_MILLIS)
        fixture.rejection(SessionEvent.FirstFrameTimedOut)
        fixture.clock.advance(1)
        assertEquals(listOf(Effect.End), fixture.handle(SessionEvent.FirstFrameTimedOut))
        assertEquals(SessionState.IDLE, fixture.core.state)
        assertNull(fixture.core.sessionId)
        assertNull(fixture.core.retiredState)
    }

    @Test
    fun endingAConnectedSessionMovesToEndedAndDiscardsAPendingSuccessor() {
        val fixture = Fixture()
        fixture.connected()
        fixture.start()
        assertEquals(listOf(Effect.End), fixture.handle(SessionEvent.EndSession))
        assertEquals(SessionState.ENDED, fixture.core.state)
        fixture.rejection(SessionEvent.PeerConnected)
    }

    @Test
    fun aRejectedEventNeverStartsPairingWhenNoStateAllowsIt() {
        val fixture = Fixture()
        fixture.negotiating()
        fixture.rejection(SessionEvent.StartPairing)
        assertEquals(SessionState.NEGOTIATING, fixture.core.state)
    }

    @Test
    fun anOutOfOrderEventIsRejectedWithAClosedCodeAndKeepsTheState() {
        val fixture = Fixture()
        val error = fixture.rejection(SessionEvent.PeerConnected)
        assertEquals("invalid_transition", error.code)
        assertEquals(SessionState.IDLE, error.from)
        assertEquals(SessionEvent.PeerConnected, error.event)
        assertEquals(SessionState.IDLE, fixture.core.state)
        fixture.rejection(SessionEvent.EndSession)
        fixture.rejection(SessionEvent.NegotiationCompleted)
        fixture.rejection(SessionEvent.FirstFrameTimedOut)
    }

    @Test
    fun theManualClockOnlyMovesWhenAdvancedOrSet() {
        val clock = ManualClock(Timestamp(5))
        assertEquals(Timestamp(5), clock.now())
        clock.advance(10)
        assertEquals(Timestamp(15), clock.now())
        clock.set(Timestamp(1))
        assertEquals(Timestamp(1), clock.now())
        assertTrue(Timestamp(1) < Timestamp(2))
    }

    @Test
    fun theSeededSourceRepeatsItsStreamAndFillsTheWholeBuffer() {
        val first = ByteArray(13).also { SeededRandomSource(7).fill(it) }
        val again = ByteArray(13).also { SeededRandomSource(7).fill(it) }
        assertTrue(first.contentEquals(again))
        assertTrue(first.takeLast(5).any { it.toInt() != 0 })
    }

    @Test
    fun aSessionIdRoundTripsItsBytes() {
        val bytes = ByteArray(SessionId.LENGTH) { it.toByte() }
        assertTrue(bytes.contentEquals(SessionId(bytes).bytes))
        assertFalse(SessionId(bytes) == SessionId(ByteArray(SessionId.LENGTH)))
    }
}

class PairingTest {
    @Test
    fun aNonPairFirstFrameEndsTheSessionWithProtocolViolation() {
        val fixture = Fixture()
        fixture.handshaking()
        assertEquals(PairingError.PROTOCOL_VIOLATION, fixture.refusal(ping()))
        assertEquals(SessionState.ENDED, fixture.core.state)
    }

    @Test
    fun aMalformedOrNonUtf8FirstFrameIsAProtocolViolation() {
        val malformed = Fixture().also { it.handshaking() }
        assertEquals(PairingError.PROTOCOL_VIOLATION, malformed.refusal("{not json".encodeToByteArray()))
        val binary = Fixture().also { it.handshaking() }
        assertEquals(PairingError.PROTOCOL_VIOLATION, binary.refusal(byteArrayOf(0xff.toByte(), 0xfe.toByte())))
    }

    @Test
    fun aPairFrameBeforeThePeerConnectsIsAProtocolViolation() {
        val fixture = Fixture()
        val material = fixture.start()
        assertEquals(
            PairingError.PROTOCOL_VIOLATION,
            assertIs<PairingResult.Refused>(fixture.pair(material.token.bytes)).error,
        )
        assertEquals(SessionState.ENDED, fixture.core.state)
    }

    @Test
    fun aPairFrameOnAnIdleCoreIsAProtocolViolation() {
        val fixture = Fixture()
        assertEquals(PairingError.PROTOCOL_VIOLATION, fixture.refusal(pairFrame(ByteArray(32))))
    }

    @Test
    fun differentTokenMismatchesHaveTheSameRejectionCode() {
        val fixture = Fixture()
        val material = fixture.handshaking()
        val flipped = material.token.bytes.copyOf().also { it[31] = (it[31].toInt() xor 1).toByte() }
        assertEquals(PairingError.TOKEN_INVALID, fixture.refusal(pairFrame(flipped)))
        assertEquals(SessionState.HANDSHAKING, fixture.core.state)
        val forged = Fixture().also { it.handshaking() }
        assertEquals(PairingError.TOKEN_INVALID, forged.refusal(pairFrame(ByteArray(32) { 0xee.toByte() })))
    }

    @Test
    fun aConsumedTokenIsRefusedAsTokenAlreadyUsedAfterTheSessionEnds() {
        val fixture = Fixture()
        val first = fixture.negotiating()
        fixture.handle(SessionEvent.NegotiationCompleted)
        fixture.clock.advance(FIRST_MEDIA_FRAME_TIMEOUT_MILLIS + 1)
        fixture.handle(SessionEvent.FirstFrameTimedOut)
        fixture.handshaking()
        assertEquals(PairingError.TOKEN_ALREADY_USED, fixture.refusal(pairFrame(first.token.bytes)))
        assertEquals(SessionState.HANDSHAKING, fixture.core.state)
    }

    @Test
    fun aConsumedTokenWhileTheSessionIsActiveIsRefusedAsSuperseded() {
        val fixture = Fixture()
        val first = fixture.negotiating()
        assertEquals(PairingError.SUPERSEDED, fixture.refusal(pairFrame(first.token.bytes)))
        assertEquals(SessionState.NEGOTIATING, fixture.core.state)
    }

    @Test
    fun aPairFrameWhileNegotiatingIsAProtocolViolationThatKeepsTheSession() {
        val fixture = Fixture()
        fixture.negotiating()
        assertEquals(PairingError.PROTOCOL_VIOLATION, fixture.refusal(pairFrame(ByteArray(32) { 1 })))
        assertEquals(SessionState.NEGOTIATING, fixture.core.state)
        assertEquals(PairingError.PROTOCOL_VIOLATION, fixture.refusal(ping()))
        assertEquals(SessionState.NEGOTIATING, fixture.core.state)
    }

    @Test
    fun anOversizedFirstFrameIsASecurityViolation() {
        val fixture = Fixture()
        fixture.handshaking()
        assertEquals(PairingError.SECURITY_VIOLATION, fixture.refusal(ping(MAX_SIGNALING_MESSAGE_BYTES)))
        assertEquals(SessionState.ENDED, fixture.core.state)
    }

    @Test
    fun everyPairingFailureMapsToItsClosedReasonCode() {
        assertEquals(
            listOf(
                ReasonCode.PROTOCOL_VIOLATION,
                ReasonCode.TOKEN_INVALID,
                ReasonCode.TOKEN_ALREADY_USED,
                ReasonCode.SUPERSEDED,
                ReasonCode.SECURITY_VIOLATION,
            ),
            PairingError.entries.map { it.code },
        )
    }

    @Test
    fun theTokenIsRedactedFromTextAndMatchesInConstantTimeSemantics() {
        val material = Fixture().start()
        assertFalse(material.toString().contains(material.token.bytes.joinToString("") { "%02x".format(it) }))
        assertEquals("PairingToken([REDACTED])", material.token.toString())
        assertTrue(material.token.matches(material.token.bytes.copyOf()))
        assertFalse(material.token.matches(ByteArray(32)))
        assertFalse(material.token.matches(ByteArray(31)))
        val paired = Effect.SendPaired(material.sessionId, 1, DESKTOP_FINGERPRINT, Limits())
        assertTrue(paired.toString().contains("[REDACTED]"))
        assertFalse(paired.toString().contains("4444"))
    }
}

class PairingWindowTest {
    private fun assertRefusedAndReplaced(effects: List<Effect>, reason: ReasonCode, fixture: Fixture) {
        assertEquals(Effect.RejectPeer(reason), effects.first())
        assertIs<Effect.PresentPairing>(effects.last())
        assertEquals(SessionState.PAIRING_READY, fixture.core.state)
        assertEquals(2L, fixture.core.epoch)
    }

    @Test
    fun aQrIsAcceptedJustBeforeItsWindowCloses() {
        val fixture = Fixture()
        val material = fixture.start()
        fixture.clock.advance(PAIRING_WINDOW_MILLIS - 1)
        fixture.handle(SessionEvent.PeerConnected)
        fixture.authenticate(material.token.bytes)
        assertEquals(SessionState.NEGOTIATING, fixture.core.state)
    }

    @Test
    fun anExpiredQrIsRefusedBeforeTheHandshakeAndReplaced() {
        val fixture = Fixture()
        fixture.start()
        fixture.clock.advance(PAIRING_WINDOW_MILLIS)
        assertRefusedAndReplaced(fixture.handle(SessionEvent.PeerConnected), ReasonCode.QR_EXPIRED, fixture)
    }

    @Test
    fun anExpiredTokenIsRefusedAtAuthenticationAndReplaced() {
        val fixture = Fixture()
        val material = fixture.handshaking()
        fixture.clock.advance(PAIRING_WINDOW_MILLIS)
        assertRefusedAndReplaced(fixture.authenticate(material.token.bytes), ReasonCode.TOKEN_EXPIRED, fixture)
    }

    @Test
    fun aClockThatMovesBackwardsDoesNotInvalidateTheSession() {
        val fixture = Fixture(start = 1_000)
        val material = fixture.start()
        fixture.clock.set(Timestamp(10))
        fixture.handle(SessionEvent.PeerConnected)
        fixture.authenticate(material.token.bytes)
        assertEquals(SessionState.NEGOTIATING, fixture.core.state)
    }

    @Test
    fun aStaleEpochIsDroppedAndCounted() {
        val fixture = Fixture()
        fixture.start()
        assertEquals(0L, fixture.core.protocolViolations)
        assertTrue(fixture.core.admitFrameEpoch(1))
        assertTrue(fixture.core.admitFrameEpoch(5))
        assertFalse(fixture.core.admitFrameEpoch(0))
        assertEquals(1L, fixture.core.protocolViolations)
    }

    @Test
    fun withoutAnySessionEveryEpochIsAdmitted() {
        val fixture = Fixture()
        assertTrue(fixture.core.admitFrameEpoch(0))
        assertEquals(0L, fixture.core.protocolViolations)
    }

    @Test
    fun coresSharingOneEpochSourceNeverReuseAnEpoch() {
        val epochs = EpochSource()
        val first = Fixture(epochs = epochs).start()
        val second = Fixture(epochs = epochs).start()
        assertEquals(listOf(1L, 2L), listOf(first.epoch, second.epoch))
    }

    @Test
    fun aSpentEpochRangeRefusesNewMaterialWithoutLeavingIdle() {
        val epochs = EpochSource.startingAt(0xFFFF_FFFFL)
        assertEquals(0xFFFF_FFFFL, Fixture(epochs = epochs).start().epoch)

        val exhausted = Fixture(epochs = epochs)
        assertEquals("invalid_transition", exhausted.rejection(SessionEvent.StartPairing).code)
        assertEquals(SessionState.IDLE, exhausted.core.state)
    }

    @Test
    fun anExhaustedEpochSourceDuringReplacementFallsBackToIdle() {
        val fixture = Fixture(epochs = EpochSource.startingAt(0xFFFF_FFFFL))
        val material = fixture.handshaking()
        fixture.clock.advance(PAIRING_WINDOW_MILLIS)
        val effects = fixture.authenticate(material.token.bytes)
        assertEquals(listOf<Effect>(Effect.RejectPeer(ReasonCode.TOKEN_EXPIRED)), effects)
        assertEquals(SessionState.IDLE, fixture.core.state)
        assertNull(fixture.core.epoch)
    }
}

class SupersedingTest {
    @Test
    fun startingAPairingWhileConnectedKeepsThePredecessor() {
        val fixture = Fixture()
        val predecessor = fixture.connected()
        val successor = fixture.start()
        assertEquals(SessionState.CONNECTED, fixture.core.state)
        assertEquals(predecessor.sessionId, fixture.core.sessionId)
        assertEquals(1L, fixture.core.epoch)
        assertEquals(2L, successor.epoch)
    }

    @Test
    fun aValidSuccessorClosesThePredecessorBeforeItIsPaired() {
        val fixture = Fixture()
        fixture.connected()
        val successor = fixture.start()
        fixture.handle(SessionEvent.PeerConnected)
        val effects = fixture.authenticate(successor.token.bytes)
        assertEquals(Effect.SendClose(ReasonCode.SUPERSEDED), effects[0])
        assertEquals(Effect.End, effects[1])
        assertIs<Effect.SendPaired>(effects[2])
        assertEquals(3, effects.size)
        assertEquals(SessionState.NEGOTIATING, fixture.core.state)
        assertEquals(SessionState.SUPERSEDED, fixture.core.retiredState)
        assertEquals(2L, fixture.core.epoch)
    }

    @Test
    fun anUnusedQrThatExpiresKeepsThePredecessorConnected() {
        val fixture = Fixture()
        fixture.connected()
        fixture.start()
        fixture.clock.advance(PAIRING_WINDOW_MILLIS)
        val effects = fixture.handle(SessionEvent.PeerConnected)
        assertEquals(listOf<Effect>(Effect.RejectPeer(ReasonCode.QR_EXPIRED)), effects)
        assertEquals(SessionState.CONNECTED, fixture.core.state)
        assertEquals(1L, fixture.core.epoch)
    }

    @Test
    fun aSuccessorTokenThatExpiresAfterItsPeerConnectedKeepsThePredecessor() {
        val fixture = Fixture()
        fixture.connected()
        val successor = fixture.start()
        fixture.handle(SessionEvent.PeerConnected)
        fixture.clock.advance(PAIRING_WINDOW_MILLIS)
        val effects = fixture.authenticate(successor.token.bytes)
        assertEquals(listOf<Effect>(Effect.RejectPeer(ReasonCode.TOKEN_EXPIRED)), effects)
        assertEquals(SessionState.CONNECTED, fixture.core.state)
    }

    @Test
    fun aWrongTokenForTheSuccessorLeavesThePredecessorConnected() {
        val fixture = Fixture()
        fixture.connected()
        fixture.start()
        fixture.handle(SessionEvent.PeerConnected)
        assertEquals(PairingError.TOKEN_INVALID, fixture.refusal(pairFrame(ByteArray(32) { 9 })))
        assertEquals(SessionState.CONNECTED, fixture.core.state)
    }

    @Test
    fun anInvalidFrameFromTheSuccessorDiscardsItsQrAndKeepsThePredecessor() {
        val fixture = Fixture()
        fixture.connected()
        fixture.start()
        fixture.handle(SessionEvent.PeerConnected)
        assertEquals(PairingError.PROTOCOL_VIOLATION, fixture.refusal(ping()))
        assertEquals(SessionState.CONNECTED, fixture.core.state)
        assertEquals(
            listOf<Effect>(Effect.RejectPeer(ReasonCode.SUPERSEDED)),
            fixture.handle(SessionEvent.PeerConnected),
        )
    }

    @Test
    fun aPairFrameBeforeTheSuccessorConnectsDoesNotEndThePredecessor() {
        val fixture = Fixture()
        fixture.connected()
        val successor = fixture.start()
        assertEquals(PairingError.PROTOCOL_VIOLATION, fixture.refusal(pairFrame(successor.token.bytes)))
        assertEquals(SessionState.CONNECTED, fixture.core.state)
    }

    @Test
    fun theConsumedPredecessorTokenReplayedWhileConnectedIsRefusedAsSuperseded() {
        val fixture = Fixture()
        val predecessor = fixture.connected()
        assertEquals(PairingError.SUPERSEDED, fixture.refusal(pairFrame(predecessor.token.bytes)))
        assertEquals(SessionState.CONNECTED, fixture.core.state)
    }

    @Test
    fun aPeerConnectingWithoutAPendingQrIsRefusedAsSuperseded() {
        val negotiating = Fixture().also { it.negotiating() }
        assertEquals(
            listOf<Effect>(Effect.RejectPeer(ReasonCode.SUPERSEDED)),
            negotiating.handle(SessionEvent.PeerConnected),
        )
        assertEquals(SessionState.NEGOTIATING, negotiating.core.state)
        val connected = Fixture().also { it.connected() }
        assertEquals(
            listOf<Effect>(Effect.RejectPeer(ReasonCode.SUPERSEDED)),
            connected.handle(SessionEvent.PeerConnected),
        )
        assertEquals(SessionState.CONNECTED, connected.core.state)
    }

    @Test
    fun theReplacedTokenIsRefusedAsSupersededWhileTheSuccessorNegotiates() {
        val fixture = Fixture()
        val predecessor = fixture.connected()
        val successor = fixture.start()
        fixture.handle(SessionEvent.PeerConnected)
        fixture.authenticate(successor.token.bytes)
        assertEquals(PairingError.SUPERSEDED, fixture.refusal(pairFrame(predecessor.token.bytes)))
        assertEquals(PairingError.SUPERSEDED, fixture.refusal(pairFrame(successor.token.bytes)))
        assertEquals(SessionState.NEGOTIATING, fixture.core.state)
    }

    @Test
    fun aFirstFrameThatReusesNoConsumedTokenIsAProtocolViolationWhileNegotiating() {
        val fixture = Fixture()
        fixture.negotiating()
        assertEquals(PairingError.PROTOCOL_VIOLATION, fixture.refusal(ping()))
        val connected = Fixture().also { it.connected() }
        assertEquals(PairingError.PROTOCOL_VIOLATION, connected.refusal(ping()))
        assertEquals(SessionState.CONNECTED, connected.core.state)
    }

    @Test
    fun aMalformedFrameOnAConnectedSessionWithAPendingQrEndsNothing() {
        val fixture = Fixture()
        fixture.connected()
        fixture.start()
        fixture.handle(SessionEvent.PeerConnected)
        fixture.refusal("{".encodeToByteArray())
        assertEquals(SessionState.CONNECTED, fixture.core.state)
    }
}

class SignalingAdmissionTest {
    private fun refusal(result: AdmitResult): SignalingRefusal = assertIs<AdmitResult.Refused>(result).refusal

    @Test
    fun aFrameAtTheSizeLimitIsAdmitted() {
        val fixture = Fixture().also { it.negotiating() }
        val frame = ping(MAX_SIGNALING_MESSAGE_BYTES - ping().size)
        assertEquals(MAX_SIGNALING_MESSAGE_BYTES, frame.size)
        assertEquals(SignalingMessage.Ping, assertIs<AdmitResult.Admitted>(fixture.core.admitSignalingFrame(frame)).message)
    }

    @Test
    fun aFrameAboveTheSizeLimitEndsAnEstablishedSession() {
        for (build in listOf<() -> Fixture>({ Fixture().also { it.negotiating() } }, { Fixture().also { it.connected() } })) {
            val fixture = build()
            val refusal = refusal(fixture.core.admitSignalingFrame(ping(MAX_SIGNALING_MESSAGE_BYTES)))
            assertEquals(ReasonCode.SECURITY_VIOLATION, refusal.code)
            assertEquals(listOf(Effect.SendClose(ReasonCode.SECURITY_VIOLATION), Effect.End), refusal.effects)
            assertEquals(SessionState.ENDED, fixture.core.state)
        }
    }

    @Test
    fun aFrameAboveTheSizeLimitBeforeTheSessionIsEstablishedKeepsTheState() {
        val fixture = Fixture().also { it.handshaking() }
        val refusal = refusal(fixture.core.admitSignalingFrame(ping(MAX_SIGNALING_MESSAGE_BYTES)))
        assertEquals(ReasonCode.SECURITY_VIOLATION, refusal.code)
        assertEquals(emptyList(), refusal.effects)
        assertEquals(SessionState.HANDSHAKING, fixture.core.state)
    }

    @Test
    fun aSignalingFrameWithoutAnEstablishedSessionIsAProtocolViolation() {
        val fixture = Fixture()
        assertEquals(ReasonCode.PROTOCOL_VIOLATION, refusal(fixture.core.admitSignalingFrame(ping())).code)
    }

    @Test
    fun aFrameOutsideTheSchemaIsRefusedWithOneCodeAndKeepsTheSession() {
        val fixture = Fixture().also { it.negotiating() }
        val unknown = """{"version":1,"type":"ping","extra":1}""".encodeToByteArray()
        val future = """{"version":2,"type":"ping"}""".encodeToByteArray()
        assertEquals(ReasonCode.INVALID_MESSAGE, refusal(fixture.core.admitSignalingFrame(unknown)).code)
        assertEquals(ReasonCode.UNSUPPORTED_VERSION, refusal(fixture.core.admitSignalingFrame(future)).code)
        assertEquals(ReasonCode.INVALID_MESSAGE, refusal(fixture.core.admitSignalingFrame(byteArrayOf(0xff.toByte()))).code)
        assertEquals(SessionState.NEGOTIATING, fixture.core.state)
    }

    @Test
    fun messagesThatOnlyTheDesktopSendsAreRefusedWhenReceived() {
        val fixture = Fixture().also { it.negotiating() }
        val paired = SignalingEncoder.encode(
            SignalingMessage.Paired(ByteArray(16), 1, ByteArray(32), Limits()),
        ).encodeToByteArray()
        val answer = SignalingEncoder.encode(
            SignalingMessage.Answer(1, "v=0\r\nm=video 9 UDP/TLS/RTP/SAVPF 96\r\na=recvonly\r\n"),
        ).encodeToByteArray()
        val error = SignalingEncoder.encode(
            SignalingMessage.Error(ReasonCode.INTERNAL_ERROR, null),
        ).encodeToByteArray()
        for (frame in listOf(paired, answer, error)) {
            assertEquals(ReasonCode.INVALID_MESSAGE, refusal(fixture.core.admitSignalingFrame(frame)).code)
        }
    }

    @Test
    fun aSecondPairFrameOnAnEstablishedSessionIsAProtocolViolation() {
        val fixture = Fixture().also { it.negotiating() }
        assertEquals(
            ReasonCode.PROTOCOL_VIOLATION,
            refusal(fixture.core.admitSignalingFrame(pairFrame(ByteArray(32)))).code,
        )
    }

    @Test
    fun aConformingOfferIsAdmitted() {
        val fixture = Fixture().also { it.negotiating() }
        assertIs<SignalingMessage.Offer>(assertIs<AdmitResult.Admitted>(fixture.core.admitSignalingFrame(offer())).message)
    }
}
