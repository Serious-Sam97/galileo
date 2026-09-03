package dev.galileo.android

import java.io.File
import java.net.HttpURLConnection
import java.net.URL
import java.util.concurrent.Executors
import java.util.concurrent.TimeUnit

/** Batches items, posts OTLP/JSON with a bearer key, keeps an on-disk queue when offline. */
internal class Transport(private val endpoint: String, private val apiKey: String, private val queueDir: File?) {
    private val spans = ArrayList<SpanData>()
    private val logs = ArrayList<LogData>()
    private val gauges = ArrayList<GaugeData>()
    private val exec = Executors.newSingleThreadScheduledExecutor { r -> Thread(r, "galileo-transport").apply { isDaemon = true } }
    @Volatile var resource: Map<String, Any?> = emptyMap()

    init {
        exec.scheduleWithFixedDelay({ flush() }, 5, 5, TimeUnit.SECONDS)
        exec.execute { drainDisk() }
    }

    @Synchronized fun span(s: SpanData) { spans.add(s); if (spans.size >= 50) exec.execute { flush() } }
    @Synchronized fun log(l: LogData) { logs.add(l) }
    @Synchronized fun gauge(g: GaugeData) { gauges.add(g) }

    fun flush() {
        val (s, l, g) = synchronized(this) { Triple(ArrayList(spans).also { spans.clear() }, ArrayList(logs).also { logs.clear() }, ArrayList(gauges).also { gauges.clear() }) }
        if (s.isNotEmpty()) send("/v1/traces", OtlpEncoder.traces(resource, s))
        if (l.isNotEmpty()) send("/v1/logs", OtlpEncoder.logs(resource, l))
        if (g.isNotEmpty()) send("/v1/metrics", OtlpEncoder.metrics(resource, g))
    }

    /** Synchronous flush for crashes: block up to 2 s so the error leaves the device. */
    fun flushNow() { try { exec.submit { flush() }.get(2, TimeUnit.SECONDS) } catch (_: Exception) {} }

    private fun send(path: String, body: String, fromDisk: Boolean = false): Boolean {
        return try {
            val c = URL(endpoint + path).openConnection() as HttpURLConnection
            c.requestMethod = "POST"; c.connectTimeout = 5000; c.readTimeout = 10000; c.doOutput = true
            c.setRequestProperty("content-type", "application/json")
            if (apiKey.isNotEmpty()) c.setRequestProperty("authorization", "Bearer $apiKey")
            c.outputStream.use { it.write(body.toByteArray()) }
            val ok = c.responseCode in 200..299
            c.disconnect()
            if (!ok && c.responseCode >= 500 && !fromDisk) park(path, body)
            ok
        } catch (_: Exception) { if (!fromDisk) park(path, body); false }
    }

    private fun park(path: String, body: String) {
        val dir = queueDir ?: return
        try { dir.mkdirs(); File(dir, "${System.currentTimeMillis()}-${path.trimStart('/').replace('/', '_')}.json").writeText(body) } catch (_: Exception) {}
        // cap the queue at ~50 files
        dir.listFiles()?.sortedBy { it.name }?.dropLast(50)?.forEach { it.delete() }
    }

    private fun drainDisk() {
        val dir = queueDir ?: return
        dir.listFiles()?.sortedBy { it.name }?.forEach { f ->
            val path = "/" + f.name.substringAfter('-').removeSuffix(".json").replace('_', '/')
            if (send(path, f.readText(), fromDisk = true)) f.delete()
        }
    }
}
