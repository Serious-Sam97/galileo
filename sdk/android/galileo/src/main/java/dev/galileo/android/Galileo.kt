package dev.galileo.android

import android.app.Activity
import android.app.Application
import android.content.Context
import android.os.Build
import android.os.Bundle
import android.os.Handler
import android.os.Looper
import android.view.Choreographer
import java.io.File

/**
 * Galileo for Android: screens as page views, OkHttp calls with traceparent, DB queries with the
 * calling function, crashes/ANRs as issues, app vitals — one session per process, identity on
 * every item.
 */
object Galileo {
    const val VERSION = "0.1.0"
    @Volatile internal var transport: Transport? = null
    internal var session: String = Ids.hex(16)
    internal var pageView = 0
    @Volatile internal var currentScreen: SpanData? = null
    private val user = mutableMapOf<String, Any?>()
    private var propagate: List<String> = emptyList()
    private var service = "android-app"
    private var startMs = 0L
    private var backgroundedAt = 0L
    private var slowFrames = 0; private var frozenFrames = 0

    @JvmStatic @JvmOverloads
    fun init(app: Context, endpoint: String, apiKey: String, service: String, env: String? = null, version: String? = null, propagateTo: List<String> = emptyList()) {
        if (transport != null) return
        this.service = service; this.propagate = propagateTo; startMs = System.currentTimeMillis()
        val t = Transport(endpoint.trimEnd('/'), apiKey, File(app.cacheDir, "galileo-queue"))
        t.resource = mapOf(
            "service.name" to service, "service.version" to (version ?: appVersion(app)), "deployment.environment" to env,
            "telemetry.sdk.name" to "galileo-android", "telemetry.sdk.version" to VERSION, "galileo.rum" to true,
            "platform" to "android", "browser.name" to "android", "os.name" to "android", "os.version" to Build.VERSION.RELEASE,
            "device.model" to Build.MODEL, "device.manufacturer" to Build.MANUFACTURER, "browser.mobile" to true,
        )
        transport = t
        (app.applicationContext as? Application)?.registerActivityLifecycleCallbacks(lifecycle)
        CrashHandler.install()
        AnrWatchdog.start()
        startFrameWatch()
    }

    @JvmStatic fun setUser(id: Any?, email: String? = null, name: String? = null, tenant: Any? = null) {
        user.clear(); if (id != null) user["user.id"] = id.toString(); if (email != null) user["user.email"] = email; if (name != null) user["user.name"] = name; if (tenant != null) user["tenant.id"] = tenant.toString()
    }

    internal fun common(extra: Map<String, Any?> = emptyMap()): MutableMap<String, Any?> {
        val m = mutableMapOf<String, Any?>("session.id" to session, "page.view" to pageView, "url.path" to (currentScreen?.attributes?.get("screen.name") ?: ""), "platform" to "android", "browser.name" to "android")
        m.putAll(user); m.putAll(extra); return m
    }

    /** Start a span; end it with [end]. Children of the current screen share its trace. */
    @JvmStatic fun startSpan(name: String, kind: Int = 1, attributes: Map<String, Any?> = emptyMap()): SpanData {
        val parent = currentScreen
        return SpanData(parent?.traceId ?: Ids.hex(16), Ids.hex(8), parent?.spanId, name, kind, System.currentTimeMillis(), 0L, common(attributes))
    }
    @JvmStatic fun end(span: SpanData, error: String? = null) { span.endMs = System.currentTimeMillis(); if (error != null) span.error = error; transport?.span(span) }

    /** DB query with the calling Kotlin function attached. */
    fun <T> query(statement: String, system: String = "sqlite", block: () -> T): T {
        val op = statement.trim().split(Regex("\\s+")).firstOrNull()?.uppercase() ?: "QUERY"
        val table = Regex("(?i)\\b(?:from|into|update|join)\\s+[`\"\\[]?([A-Za-z0-9_.]+)").find(statement)?.groupValues?.get(1) ?: ""
        val s = startSpan(if (table.isEmpty()) op else "$op $table", 3, mapOf("db.system" to system, "db.statement" to statement.take(4000), "db.operation" to op, "db.sql.table" to table) + callSite())
        return try { block().also { end(s) } } catch (e: Throwable) { s.attributes["exception.type"] = e.javaClass.simpleName; s.attributes["exception.message"] = e.message; end(s, e.message ?: e.javaClass.simpleName); throw e }
    }

    /** Custom event span. */
    @JvmStatic fun event(name: String, attributes: Map<String, Any?> = emptyMap()) { val s = startSpan(name, 1, attributes + ("rum.type" to "event")); end(s) }

    @JvmStatic fun log(level: String, message: String, attributes: Map<String, Any?> = emptyMap()) {
        val sev = when (level.lowercase()) { "error" -> 17; "warn", "warning" -> 13; "debug" -> 5; else -> 9 }
        transport?.log(LogData(System.currentTimeMillis(), sev, message, common(attributes), currentScreen?.traceId, currentScreen?.spanId))
    }

    @JvmStatic fun captureException(e: Throwable, extra: Map<String, Any?> = emptyMap(), fatal: Boolean = false) {
        val s = startSpan("exception ${e.javaClass.simpleName}", 1, mapOf("rum.type" to "error", "exception.type" to e.javaClass.simpleName, "exception.message" to e.message, "exception.stacktrace" to e.stackTraceToString().take(8000), "exception.escaped" to fatal) + extra)
        s.events.add("exception" to mapOf("exception.type" to e.javaClass.simpleName, "exception.message" to e.message, "exception.stacktrace" to e.stackTraceToString().take(8000)))
        end(s, e.message ?: e.javaClass.simpleName)
        log("error", "${e.javaClass.simpleName}: ${e.message}", mapOf("exception.type" to e.javaClass.simpleName, "exception.message" to e.message, "exception.stacktrace" to e.stackTraceToString().take(8000)))
        if (fatal) transport?.flushNow()
    }

