package remotecam.session

import java.util.concurrent.atomic.AtomicLong

/**
 * A clock that only moves when the caller advances it. Test and conformance
 * support, mirroring the Rust `testing::ManualClock`.
 */
class ManualClock(start: Timestamp = Timestamp.ZERO) : Clock {
    private val nowMillis = AtomicLong(start.millis)

    fun advance(millis: Long) {
        nowMillis.addAndGet(millis)
    }

    fun set(now: Timestamp) {
        nowMillis.set(now.millis)
    }

    override fun now(): Timestamp = Timestamp(nowMillis.get())
}

/**
 * A non-cryptographic byte source (SplitMix64) seeded with one value. It
 * produces the same stream as the Rust `testing::SeededRandomSource`, so both
 * cores assign the same identities to the same seed. It must never generate
 * production secrets.
 */
class SeededRandomSource(seed: Long) : RandomSource {
    private var state: ULong = seed.toULong()

    private fun nextULong(): ULong {
        state += 0x9e37_79b9_7f4a_7c15uL
        var mixed = state
        mixed = (mixed xor (mixed shr 30)) * 0xbf58_476d_1ce4_e5b9uL
        mixed = (mixed xor (mixed shr 27)) * 0x94d0_49bb_1331_11ebuL
        return mixed xor (mixed shr 31)
    }

    override fun fill(destination: ByteArray) {
        var offset = 0
        while (offset < destination.size) {
            val value = nextULong()
            val count = minOf(8, destination.size - offset)
            for (index in 0 until count) {
                destination[offset + index] = (value shr (8 * index)).toByte()
            }
            offset += count
        }
    }
}
