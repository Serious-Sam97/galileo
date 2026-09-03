package dev.galileo.android

import okhttp3.Interceptor
import okhttp3.Response

internal class OkHttpInterceptor(private val propagateTo: List<String>) : Interceptor {
    override fun intercept(chain: Interceptor.Chain): Response {
        val req = chain.request()
        if (Galileo.transport == null) return chain.proceed(req)
        val url = req.url.toString()
        val span = Galileo.startSpan("${req.method} ${req.url.encodedPath}", 3, mapOf("rum.type" to "fetch", "http.request.method" to req.method, "url.full" to url.take(1000), "http.url.path" to req.url.encodedPath, "server.address" to req.url.host) + Galileo.callSite())
        val b = req.newBuilder()
        if (propagateTo.any { url.startsWith(it) } || Galileo.shouldPropagate(url)) b.header("traceparent", span.traceparent())
        Galileo.headers().filterKeys { it != "traceparent" }.forEach { (k, v) -> b.header(k, v) }
        return try {
            val resp = chain.proceed(b.build())
            span.attributes["http.response.status_code"] = resp.code
            resp.body?.contentLength()?.takeIf { it >= 0 }?.let { span.attributes["http.response.body.size"] = it }
            Galileo.end(span, if (resp.code >= 400) "HTTP ${resp.code}" else null)
            resp
        } catch (e: Exception) {
            span.attributes["error.type"] = e.javaClass.simpleName
            Galileo.end(span, e.message ?: e.javaClass.simpleName); throw e
        }
    }
}
