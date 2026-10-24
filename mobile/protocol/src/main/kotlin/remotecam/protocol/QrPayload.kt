package remotecam.protocol

/** Result of decoding a QR payload: a payload or one closed rejection code. */
sealed interface QrDecodeResult {
    data class Accepted(val payload: QrPayload) : QrDecodeResult
    data class Rejected(val reason: ReasonCode) : QrDecodeResult
}

/** Failure while encoding a payload. */
class QrEncodeException(message: String) : Exception(message)

/** One endpoint record: family, 16-byte address and TCP port. */
class QrEndpoint(
    val family: Int,
    val address: ByteArray,
    val port: Int,
) {
    init {
        require(address.size == 16) { "an endpoint address is always 16 bytes" }
    }

    fun addressString(): String? = Addresses.format(family, address)

    internal fun sortKey(): Pair<Int, List<Byte>> = family to address.toList()

    internal fun encode(): ByteArray {
        val record = ByteArray(QrPayload.ENDPOINT_BYTES)
        record[0] = family.toByte()
        address.copyInto(record, 1)
        record[17] = ((port ushr 8) and 0xff).toByte()
        record[18] = (port and 0xff).toByte()
        return record
    }

    override fun equals(other: Any?): Boolean =
        other is QrEndpoint && family == other.family && port == other.port &&
            address.contentEquals(other.address)

    override fun hashCode(): Int = 31 * (31 * family + port) + address.contentHashCode()

    companion object {
        fun fromAddress(family: Int, address: String, port: Int): QrEndpoint? {
            val packed = Addresses.parsePrivate(address) ?: return null
            val matchesFamily = when (family) {
                4 -> packed.take(12).all { it.toInt() == 0 }
                6 -> true
                else -> false
            }
            return if (matchesFamily) QrEndpoint(family, packed, port) else null
        }
    }
}

/**
 * A decoded version 1 QR payload. The payload is always handled as raw bytes;
 * a decoder that returns UTF-8 text corrupts it because it contains arbitrary
 * bytes.
 */
