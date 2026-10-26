package remotecam.session

import java.nio.ByteBuffer
import java.nio.charset.CharacterCodingException
import java.nio.charset.CodingErrorAction
import remotecam.protocol.Limits
import remotecam.protocol.ReasonCode
import remotecam.protocol.SignalingMessage
import remotecam.protocol.SignalingParseResult
import remotecam.protocol.SignalingParser

private class SessionIdentity(val sessionId: SessionId, val epoch: Long)

private class PendingQr(
    val identity: SessionIdentity,
    val token: PairingToken,
    val expiresAt: Timestamp,
    var peerConnected: Boolean,
)

/**
 * The mobile session core. It mirrors the Rust `SessionCore` decision for
 * decision (states, transitions, epoch, validation and reason codes) and
 * performs no I/O. `protocol/vectors` replays the same scenarios on both.
 */
class SessionCore(
    private val clock: Clock,
    private val random: RandomSource,
    desktopDtlsFingerprint: ByteArray,
    private val epochs: EpochSource = EpochSource(),
) {
    private val desktopDtlsFingerprint = desktopDtlsFingerprint.copyOf()
    private var active: SessionIdentity? = null
    private var pendingQr: PendingQr? = null
    private var hasRetiredPredecessor = false
    private val consumedTokens = mutableListOf<PairingToken>()
    private var firstFrameDeadline: Timestamp? = null

    var state: SessionState = SessionState.IDLE
        private set

    /** The injected clock reading captured at the last accepted transition. */
    var lastTransitionAt: Timestamp = Timestamp.ZERO
        private set

    /** Frames discarded for carrying an epoch below the current session's. */
    var protocolViolations: Long = 0
        private set

    val sessionId: SessionId? get() = currentIdentity()?.sessionId

    val epoch: Long? get() = currentIdentity()?.epoch

    val retiredState: SessionState? get() = if (hasRetiredPredecessor) SessionState.SUPERSEDED else null

    /** Discards a frame whose epoch is below the current session's, and counts it. */
    fun admitFrameEpoch(frameEpoch: Long): Boolean {
        val current = currentIdentity()
        if (current != null && frameEpoch < current.epoch) {
            protocolViolations += 1
            return false
        }
        return true
    }

    /** Decides on [event] and returns the effects an adapter must perform. */
    fun handle(event: SessionEvent): TransitionResult {
        val (nextState, effects) = transition(event)
            ?: return TransitionResult.Rejected(TransitionError(state, event))
        state = nextState
        lastTransitionAt = clock.now()
        return TransitionResult.Accepted(effects)
    }

    /** Authenticates the required first signaling frame from a connected peer. */
    fun authenticateFirstFrame(frame: ByteArray): PairingResult {
        if (frame.size > MAX_SIGNALING_MESSAGE_BYTES) {
            refusePairing()
            return PairingResult.Refused(PairingError.SECURITY_VIOLATION)
        }

        val presented = parsePair(frame)

        if (presented != null && isConsumed(presented.token)) {
            return PairingResult.Refused(consumedTokenRefusal())
        }

        if (state == SessionState.NEGOTIATING ||
            (state == SessionState.CONNECTED && pendingQr == null)
        ) {
            return PairingResult.Refused(PairingError.PROTOCOL_VIOLATION)
        }

        if (!awaitsPairFrame()) {
            refusePairing()
            return PairingResult.Refused(PairingError.PROTOCOL_VIOLATION)
        }

        if (presented == null) {
            refusePairing()
            return PairingResult.Refused(PairingError.PROTOCOL_VIOLATION)
        }

        if (pairingWindowClosed()) {
            val (nextState, effects) = retireExpiredPairing(ReasonCode.TOKEN_EXPIRED)
            state = nextState
            lastTransitionAt = clock.now()
            return PairingResult.Authenticated(effects)
        }

        val pending = pendingQr?.takeIf { it.token.matches(presented.token.bytes) }
        if (pending == null) {
            if (state == SessionState.CONNECTED) refusePairing()
            return PairingResult.Refused(PairingError.TOKEN_INVALID)
        }
        pendingQr = null
        return PairingResult.Authenticated(activatePairing(pending, presented.limits))
    }

    /** Validates an inbound frame on an established session, in the order of ADR-0015. */
    fun admitSignalingFrame(frame: ByteArray): AdmitResult {
        val established = state == SessionState.NEGOTIATING || state == SessionState.CONNECTED
        if (frame.size > MAX_SIGNALING_MESSAGE_BYTES) {
            if (!established) {
                return refuseKeepingSession(ReasonCode.SECURITY_VIOLATION)
            }
            pendingQr = null
            state = SessionState.ENDED
            lastTransitionAt = clock.now()
            return AdmitResult.Refused(
                SignalingRefusal(
                    ReasonCode.SECURITY_VIOLATION,
                    listOf(Effect.SendClose(ReasonCode.SECURITY_VIOLATION), Effect.End),
                ),
            )
        }
        if (!established) return refuseKeepingSession(ReasonCode.PROTOCOL_VIOLATION)

        val text = decodeUtf8(frame) ?: return refuseKeepingSession(ReasonCode.INVALID_MESSAGE)
        return when (val parsed = SignalingParser.parse(text)) {
            is SignalingParseResult.Rejected -> refuseKeepingSession(parsed.reason)
            is SignalingParseResult.Accepted -> when (parsed.message) {
                is SignalingMessage.Pair -> refuseKeepingSession(ReasonCode.PROTOCOL_VIOLATION)
                is SignalingMessage.Paired,
                is SignalingMessage.Answer,
                is SignalingMessage.Error,
                -> refuseKeepingSession(ReasonCode.INVALID_MESSAGE)
                else -> AdmitResult.Admitted(parsed.message)
            }
        }
    }

    private fun refuseKeepingSession(code: ReasonCode): AdmitResult =
        AdmitResult.Refused(SignalingRefusal(code, emptyList()))

    private fun decodeUtf8(frame: ByteArray): String? = try {
        Charsets.UTF_8.newDecoder()
            .onMalformedInput(CodingErrorAction.REPORT)
            .onUnmappableCharacter(CodingErrorAction.REPORT)
            .decode(ByteBuffer.wrap(frame))
            .toString()
    } catch (_: CharacterCodingException) {
        null
    }

    private class Presented(val token: PairingToken, val limits: Limits)

    private fun parsePair(frame: ByteArray): Presented? {
        val text = decodeUtf8(frame) ?: return null
        val parsed = SignalingParser.parse(text) as? SignalingParseResult.Accepted ?: return null
        val pair = parsed.message as? SignalingMessage.Pair ?: return null
        return Presented(PairingToken(pair.token), pair.limits)
    }

    private fun currentIdentity(): SessionIdentity? = active ?: pendingQr?.identity

    private fun awaitsPairFrame(): Boolean =
        (state == SessionState.HANDSHAKING || state == SessionState.CONNECTED) &&
            pendingQr?.peerConnected == true

    private fun activatePairing(pending: PendingQr, acceptedLimits: Limits): List<Effect> {
        val effects = mutableListOf<Effect>()
        hasRetiredPredecessor = active != null
        if (hasRetiredPredecessor) {
            effects.add(Effect.SendClose(ReasonCode.SUPERSEDED))
            effects.add(Effect.End)
        }
        consumedTokens.add(pending.token)
        active = pending.identity
        state = SessionState.NEGOTIATING
        lastTransitionAt = clock.now()
        effects.add(
            Effect.SendPaired(
                pending.identity.sessionId,
                pending.identity.epoch,
                desktopDtlsFingerprint,
                acceptedLimits,
            ),
        )
        return effects
    }

    /** Checks every consumed token, without stopping at the first match. */
    private fun isConsumed(token: PairingToken): Boolean =
        consumedTokens.fold(false) { found, used -> used.matches(token.bytes) or found }

    private fun consumedTokenRefusal(): PairingError =
        if (state == SessionState.NEGOTIATING || state == SessionState.CONNECTED) {
            PairingError.SUPERSEDED
        } else {
            PairingError.TOKEN_ALREADY_USED
        }

    private fun refusePairing() {
        if (state != SessionState.CONNECTED) state = SessionState.ENDED
        pendingQr = null
        lastTransitionAt = clock.now()
    }

    private fun pairingWindowClosed(): Boolean =
        pendingQr?.let { clock.now() >= it.expiresAt } ?: false

    private fun markPeerConnected() {
        pendingQr?.peerConnected = true
    }

    private fun retireExpiredPairing(reason: ReasonCode): Pair<SessionState, List<Effect>> {
        pendingQr = null
        if (active != null) {
            return SessionState.CONNECTED to listOf(Effect.RejectPeer(reason))
        }
        val material = beginPairing()
        if (material == null) {
            clearPairing()
            return SessionState.IDLE to listOf(Effect.RejectPeer(reason))
        }
        return SessionState.PAIRING_READY to
            listOf(Effect.RejectPeer(reason), Effect.PresentPairing(material))
    }

    private fun clearPairing() {
        pendingQr = null
        active = null
        hasRetiredPredecessor = false
    }

    private fun beginPairing(): PairingMaterial? {
        val epoch = epochs.issue() ?: return null
        val sessionId = SessionId.generate(random)
        val tokenBytes = ByteArray(32).also(random::fill)
        val material = PairingMaterial(sessionId, epoch, PairingToken(tokenBytes))
        pendingQr = PendingQr(
            identity = SessionIdentity(sessionId, epoch),
            token = PairingToken(tokenBytes),
            expiresAt = Timestamp(clock.now().millis + PAIRING_WINDOW_MILLIS),
            peerConnected = false,
        )
        tokenBytes.fill(0)
        return material
    }

    private fun transition(event: SessionEvent): Pair<SessionState, List<Effect>>? = when {
        (state == SessionState.IDLE || state == SessionState.CONNECTED) &&
            event == SessionEvent.StartPairing -> {
            val material = beginPairing()
            if (material == null) {
                null
            } else {
                val next = if (state == SessionState.IDLE) SessionState.PAIRING_READY else SessionState.CONNECTED
                next to listOf(Effect.PresentPairing(material))
            }
        }

        (state == SessionState.PAIRING_READY || state == SessionState.CONNECTED) &&
            event == SessionEvent.PeerConnected &&
            (state == SessionState.PAIRING_READY || pendingQr != null) -> {
            if (pairingWindowClosed()) {
                retireExpiredPairing(ReasonCode.QR_EXPIRED)
            } else {
                markPeerConnected()
                val next = if (state == SessionState.PAIRING_READY) SessionState.HANDSHAKING else SessionState.CONNECTED
                next to emptyList()
            }
        }

        (state == SessionState.NEGOTIATING || state == SessionState.CONNECTED) &&
            event == SessionEvent.PeerConnected &&
            pendingQr == null ->
            state to listOf(Effect.RejectPeer(ReasonCode.SUPERSEDED))

        state == SessionState.NEGOTIATING &&
            event == SessionEvent.NegotiationCompleted &&
            firstFrameDeadline == null -> {
            firstFrameDeadline = Timestamp(clock.now().millis + FIRST_MEDIA_FRAME_TIMEOUT_MILLIS)
            SessionState.NEGOTIATING to listOf(Effect.StartMedia)
        }

        state == SessionState.NEGOTIATING && event is SessionEvent.FirstMediaFrame -> {
            val deadline = firstFrameDeadline
            if (deadline != null && event.frame.timestamp <= deadline && clock.now() <= deadline) {
                firstFrameDeadline = null
                SessionState.CONNECTED to emptyList()
            } else {
                null
            }
        }

        state == SessionState.NEGOTIATING &&
            event == SessionEvent.FirstFrameTimedOut &&
            firstFrameDeadline?.let { clock.now() > it } == true -> {
            firstFrameDeadline = null
            clearPairing()
            SessionState.IDLE to listOf(Effect.End)
        }

        state == SessionState.CONNECTED && event == SessionEvent.EndSession -> {
            pendingQr = null
            SessionState.ENDED to listOf(Effect.End)
        }

        else -> null
    }
}