    /** Headers for a manual HTTP client: traceparent (if a screen is active) + identity. */
    @JvmStatic fun headers(url: String? = null): Map<String, String> {
        val h = mutableMapOf<String, String>()
        val sc = currentScreen
        if (sc != null && (url == null || shouldPropagate(url))) h["traceparent"] = "00-${sc.traceId}-${Ids.hex(8)}-01"
        user["user.id"]?.let { h["x-galileo-user-id"] = it.toString() }; user["tenant.id"]?.let { h["x-galileo-tenant-id"] = it.toString() }
        h["x-galileo-session-id"] = session
        return h
    }

    internal fun shouldPropagate(url: String): Boolean = propagate.any { url.startsWith(it) }

    /** OkHttp interceptor: client spans with traceparent to [propagateTo] origins. Requires okhttp on the classpath. */
    @JvmStatic fun okHttpInterceptor(propagateTo: List<String> = propagate): okhttp3.Interceptor = OkHttpInterceptor(propagateTo)

    @JvmStatic fun flush() { transport?.flush() }

    // ---- screens ------------------------------------------------------------------------------
    internal fun screen(name: String) {
        currentScreen?.let { end(it) }
        pageView++
        val s = SpanData(Ids.hex(16), Ids.hex(8), null, "screen $name", 2, System.currentTimeMillis(), 0L, mutableMapOf())
        s.attributes.putAll(common(mapOf("rum.type" to "screen", "screen.name" to name, "url.path" to name)))
        currentScreen = s
        if (pageView == 1 && startMs > 0) transport?.gauge(GaugeData("mobile.vital.cold_start_ms", (System.currentTimeMillis() - startMs).toDouble(), System.currentTimeMillis(), common(mapOf("screen.name" to name))))
    }

    private val lifecycle = object : Application.ActivityLifecycleCallbacks {
        private var started = 0
        override fun onActivityResumed(a: Activity) { screen(a.javaClass.simpleName) }
        override fun onActivityStarted(a: Activity) { if (started++ == 0 && backgroundedAt > 0 && System.currentTimeMillis() - backgroundedAt > 30 * 60_000) { session = Ids.hex(16); pageView = 0 } }
        override fun onActivityStopped(a: Activity) { if (--started == 0) { backgroundedAt = System.currentTimeMillis(); emitFrames(); currentScreen?.let { end(it) }; currentScreen = null; transport?.flush() } }
        override fun onActivityCreated(a: Activity, b: Bundle?) {}
        override fun onActivityPaused(a: Activity) {}
        override fun onActivitySaveInstanceState(a: Activity, b: Bundle) {}
        override fun onActivityDestroyed(a: Activity) {}
    }

    // ---- vitals -------------------------------------------------------------------------------
    private fun startFrameWatch() {
        try {
            Handler(Looper.getMainLooper()).post {
                var last = 0L
                Choreographer.getInstance().postFrameCallback(object : Choreographer.FrameCallback {
                    override fun doFrame(t: Long) {
                        if (last > 0) { val ms = (t - last) / 1_000_000.0; if (ms > 700) frozenFrames++ else if (ms > 32) slowFrames++ }
                        last = t; Choreographer.getInstance().postFrameCallback(this)
                    }
                })
            }
        } catch (_: Throwable) {}
    }
    private fun emitFrames() {
        val now = System.currentTimeMillis(); val c = common(mapOf("screen.name" to (currentScreen?.attributes?.get("screen.name") ?: "")))
        transport?.gauge(GaugeData("mobile.vital.slow_frames", slowFrames.toDouble(), now, c, "1")); transport?.gauge(GaugeData("mobile.vital.frozen_frames", frozenFrames.toDouble(), now, c, "1"))
        val rt = Runtime.getRuntime(); transport?.gauge(GaugeData("mobile.vital.memory_mb", (rt.totalMemory() - rt.freeMemory()) / 1e6, now, c, "MB"))
        slowFrames = 0; frozenFrames = 0
    }

    internal fun callSite(): Map<String, Any?> {
        val st = Thread.currentThread().stackTrace
        val sdk = listOf("dev.galileo.android.Galileo", "dev.galileo.android.OkHttpInterceptor", "dev.galileo.android.Transport", "dev.galileo.android.CrashHandler", "dev.galileo.android.AnrWatchdog", "dev.galileo.android.Otlp", "dev.galileo.android.Ids")
        val f = st.firstOrNull { fr -> sdk.none { fr.className.startsWith(it) } && !fr.className.startsWith("java.") && !fr.className.startsWith("kotlin.") && !fr.className.startsWith("android.") && !fr.className.startsWith("dalvik.") && !fr.className.startsWith("androidx.") && !fr.className.startsWith("okhttp3.") && !fr.className.startsWith("jdk.") && !fr.className.startsWith("sun.") && !fr.className.startsWith("org.junit") } ?: return emptyMap()
        return mapOf("code.function.name" to f.methodName, "code.namespace" to f.className, "code.file.path" to (f.fileName ?: ""), "code.line.number" to f.lineNumber)
    }

    private fun appVersion(c: Context): String = try { c.packageManager.getPackageInfo(c.packageName, 0).versionName ?: "" } catch (_: Exception) { "" }
}
