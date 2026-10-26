package remotecam.session

import java.security.MessageDigest
import remotecam.protocol.Limits
import remotecam.protocol.ReasonCode
import remotecam.protocol.SignalingParser

/** Largest signaling frame the core admits, shared with the strict parser. */
const val MAX_SIGNALING_MESSAGE_BYTES: Int = SignalingParser.MAX_MESSAGE_BYTES

/** Time between the first negotiation completing and the first media frame. */
const val FIRST_MEDIA_FRAME_TIMEOUT_MILLIS: Long = 3_000

/** Lifetime of one pairing QR and its token. */
const val PAIRING_WINDOW_MILLIS: Long = 120_000

/** The lifecycle position of the single session a mobile may hold. */
enum class SessionState(val label: String) {
    IDLE("idle"),
    PAIRING_READY("pairing_ready"),
    HANDSHAKING("handshaking"),
    NEGOTIATING("negotiating"),
    CONNECTED("connected"),
    ENDED("ended"),
    SUPERSEDED("superseded"),
}

/** A point on the injected monotonic clock, in milliseconds. */
data class Timestamp(val millis: Long) : Comparable<Timestamp> {
    override fun compareTo(other: Timestamp): Int = millis.compareTo(other.millis)

    companion object {
        val ZERO = Timestamp(0)
    }
}

/** The time source the core reads. It is injected, never global. */
interface Clock {
    fun now(): Timestamp
}

/** The randomness source the core draws from. It is injected, never global. */
interface RandomSource {
    fun fill(destination: ByteArray)
}

/**
 * The process-local epoch sequence (ADR-0004): the first epoch is 1, each issue
 * adds one, and the unsigned 32-bit range is spent once it passes `0xFFFFFFFF`.
 */
class EpochSource private constructor(first: Long) {
    private var next: Long? = first

    constructor() : this(1L)

    @Synchronized
    internal fun issue(): Long? {
        val epoch = next ?: return null
        next = if (epoch < MAX_EPOCH) epoch + 1 else null
        return epoch
    }

    companion object {
        private const val MAX_EPOCH = 0xFFFF_FFFFL

        /** An epoch sequence whose next issued epoch is [first]. */
        fun startingAt(first: Long): EpochSource = EpochSource(first)
    }
}

/** The identity assigned to one pairing attempt. */
class SessionId(bytes: ByteArray) {
    val bytes: ByteArray = bytes.copyOf()

    fun hex(): String = bytes.joinToString("") { "%02x".format(it) }

    override fun equals(other: Any?): Boolean = other is SessionId && bytes.contentEquals(other.bytes)

    override fun hashCode(): Int = bytes.contentHashCode()

    override fun toString(): String = "SessionId(${hex()})"

    companion object {
        const val LENGTH = 16

        fun generate(random: RandomSource): SessionId = SessionId(ByteArray(LENGTH).also(random::fill))
    }
}

/** The single-use secret of one pairing attempt. Its text form never shows the bytes. */
class PairingToken(bytes: ByteArray) {
    val bytes: ByteArray = bytes.copyOf()

    /** Compares in constant time. */
    fun matches(other: ByteArray): Boolean = MessageDigest.isEqual(bytes, other)

    override fun toString(): String = "PairingToken([REDACTED])"
}

/** The short-lived material the desktop presents for one pairing attempt. */
class PairingMaterial(val sessionId: SessionId, val epoch: Long, val token: PairingToken) {
    override fun toString(): String = "PairingMaterial(sessionId=$sessionId, epoch=$epoch, token=$token)"
}

/** A media frame reported by the media port. */
data class MediaFrame(val sequence: Long, val timestamp: Timestamp)

/** An input the core decides on. */
sealed interface SessionEvent {
    data object StartPairing : SessionEvent
    data object PeerConnected : SessionEvent
    data object NegotiationCompleted : SessionEvent
    data class FirstMediaFrame(val frame: MediaFrame) : SessionEvent
    data object FirstFrameTimedOut : SessionEvent
    data object EndSession : SessionEvent
}

/** An action an adapter performs after the core decides. */
sealed interface Effect {
    data class PresentPairing(val material: PairingMaterial) : Effect
    data class RejectPeer(val reason: ReasonCode) : Effect
    data class SendClose(val reason: ReasonCode) : Effect

    class SendPaired(
        val sessionId: SessionId,
        val epoch: Long,
        dtlsFingerprint: ByteArray,
        val acceptedLimits: Limits,
    ) : Effect {
        val dtlsFingerprint: ByteArray = dtlsFingerprint.copyOf()

        override fun toString(): String =
            "SendPaired(sessionId=$sessionId, epoch=$epoch, dtlsFingerprint=[REDACTED], " +
                "acceptedLimits=$acceptedLimits)"
    }

    data object StartMedia : Effect
    data object End : Effect
}

/** A rejected first-frame pairing attempt. */
enum class PairingError(val code: ReasonCode) {
    PROTOCOL_VIOLATION(ReasonCode.PROTOCOL_VIOLATION),
    TOKEN_INVALID(ReasonCode.TOKEN_INVALID),
    TOKEN_ALREADY_USED(ReasonCode.TOKEN_ALREADY_USED),
    SUPERSEDED(ReasonCode.SUPERSEDED),
    SECURITY_VIOLATION(ReasonCode.SECURITY_VIOLATION),
}

/** Why the core rejected an event: a local state-machine code, not a wire reason. */
data class TransitionError(val from: SessionState, val event: SessionEvent) {
    val code: String = "invalid_transition"
}

/** A refused signaling frame: exactly one closed code and the effects it implies. */
class SignalingRefusal internal constructor(val code: ReasonCode, val effects: List<Effect>)

sealed interface TransitionResult {
    data class Accepted(val effects: List<Effect>) : TransitionResult
    data class Rejected(val error: TransitionError) : TransitionResult
}

sealed interface PairingResult {
    data class Authenticated(val effects: List<Effect>) : PairingResult
    data class Refused(val error: PairingError) : PairingResult
}

sealed interface AdmitResult {
    data class Admitted(val message: remotecam.protocol.SignalingMessage) : AdmitResult
    data class Refused(val refusal: SignalingRefusal) : AdmitResult
}
