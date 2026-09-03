package dev.galileo.android

import android.os.Handler
import android.os.Looper

internal object CrashHandler {
    fun install() {
        val prev = Thread.getDefaultUncaughtExceptionHandler()
        Thread.setDefaultUncaughtExceptionHandler { t, e ->
            try { Galileo.captureException(e, mapOf("thread.name" to t.name), fatal = true) } catch (_: Throwable) {}
            prev?.uncaughtException(t, e)
        }
    }
}

/** Reports an ANR when the main thread does not respond for 5 s (once per stall). */
internal object AnrWatchdog {
    fun start() {
        val h = Handler(Looper.getMainLooper())
        Thread({
            var tick = 0L
            while (true) {
                val before = tick
                h.post { tick++ }
                try { Thread.sleep(5000) } catch (_: InterruptedException) { return@Thread }
                if (tick == before) {
                    val stack = Looper.getMainLooper().thread.stackTrace.joinToString("\n") { "    at $it" }
                    val s = Galileo.startSpan("ANR", 1, mapOf("rum.type" to "error", "exception.type" to "ANR", "exception.message" to "main thread blocked > 5s", "exception.stacktrace" to stack.take(8000)))
                    Galileo.end(s, "ANR")
                    Galileo.log("error", "ANR: main thread blocked > 5s", mapOf("exception.type" to "ANR", "exception.stacktrace" to stack.take(8000)))
                    try { Thread.sleep(20000) } catch (_: InterruptedException) { return@Thread }
                }
            }
        }, "galileo-anr").apply { isDaemon = true }.start()
    }
}
