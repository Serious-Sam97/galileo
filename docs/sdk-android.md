# Android SDK (`sdk/android/galileo`)

Kotlin library (minSdk 24) that reports screens, network calls, database queries, crashes and app
vitals to Galileo as OTLP/JSON, so a phone session shows up next to browser sessions.

```kotlin
// Application.onCreate()
Galileo.init(this, endpoint = "https://galileo.example.com:4318", apiKey = "glk_…", service = "momentum-android", env = "prod")
Galileo.setUser(id = user.id, email = user.email)

val client = OkHttpClient.Builder().addInterceptor(Galileo.okHttpInterceptor(propagateTo = listOf("https://api.example.com"))).build()
val rows = Galileo.query("SELECT * FROM habits", "sqlite") { dao.all() }
Galileo.event("habit.completed", mapOf("habit.id" to id))
```

What is recorded:

| Signal | Span / metric |
|---|---|
| Screen shown (Activity/Fragment) | `screen <Name>` page view with `rum.type = screen`, `session.id`, `screen.name` |
| OkHttp / Retrofit call | CLIENT span `GET /path` with `http.*`, `traceparent` sent to `propagateTo` origins |
| Room / SQLite (via `Galileo.query`) | CLIENT span with `db.system`, `db.statement`, `db.sql.table` and the calling Kotlin function |
| Crash (uncaught exception) | error span + log with `exception.type/message/stacktrace`, flushed synchronously before the process dies |
| ANR (main thread blocked > 5 s) | error span `ANR` with the main-thread stack |
| Cold start, frame drops, memory | gauges `mobile.vital.cold_start_ms`, `mobile.vital.slow_frames`, `mobile.vital.frozen_frames`, `mobile.vital.memory_mb` per screen |

Batches are flushed every 5 s, on background, and kept in an on-disk queue when offline (sent on the
next start). Identity (`user.id`, `user.email`) and `session.id` (per process, rotated after 30 min
in background) are on every item. The Browser page lists mobile sessions with `platform = android`.

Build: the module is a standard Gradle library (`sdk/android/galileo`); add it with
`implementation(project(":galileo"))` or publish it to your Maven repo. Unit tests cover the OTLP
encoder, the session/identity attributes and the interceptor.
