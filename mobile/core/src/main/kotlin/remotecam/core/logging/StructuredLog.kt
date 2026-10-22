package remotecam.core.logging

import java.time.Instant
import java.time.temporal.ChronoUnit
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.buildJsonObject
import remotecam.protocol.ReasonCode

/** Log severity. The wire spelling is lowercase. */
enum class Severity(val wireName: String) {
    DEBUG("debug"),
    INFO("info"),
    WARNING("warning"),
    ERROR("error"),
}

/** Operation outcome, distinct from the severity. */
enum class Outcome(val wireName: String) {
    STARTED("started"),
    SUCCESS("success"),
    REJECTED("rejected"),
    FAILED("failed"),
    STOPPED("stopped"),
}

/** One structured log record. Additional fields are deliberately impossible. */
data class LogRecord(
    val timestamp: String,
    val severity: Severity,
    val operation: String,
    val correlationId: String,
    val outcome: Outcome,
    val reasonCode: ReasonCode? = null,
    val detail: String? = null,
)

/**
 * Structured logging shared with the desktop application.
 *
 * The record schema is `docs/spike/logging-schema.md`. Every detail is redacted
 * before it is stored, and an invalid operation, correlation id or an unsafe
 * detail is refused rather than written.
 */
object StructuredLog {
    private val operationPattern = Regex("^[a-z][a-z0-9_.]{0,63}$")
    private val correlationPattern = Regex("^[0-9a-f]{32}$")
    private val pathPrefixes = listOf("/home/", "/Users/", "C:\\Users\\", "c:\\Users\\")

    fun now(
        severity: Severity,
        operation: String,
        correlationId: String,
        outcome: Outcome,
    ): LogRecord = at(Instant.now().truncatedTo(ChronoUnit.SECONDS).toString(), severity, operation, correlationId, outcome)

    fun at(
        timestamp: String,
        severity: Severity,
        operation: String,
        correlationId: String,
        outcome: Outcome,
    ): LogRecord {
        require(operationPattern.matches(operation)) { "invalid operation" }
        require(correlationPattern.matches(correlationId)) { "invalid correlation id" }
        return LogRecord(timestamp, severity, operation, correlationId, outcome)
    }

    /** Attaches a detail after redaction. */
    fun withDetail(record: LogRecord, detail: String): LogRecord {
        val redacted = redact(detail)
        require(!containsUnsafeMaterial(redacted)) { "unsafe detail" }
        return record.copy(detail = redacted.take(200))
    }

    /** Serializes the record as one JSON object. */
    fun toJson(record: LogRecord): String = buildJsonObject {
        put("timestamp", JsonPrimitive(record.timestamp))
        put("severity", JsonPrimitive(record.severity.wireName))
        put("operation", JsonPrimitive(record.operation))
        put("correlation_id", JsonPrimitive(record.correlationId))
        put("outcome", JsonPrimitive(record.outcome.wireName))
        record.reasonCode?.let { put("reason_code", JsonPrimitive(it.wireName)) }
        record.detail?.let { put("detail", JsonPrimitive(it)) }
    }.toString()

    /** Replaces token- and fingerprint-shaped runs and absolute user paths. */
    fun redact(input: String): String = redactPaths(redactHexRuns(input))
        .map { if (it.isISOControl()) ' ' else it }
        .joinToString("")

    private fun redactHexRuns(input: String): String {
        val output = StringBuilder(input.length)
        var runStart = -1
        for ((index, character) in input.withIndex()) {
            if (character.isHexDigit()) {
                if (runStart < 0) runStart = index
            } else if (runStart >= 0) {
                pushRedactedRun(output, input.substring(runStart, index))
                output.append(character)
                runStart = -1
            } else {
                output.append(character)
            }
        }
        if (runStart >= 0) pushRedactedRun(output, input.substring(runStart))
        return output.toString()
    }

    private fun pushRedactedRun(output: StringBuilder, run: String) {
        if (run.length >= 32) output.append("[redacted]") else output.append(run)
    }

    private fun redactPaths(input: String): String {
        val output = StringBuilder(input.length)
        var index = 0
        while (index < input.length) {
            val prefix = pathPrefixes.firstOrNull { input.startsWith(it, index) }
            if (prefix != null) {
                output.append("<path>")
                val whitespace = input.indexOfFirstWhitespace(index)
                index = if (whitespace < 0) input.length else whitespace
            } else {
                output.append(input[index])
                index += 1
            }
        }
        return output.toString()
    }

    private fun String.indexOfFirstWhitespace(start: Int): Int {
        for (index in start until length) {
            if (this[index].isWhitespace()) return index
        }
        return -1
    }

    private fun containsUnsafeMaterial(input: String): Boolean {
        var hexRun = 0
        for (character in input) {
            if (character.isHexDigit()) {
                hexRun += 1
                if (hexRun >= 32) return true
            } else {
                hexRun = 0
            }
        }
        return pathPrefixes.any { input.contains(it) }
    }

    private fun Char.isHexDigit(): Boolean =
        this in '0'..'9' || this in 'a'..'f'
}
