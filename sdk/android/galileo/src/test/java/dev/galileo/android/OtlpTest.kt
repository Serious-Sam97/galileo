package dev.galileo.android

import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class OtlpTest {
    @Test fun spanEncodesOtlpJson() {
        val s = SpanData("a".repeat(32), "b".repeat(16), null, "GET /x", 3, 1000L, 1250L, mutableMapOf("http.response.status_code" to 200, "session.id" to "s1", "ok" to true))
        val j = JSONObject(OtlpEncoder.traces(mapOf("service.name" to "app"), listOf(s)))
        val span = j.getJSONArray("resourceSpans").getJSONObject(0).getJSONArray("scopeSpans").getJSONObject(0).getJSONArray("spans").getJSONObject(0)
        assertEquals("GET /x", span.getString("name"))
        assertEquals("1000000000", span.getString("startTimeUnixNano"))
        assertEquals("1250000000", span.getString("endTimeUnixNano"))
        val attrs = span.getJSONArray("attributes")
        val status = (0 until attrs.length()).map { attrs.getJSONObject(it) }.first { it.getString("key") == "http.response.status_code" }
        assertEquals("200", status.getJSONObject("value").getString("intValue"))
        assertEquals(0, span.getJSONObject("status").getInt("code"))
        assertEquals("00-${"a".repeat(32)}-${"b".repeat(16)}-01", s.traceparent())
    }

    @Test fun errorSpanHasStatusAndEvent() {
        val s = SpanData("a".repeat(32), "b".repeat(16), "c".repeat(16), "exception X", 1, 1L, 2L, mutableMapOf(), "boom", mutableListOf("exception" to mapOf("exception.type" to "X")))
        val span = JSONObject(s.toJson().toString())
        assertEquals(2, span.getJSONObject("status").getInt("code"))
        assertEquals("c".repeat(16), span.getString("parentSpanId"))
        assertEquals("exception", span.getJSONArray("events").getJSONObject(0).getString("name"))
    }

    @Test fun gaugesGroupByName() {
        val j = JSONObject(OtlpEncoder.metrics(mapOf("service.name" to "app"), listOf(GaugeData("mobile.vital.cold_start_ms", 420.0, 1L, mapOf("screen.name" to "Main")), GaugeData("mobile.vital.cold_start_ms", 380.0, 2L, emptyMap()), GaugeData("mobile.vital.slow_frames", 3.0, 2L, emptyMap(), "1"))))
        val metrics = j.getJSONArray("resourceMetrics").getJSONObject(0).getJSONArray("scopeMetrics").getJSONObject(0).getJSONArray("metrics")
        assertEquals(2, metrics.length())
        val cold = (0 until metrics.length()).map { metrics.getJSONObject(it) }.first { it.getString("name") == "mobile.vital.cold_start_ms" }
        assertEquals(2, cold.getJSONObject("gauge").getJSONArray("dataPoints").length())
    }

    @Test fun logEncodesSeverityAndTrace() {
        val l = LogData(5L, 17, "ANR: blocked", mapOf("exception.type" to "ANR"), "t".repeat(32), "s".repeat(16))
        val j = l.toJson()
        assertEquals("ERROR", j.getString("severityText"))
        assertEquals("t".repeat(32), j.getString("traceId"))
        assertTrue(j.getJSONObject("body").getString("stringValue").startsWith("ANR"))
    }
}