class QrPayload(
    val sessionId: ByteArray,
    val epoch: Long,
    val expiresAt: Long,
    val pairingToken: ByteArray,
    val tlsFingerprint: ByteArray,
    val capabilities: Int,
    val endpoints: List<QrEndpoint>,
) {
    init {
        require(sessionId.size == 16) { "session_id is 16 bytes" }
        require(pairingToken.size == 32) { "pairing_token is 32 bytes" }
        require(tlsFingerprint.size == 32) { "tls_fingerprint is 32 bytes" }
    }

    /** Encodes the payload, sorting endpoints into the stable contract order. */
    fun encode(): ByteArray {
        if (endpoints.isEmpty()) throw QrEncodeException("no endpoints")
        for (endpoint in endpoints) {
            val packed = endpoint.address
            val valid = when (endpoint.family) {
                4 -> packed.take(12).all { it.toInt() == 0 } &&
                    Addresses.isPrivateIpv4(packed.copyOfRange(12, 16))
                6 -> Addresses.isPrivateIpv6(packed)
                else -> false
            }
            if (!valid) throw QrEncodeException("invalid endpoint")
        }

        val sorted = endpoints.sortedWith(compareBy({ it.family }, { Hex.encode(it.address) }))
        val endpointsLength = sorted.size * ENDPOINT_BYTES
        val encodedLength = HEADER_BYTES + endpointsLength
        if (encodedLength > MAX_PAYLOAD_BYTES) {
            throw QrEncodeException("payload would be $encodedLength bytes")
        }

        val output = ByteArray(encodedLength)
        MAGIC.copyInto(output, 0)
        output[4] = SUPPORTED_VERSION.toByte()
        output[5] = 0
        output[6] = ((endpointsLength ushr 8) and 0xff).toByte()
        output[7] = (endpointsLength and 0xff).toByte()
        sessionId.copyInto(output, 8)
        writeU32(output, 24, epoch)
        writeU64(output, 28, expiresAt)
        pairingToken.copyInto(output, 36)
        tlsFingerprint.copyInto(output, 68)
        output[100] = ((capabilities ushr 8) and 0xff).toByte()
        output[101] = (capabilities and 0xff).toByte()
        var offset = HEADER_BYTES
        for (endpoint in sorted) {
            endpoint.encode().copyInto(output, offset)
            offset += ENDPOINT_BYTES
        }
        return output
    }

    override fun equals(other: Any?): Boolean =
        other is QrPayload && epoch == other.epoch && expiresAt == other.expiresAt &&
            capabilities == other.capabilities && sessionId.contentEquals(other.sessionId) &&
            pairingToken.contentEquals(other.pairingToken) &&
            tlsFingerprint.contentEquals(other.tlsFingerprint) && endpoints == other.endpoints

    override fun hashCode(): Int {
        var result = sessionId.contentHashCode()
        result = 31 * result + epoch.hashCode()
        result = 31 * result + expiresAt.hashCode()
        result = 31 * result + pairingToken.contentHashCode()
        result = 31 * result + tlsFingerprint.contentHashCode()
        result = 31 * result + capabilities
        result = 31 * result + endpoints.hashCode()
        return result
    }

    companion object {
        val MAGIC: ByteArray = byteArrayOf(0x52, 0x43, 0x4d, 0x31)
        const val SUPPORTED_VERSION: Int = 1
        const val HEADER_BYTES: Int = 102
        const val ENDPOINT_BYTES: Int = 19
        const val MAX_PAYLOAD_BYTES: Int = 512
        private const val CAPABILITY_RESERVED_MASK: Int = 0xffc0

        fun decode(input: ByteArray, nowUnix: Long): QrDecodeResult = decodeInternal(input, nowUnix)

        private fun decodeInternal(input: ByteArray, nowUnix: Long?): QrDecodeResult {
            if (input.size < HEADER_BYTES) return rejected(ReasonCode.INVALID_QR_PAYLOAD)
            for (index in 0 until 4) {
                if (input[index] != MAGIC[index]) return rejected(ReasonCode.INVALID_QR_PAYLOAD)
            }
            if ((input[4].toInt() and 0xff) != SUPPORTED_VERSION) {
                return rejected(ReasonCode.UNSUPPORTED_QR_VERSION)
            }
            if (input[5].toInt() != 0) return rejected(ReasonCode.INVALID_QR_PAYLOAD)

            val capabilities = readU16(input, 100)
            if (capabilities and CAPABILITY_RESERVED_MASK != 0) {
                return rejected(ReasonCode.INVALID_QR_PAYLOAD)
            }
            val endpointsLength = readU16(input, 6)
            if (input.size != HEADER_BYTES + endpointsLength) {
                return rejected(ReasonCode.INVALID_QR_PAYLOAD)
            }
            if (endpointsLength == 0 || endpointsLength % ENDPOINT_BYTES != 0) {
                return rejected(ReasonCode.INVALID_QR_PAYLOAD)
            }

            val endpoints = mutableListOf<QrEndpoint>()
            for (offset in HEADER_BYTES until input.size step ENDPOINT_BYTES) {
                val decoded = try {
                    decodeEndpoint(input, offset)
                } catch (failure: EndpointFailure) {
                    return rejected(failure.reason)
                }
                endpoints.add(decoded)
            }

            val expiresAt = readU64(input, 28)
            if (nowUnix != null && expiresAt <= nowUnix) return rejected(ReasonCode.QR_EXPIRED)

            return QrDecodeResult.Accepted(
                QrPayload(
                    sessionId = input.copyOfRange(8, 24),
                    epoch = readU32(input, 24),
                    expiresAt = expiresAt,
                    pairingToken = input.copyOfRange(36, 68),
                    tlsFingerprint = input.copyOfRange(68, 100),
                    capabilities = capabilities,
                    endpoints = endpoints,
                ),
            )
        }

        private class EndpointFailure(val reason: ReasonCode) : Exception()

        private fun decodeEndpoint(input: ByteArray, offset: Int): QrEndpoint {
            val family = input[offset].toInt() and 0xff
            val address = input.copyOfRange(offset + 1, offset + 17)
            val port = readU16(input, offset + 17)
            return when (family) {
                4 -> {
                    val paddingOk = address.take(12).all { it.toInt() == 0 }
                    val ipv4 = address.copyOfRange(12, 16)
                    if (!paddingOk || !Addresses.isPrivateIpv4(ipv4)) {
                        throw EndpointFailure(ReasonCode.NO_PRIVATE_ENDPOINT)
                    }
                    QrEndpoint(4, address, port)
                }
                6 -> {
                    if (!Addresses.isPrivateIpv6(address)) {
                        throw EndpointFailure(ReasonCode.NO_PRIVATE_ENDPOINT)
                    }
                    QrEndpoint(6, address, port)
                }
                else -> throw EndpointFailure(ReasonCode.INVALID_QR_PAYLOAD)
            }
        }

        private fun rejected(reason: ReasonCode) = QrDecodeResult.Rejected(reason)

        private fun readU16(input: ByteArray, offset: Int): Int =
            ((input[offset].toInt() and 0xff) shl 8) or (input[offset + 1].toInt() and 0xff)

        private fun readU32(input: ByteArray, offset: Int): Long {
            var value = 0L
            for (index in 0 until 4) {
                value = (value shl 8) or (input[offset + index].toLong() and 0xff)
            }
            return value
        }

        private fun readU64(input: ByteArray, offset: Int): Long {
            var value = 0L
            for (index in 0 until 8) {
                value = (value shl 8) or (input[offset + index].toLong() and 0xff)
            }
            return value
        }

        private fun writeU32(output: ByteArray, offset: Int, value: Long) {
            for (index in 0 until 4) {
                output[offset + 3 - index] = ((value ushr (index * 8)) and 0xff).toByte()
            }
        }

        private fun writeU64(output: ByteArray, offset: Int, value: Long) {
            for (index in 0 until 8) {
                output[offset + 7 - index] = ((value ushr (index * 8)) and 0xff).toByte()
            }
        }
    }

    fun isStructurallyValid(): Boolean =
        try {
            decodeInternal(encode(), null) is QrDecodeResult.Accepted
        } catch (error: QrEncodeException) {
            false
        }

    fun isExpiredAt(nowUnix: Long): Boolean = expiresAt <= nowUnix

    /** Check if payload has at least one private endpoint. */
    fun hasPrivateEndpoints(): Boolean = endpoints.isNotEmpty()

    /** TLS fingerprint as hex string. */
    val tlsFingerprintHex: String = tlsFingerprint.joinToString("") { "%02x".format(it) }

    /** Pairing token as hex string. */
    val pairingTokenHex: String = pairingToken.joinToString("") { "%02x".format(it) }

    /** Session ID as hex string. */
    val sessionIdHex: String = sessionId.joinToString("") { "%02x".format(it) }
}
