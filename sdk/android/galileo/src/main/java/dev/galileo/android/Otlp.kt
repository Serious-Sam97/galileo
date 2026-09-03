package dev.galileo.android

import org.json.JSONArray
import org.json.JSONObject
import java.security.SecureRandom

/** Minimal OTLP/JSON model: spans, logs and gauge metrics, encoded without any OTel dependency. */
internal object Ids {
    private val rnd = SecureRandom()
    fun hex(bytes: Int): String { val b = ByteArray(bytes); rnd.nextBytes(b); return b.joinToString("") { "%02x".format(it) } }
}

fun nanos(ms: Long): String = (ms * 1_000_000L).toString()

internal fun attr(k: String, v: Any?): JSONObject? {
    if (v == null) return null
    val value = JSONObject()
    when (v) {
        is Boolean -> value.put("boolValue", v)
        is Int, is Long -> value.put("intValue", v.toString())
        is Float, is Double -> value.put("doubleValue", (v as Number).toDouble())
        else -> value.put("stringValue", v.toString().take(4000))
    }
    return JSONObject().put("key", k).put("value", value)
}

internal fun attrs(m: Map<String, Any?>): JSONArray { val a = JSONArray(); m.forEach { (k, v) -> attr(k, v)?.let { a.put(it) } }; return a }

class SpanData(
    val traceId: String, val spanId: String, val parentSpanId: String?, val name: String, val kind: Int,
    val startMs: Long, var endMs: Long, val attributes: MutableMap<String, Any?> = mutableMapOf(), var error: String? = null,
    val events: MutableList<Pair<String, Map<String, Any?>>> = mutableListOf(),
) {
    fun traceparent() = "00-$traceId-$spanId-01"
    fun toJson(): JSONObject {
        val o = JSONObject().put("traceId", traceId).put("spanId", spanId).put("name", name).put("kind", kind)
            .put("startTimeUnixNano", nanos(startMs)).put("endTimeUnixNano", nanos(endMs)).put("attributes", attrs(attributes))
            .put("status", JSONObject().put("code", if (error != null) 2 else 0).put("message", error ?: ""))
        parentSpanId?.let { o.put("parentSpanId", it) }
        if (events.isNotEmpty()) o.put("events", JSONArray().apply { events.forEach { (n, a) -> put(JSONObject().put("name", n).put("timeUnixNano", nanos(endMs)).put("attributes", attrs(a))) } })
        return o
    }
}

class LogData(val timeMs: Long, val severity: Int, val body: String, val attributes: Map<String, Any?>, val traceId: String? = null, val spanId: String? = null) {
    fun toJson(): JSONObject {
        val o = JSONObject().put("timeUnixNano", nanos(timeMs)).put("severityNumber", severity)
            .put("severityText", when { severity >= 17 -> "ERROR"; severity >= 13 -> "WARN"; else -> "INFO" })
            .put("body", JSONObject().put("stringValue", body.take(4000))).put("attributes", attrs(attributes))
        traceId?.let { o.put("traceId", it) }; spanId?.let { o.put("spanId", it) }
        return o
    }
}

class GaugeData(val name: String, val value: Double, val timeMs: Long, val attributes: Map<String, Any?>, val unit: String = "ms")

internal object OtlpEncoder {
    private fun scope() = JSONObject().put("name", "galileo-android").put("version", Galileo.VERSION)
    fun traces(resource: Map<String, Any?>, spans: List<SpanData>): String =
        JSONObject().put("resourceSpans", JSONArray().put(JSONObject().put("resource", JSONObject().put("attributes", attrs(resource)))
            .put("scopeSpans", JSONArray().put(JSONObject().put("scope", scope()).put("spans", JSONArray().apply { spans.forEach { put(it.toJson()) } }))))).toString()
    fun logs(resource: Map<String, Any?>, logs: List<LogData>): String =
        JSONObject().put("resourceLogs", JSONArray().put(JSONObject().put("resource", JSONObject().put("attributes", attrs(resource)))
            .put("scopeLogs", JSONArray().put(JSONObject().put("scope", scope()).put("logRecords", JSONArray().apply { logs.forEach { put(it.toJson()) } }))))).toString()
    fun metrics(resource: Map<String, Any?>, gauges: List<GaugeData>): String {
        val byName = gauges.groupBy { it.name }
        val metrics = JSONArray()
        byName.forEach { (name, list) ->
            metrics.put(JSONObject().put("name", name).put("unit", list.first().unit).put("gauge", JSONObject().put("dataPoints", JSONArray().apply {
                list.forEach { put(JSONObject().put("timeUnixNano", nanos(it.timeMs)).put("asDouble", it.value).put("attributes", attrs(it.attributes))) }
            })))
        }
        return JSONObject().put("resourceMetrics", JSONArray().put(JSONObject().put("resource", JSONObject().put("attributes", attrs(resource)))
            .put("scopeMetrics", JSONArray().put(JSONObject().put("scope", scope()).put("metrics", metrics))))).toString()
    }
}
