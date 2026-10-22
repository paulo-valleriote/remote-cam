package remotecam.protocol

/** Lowercase hexadecimal helpers shared by the contract consumer and its tests. */
internal object Hex {
    private const val DIGITS = "0123456789abcdef"

    /** Decodes exactly [byteLength] bytes, rejecting uppercase and non-hex input. */
    fun decodeExact(input: String, byteLength: Int): ByteArray? {
        if (input.length != byteLength * 2) return null
        val output = ByteArray(byteLength)
        for (index in 0 until byteLength) {
            val high = nibble(input[index * 2]) ?: return null
            val low = nibble(input[index * 2 + 1]) ?: return null
            output[index] = ((high shl 4) or low).toByte()
        }
        return output
    }

    fun decode(input: String): ByteArray? {
        if (input.length % 2 != 0) return null
        return decodeExact(input, input.length / 2)
    }

    fun encode(bytes: ByteArray): String {
        val builder = StringBuilder(bytes.size * 2)
        for (byte in bytes) {
            val value = byte.toInt() and 0xff
            builder.append(DIGITS[value ushr 4])
            builder.append(DIGITS[value and 0x0f])
        }
        return builder.toString()
    }

    private fun nibble(value: Char): Int? = when (value) {
        in '0'..'9' -> value - '0'
        in 'a'..'f' -> value - 'a' + 10
        else -> null
    }
}
