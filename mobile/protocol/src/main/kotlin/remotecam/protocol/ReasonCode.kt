package remotecam.protocol

/**
 * The closed reason-code set of `protocol/reason-codes.json`.
 *
 * A code is a stable wire identifier. Changing its spelling or meaning requires
 * a protocol version bump. The human-readable text belongs to the presentation
 * layer and is deliberately absent here.
 */
enum class ReasonCategory(val wireName: String) {
    PAIRING_REJECTION("pairing_rejection"),
    SESSION_CLOSE("session_close"),
    SIGNALING_ERROR("signaling_error"),
}

private fun pairing() = listOf(ReasonCategory.PAIRING_REJECTION)
private fun close() = listOf(ReasonCategory.SESSION_CLOSE)
private fun signaling() = listOf(ReasonCategory.SIGNALING_ERROR)

enum class ReasonCode(
    val wireName: String,
    val categories: List<ReasonCategory>,
    val securityEvent: Boolean,
    val userActionable: Boolean,
) {
    INVALID_QR_PAYLOAD("invalid_qr_payload", pairing(), false, true),
    UNSUPPORTED_QR_VERSION("unsupported_qr_version", pairing(), false, true),
    QR_EXPIRED("qr_expired", pairing(), false, true),
    NO_PRIVATE_ENDPOINT("no_private_endpoint", pairing(), false, true),
    DESKTOP_UNREACHABLE("desktop_unreachable", pairing(), false, true),
    TLS_HANDSHAKE_FAILED("tls_handshake_failed", pairing(), false, false),
    CERTIFICATE_PIN_MISMATCH("certificate_pin_mismatch", pairing(), true, false),
    CERTIFICATE_CHAIN_REJECTED("certificate_chain_rejected", pairing(), true, false),
    TLS_VERSION_UNSUPPORTED("tls_version_unsupported", pairing(), true, false),
    TOKEN_INVALID("token_invalid", pairing(), false, false),
    TOKEN_EXPIRED("token_expired", pairing(), false, false),
    TOKEN_ALREADY_USED("token_already_used", pairing(), true, false),
    SOURCE_ADDRESS_NOT_PRIVATE("source_address_not_private", pairing(), true, false),
    SOURCE_SUBNET_MISMATCH("source_subnet_mismatch", pairing(), true, false),
    RATE_LIMITED(
        "rate_limited",
        listOf(ReasonCategory.PAIRING_REJECTION, ReasonCategory.SIGNALING_ERROR),
        false,
        true,
    ),
    LISTENER_CLOSED("listener_closed", pairing(), true, false),
    SUPERSEDED(
        "superseded",
        listOf(ReasonCategory.PAIRING_REJECTION, ReasonCategory.SESSION_CLOSE),
        false,
        true,
    ),
    PROTOCOL_VIOLATION("protocol_violation", pairing(), false, false),
    INTERNAL_ERROR(
        "internal_error",
        listOf(
            ReasonCategory.PAIRING_REJECTION,
            ReasonCategory.SESSION_CLOSE,
            ReasonCategory.SIGNALING_ERROR,
        ),
        false,
        true,
    ),
    ENDED_BY_MOBILE("ended_by_mobile", close(), false, true),
    ENDED_BY_DESKTOP("ended_by_desktop", close(), false, true),
    TIMEOUT("timeout", close(), false, true),
    MEDIA_FAILURE("media_failure", close(), false, true),
    SECURITY_VIOLATION("security_violation", close(), true, false),
    INVALID_MESSAGE("invalid_message", signaling(), false, false),
    UNSUPPORTED_VERSION("unsupported_version", signaling(), false, false),
    ;

    companion object {
        /** Parses a wire spelling; unknown spellings are a contract break. */
        fun fromWire(value: String): ReasonCode? = entries.firstOrNull { it.wireName == value }
    }
}
