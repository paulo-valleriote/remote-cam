package remotecam.protocol

import java.net.Inet6Address
import java.net.InetAddress

/**
 * Private-address rules shared by the signaling schema and the QR payload.
 *
 * IPv4: RFC 1918 only. APIPA 169.254/16 is deliberately excluded: it is not
 * reachable across a local network even though the proposed signaling JSON
 * Schema pattern lists it. IPv6: link-local fe80::/10 and ULA fc00::/7.
 */
internal object Addresses {
    fun parsePrivate(value: String): ByteArray? {
        if (value.contains(':')) {
            return parsePrivateIpv6(value)
        }
        val octets = parseIpv4(value) ?: return null
        if (!isPrivateIpv4(octets)) return null
        return ByteArray(12) + octets
    }

    fun isPrivateIpv4(octets: ByteArray): Boolean {
        if (octets.size != 4) return false
        val first = octets[0].toInt() and 0xff
        val second = octets[1].toInt() and 0xff
        return first == 10 ||
            (first == 172 && second in 16..31) ||
            (first == 192 && second == 168)
    }

    fun isPrivateIpv6(octets: ByteArray): Boolean {
        if (octets.size != 16) return false
        val first = octets[0].toInt() and 0xff
        val second = octets[1].toInt() and 0xff
        val linkLocal = first == 0xfe && (second and 0xc0) == 0x80
        val uniqueLocal = (first and 0xfe) == 0xfc
        return linkLocal || uniqueLocal
    }

    fun format(family: Int, address: ByteArray): String? = when (family) {
        4 -> {
            if (address.size != 16 || address.take(12).any { it.toInt() != 0 }) {
                null
            } else {
                (12 until 16).joinToString(".") { (address[it].toInt() and 0xff).toString() }
            }
        }
        6 -> if (address.size == 16) InetAddress.getByAddress(address).hostAddress else null
        else -> null
    }

    private fun parseIpv4(value: String): ByteArray? {
        val parts = value.split('.')
        if (parts.size != 4) return null
        val octets = ByteArray(4)
        for ((index, part) in parts.withIndex()) {
            if (part.isEmpty() || part.length > 3 || part.any { it !in '0'..'9' }) return null
            val number = part.toIntOrNull() ?: return null
            if (number > 255) return null
            // Reject leading zeros: "010" is ambiguous between decimal and octal.
            if (part.length > 1 && part[0] == '0') return null
            octets[index] = number.toByte()
        }
        return octets
    }

    private fun parsePrivateIpv6(value: String): ByteArray? {
        if (!value.matches(Regex("[0-9a-fA-F:.]+"))) return null
        val parsed = try {
            InetAddress.getByName(value)
        } catch (_: Exception) {
            return null
        }
        if (parsed !is Inet6Address) return null
        val octets = parsed.address
        return if (isPrivateIpv6(octets)) octets else null
    }
}
