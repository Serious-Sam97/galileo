/*! galileo-rum 0.1.0 — browser telemetry for Galileo (page loads, route changes, fetch/XHR, JS errors, Web Vitals).
 *  <script src="https://galileo.example.com/rum.js" data-key="glk_…" data-service="my-web" data-endpoint="https://galileo-api.example.com/otlp" data-propagate="https://api.example.com"></script>
 *  or window.galileoRum.init({ key, service, endpoint, propagate: [origins], user, version, env }).
 *  Sends OTLP/JSON. Fetches to same-origin and `propagate` origins carry `traceparent`, so the backend span joins the page's trace. */
(function (w, d) {
  if (!w || w.galileoRum && w.galileoRum._started) return;
  var perf = w.performance, nav = w.navigator, cfg = null, started = false;
  var spans = [], logs = [], metrics = [], flushTimer = null;
  var sid = null, user = {}, page = null, pageIdx = 0, vitals = {}, clsValue = 0, inpMax = 0, vitalsSent = false;
  var SDK = { name: 'galileo-rum', version: '0.1.0' };
  var bname = 'other', bmobile = false;

  // ---- helpers ------------------------------------------------------------------------------
  function hex(n) { var a = new Uint8Array(n); (w.crypto || w.msCrypto).getRandomValues(a); var s = ''; for (var i = 0; i < n; i++) s += (a[i] < 16 ? '0' : '') + a[i].toString(16); return s; }
  function nowMs() { return perf && perf.timeOrigin ? perf.timeOrigin + perf.now() : Date.now(); }
  function nanos(ms) { return String(Math.round(ms * 1000)) + '000'; }
  function kv(k, v) {
    if (v === undefined || v === null) return null;
    if (typeof v === 'boolean') return { key: k, value: { boolValue: v } };
    if (typeof v === 'number') return Number.isInteger(v) ? { key: k, value: { intValue: String(v) } } : { key: k, value: { doubleValue: v } };
    return { key: k, value: { stringValue: String(v).slice(0, 4000) } };
  }
  function attrs(o) { var out = []; for (var k in o) { var a = kv(k, o[k]); if (a) out.push(a); } return out; }
  function pathOf(u) { try { var x = new URL(u, d.baseURI); return x.pathname; } catch (e) { return String(u); } }
  function absolute(u) { try { return new URL(u, d.baseURI).href; } catch (e) { return String(u); } }
  function shouldPropagate(url) {
    try {
      var o = new URL(url, d.baseURI).origin;
      if (o === w.location.origin) return true;
      for (var i = 0; i < cfg.propagate.length; i++) if (o === cfg.propagate[i]) return true;
    } catch (e) {}
    return false;
  }
  function isOwn(url) { return cfg.endpoint && String(url).indexOf(cfg.endpoint) === 0; }
  function browserInfo() {
    var ua = nav.userAgent || '', name = 'other';
    if (/Edg\//.test(ua)) name = 'edge'; else if (/OPR\//.test(ua)) name = 'opera'; else if (/Chrome\//.test(ua)) name = 'chrome'; else if (/Safari\//.test(ua) && !/Chrome/.test(ua)) name = 'safari'; else if (/Firefox\//.test(ua)) name = 'firefox';
    return { 'browser.name': name, 'browser.user_agent': ua.slice(0, 300), 'browser.language': nav.language, 'browser.mobile': /Mobi|Android/i.test(ua), 'screen.width': w.screen && w.screen.width, 'screen.height': w.screen && w.screen.height, 'viewport.width': w.innerWidth, 'viewport.height': w.innerHeight };
  }
  function common(extra) {
    var o = { 'session.id': sid, 'url.path': w.location.pathname, 'url.full': w.location.href.slice(0, 1000), 'page.view': pageIdx, 'browser.name': bname, 'browser.mobile': bmobile };
    if (user.id) o['user.id'] = user.id; if (user.email) o['user.email'] = user.email; if (user.name) o['user.name'] = user.name;
    for (var k in extra) o[k] = extra[k];
    return o;
  }
  function resource() {
    var r = { 'service.name': cfg.service, 'telemetry.sdk.name': SDK.name, 'telemetry.sdk.version': SDK.version, 'telemetry.sdk.language': 'webjs', 'galileo.rum': true };
    if (cfg.version) r['service.version'] = cfg.version; if (cfg.env) r['deployment.environment'] = cfg.env;
    var b = browserInfo(); for (var k in b) r[k] = b[k];
    return attrs(r);
  }

  // ---- span model ----------------------------------------------------------------------------
  function span(name, kind, startMs, parent) {
    return { traceId: parent ? parent.traceId : hex(16), spanId: hex(8), parentSpanId: parent ? parent.spanId : undefined, name: name, kind: kind, start: startMs, attrs: {}, events: [], status: 0, message: '' };
  }
  function finish(s, endMs) {
    var o = { traceId: s.traceId, spanId: s.spanId, name: s.name, kind: s.kind, startTimeUnixNano: nanos(s.start), endTimeUnixNano: nanos(endMs == null ? nowMs() : endMs), attributes: attrs(common(s.attrs)), status: { code: s.status, message: s.message } };
    if (s.parentSpanId) o.parentSpanId = s.parentSpanId;
    if (s.events.length) o.events = s.events;
    spans.push(o); schedule();
  }
  function log(severity, text, body) {
    logs.push({ timeUnixNano: nanos(nowMs()), severityNumber: severity, severityText: severity >= 17 ? 'ERROR' : severity >= 13 ? 'WARN' : 'INFO', body: { stringValue: String(text).slice(0, 4000) }, attributes: attrs(common(body || {})), traceId: page ? page.traceId : undefined, spanId: page ? page.spanId : undefined });
    schedule();
  }
  function gauge(name, value, extra) { metrics.push({ name: name, value: value, attrs: attrs(common(extra || {})), t: nanos(nowMs()) }); schedule(); }

  // ---- export --------------------------------------------------------------------------------
  function schedule() { if (spans.length + logs.length + metrics.length >= 40) flush(); else if (!flushTimer) flushTimer = setTimeout(flush, 3000); }
  function post(path, body) {
    try {
      var json = JSON.stringify(body);
      var ok = w.fetch(cfg.endpoint + path, { method: 'POST', headers: { 'content-type': 'application/json', authorization: 'Bearer ' + cfg.key }, body: json, keepalive: json.length < 60000, mode: 'cors', credentials: 'omit' });
      if (ok && ok.catch) ok.catch(function () {});
    } catch (e) {}
  }
  function flush() {
    if (flushTimer) { clearTimeout(flushTimer); flushTimer = null; }
    if (!cfg) return;
    var res = resource(), scope = { name: SDK.name, version: SDK.version };
    if (spans.length) { var s = spans.splice(0, 40); post('/v1/traces', { resourceSpans: [{ resource: { attributes: res }, scopeSpans: [{ scope: scope, spans: s }] }] }); }
    if (logs.length) { var l = logs.splice(0, 40); post('/v1/logs', { resourceLogs: [{ resource: { attributes: res }, scopeLogs: [{ scope: scope, logRecords: l }] }] }); }
    if (metrics.length) {
      var m = metrics.splice(0, 40), byName = {};
      m.forEach(function (x) { (byName[x.name] = byName[x.name] || []).push({ timeUnixNano: x.t, asDouble: x.value, attributes: x.attrs }); });
      var list = []; for (var n in byName) list.push({ name: n, unit: n.indexOf('cls') >= 0 ? '1' : 'ms', gauge: { dataPoints: byName[n] } });
      post('/v1/metrics', { resourceMetrics: [{ resource: { attributes: res }, scopeMetrics: [{ scope: scope, metrics: list }] }] });
    }
    if (spans.length || logs.length || metrics.length) schedule();
  }

  // ---- page views ------------------------------------------------------------------------------
  function endPageView() {
    if (!page) return;
    if (!vitalsSent) sendVitals();
    page = null;
  }
  function startPageView(type, startMs) {
    endPageView();
    pageIdx++; vitals = {}; clsValue = 0; inpMax = 0; vitalsSent = false;
    page = span((type === 'pageload' ? 'pageload ' : 'navigation ') + w.location.pathname, 1, startMs);
    page.attrs['rum.type'] = type; page.attrs['url.path'] = w.location.pathname; page.path = w.location.pathname; page.attrs['page.referrer'] = d.referrer ? d.referrer.slice(0, 500) : undefined; page.attrs['page.title'] = d.title;
    return page;
  }
  function sendPageload() {
    if (!page || page.sent) return;
    page.sent = true;
    var e = perf && perf.getEntriesByType && perf.getEntriesByType('navigation')[0];
    var end = nowMs();
    if (e) {
      page.attrs['navigation.type'] = e.type; page.attrs['navigation.ttfb_ms'] = Math.round(e.responseStart); page.attrs['navigation.dom_interactive_ms'] = Math.round(e.domInteractive);
      page.attrs['navigation.dom_content_loaded_ms'] = Math.round(e.domContentLoadedEventEnd); page.attrs['navigation.load_ms'] = Math.round(e.loadEventEnd || e.domComplete); page.attrs['navigation.transfer_bytes'] = e.transferSize;
      page.attrs['navigation.protocol'] = e.nextHopProtocol; if (e.loadEventEnd) end = perf.timeOrigin + e.loadEventEnd;
      vitals.ttfb = Math.round(e.responseStart);
    }
    for (var k in vitals) page.attrs['web_vital.' + k] = vitals[k];
    finish(page, end);
  }
  function sendVitals() {
    vitalsSent = true;
    if (clsValue) vitals.cls = Math.round(clsValue * 1000) / 1000; if (inpMax) vitals.inp = Math.round(inpMax);
    var pv = { 'rum.type': page ? page.attrs['rum.type'] : 'pageload', 'url.path': page ? page.path : w.location.pathname };
    for (var k in vitals) gauge('browser.web_vital.' + k, vitals[k], pv);
    flush();
  }
  function observeVitals() {
    if (!w.PerformanceObserver) return;
    try { new PerformanceObserver(function (l) { var es = l.getEntries(); var last = es[es.length - 1]; if (last) vitals.lcp = Math.round(last.startTime); }).observe({ type: 'largest-contentful-paint', buffered: true }); } catch (e) {}
    try { new PerformanceObserver(function (l) { l.getEntries().forEach(function (e) { if (e.name === 'first-contentful-paint') vitals.fcp = Math.round(e.startTime); }); }).observe({ type: 'paint', buffered: true }); } catch (e) {}
    try { new PerformanceObserver(function (l) { l.getEntries().forEach(function (e) { if (!e.hadRecentInput) clsValue += e.value; }); }).observe({ type: 'layout-shift', buffered: true }); } catch (e) {}
    try { new PerformanceObserver(function (l) { l.getEntries().forEach(function (e) { if (e.duration > inpMax) inpMax = e.duration; }); }).observe({ type: 'event', buffered: true, durationThreshold: 40 }); } catch (e) {}
    try { new PerformanceObserver(function (l) { l.getEntries().forEach(function (e) { if (e.processingStart && vitals.fid === undefined) vitals.fid = Math.round(e.processingStart - e.startTime); }); }).observe({ type: 'first-input', buffered: true }); } catch (e) {}
  }

  // ---- fetch / XHR -------------------------------------------------------------------------------
  function httpSpan(method, url, start) {
    var s = span(method + ' ' + pathOf(url), 3, start, page);
    s.attrs['rum.type'] = 'fetch'; s.attrs['http.request.method'] = method; s.attrs['url.full'] = absolute(url).slice(0, 1000); s.attrs['http.url.path'] = pathOf(url); s.attrs['server.address'] = (function () { try { return new URL(url, d.baseURI).host; } catch (e) { return ''; } })();
    return s;
  }
  function endHttp(s, status, err, bytes) {
    if (status) s.attrs['http.response.status_code'] = status; if (bytes) s.attrs['http.response.body.size'] = bytes;
    if (err) { s.status = 2; s.message = String(err).slice(0, 300); s.attrs['error.type'] = (err && err.name) || 'FetchError'; }
    else if (status >= 400) { s.status = 2; s.message = 'HTTP ' + status; s.attrs['error.type'] = String(status); }
    finish(s);
  }
  function patchFetch() {
    if (!w.fetch) return;
    var orig = w.fetch;
    w.fetch = function (input, init) {
      var url = (input && input.url) || String(input);
      if (isOwn(url)) return orig.apply(this, arguments);
      var method = ((init && init.method) || (input && input.method) || 'GET').toUpperCase();
      var s = httpSpan(method, url, nowMs());
      if (shouldPropagate(url)) {
        init = init || {};
        var h = new Headers(init.headers || (input && input.headers) || undefined);
        h.set('traceparent', '00-' + s.traceId + '-' + s.spanId + '-01');
        init.headers = h;
      }
      return orig.call(this, input, init).then(function (r) {
        var len = r.headers && r.headers.get ? Number(r.headers.get('content-length')) || 0 : 0;
        endHttp(s, r.status, null, len); return r;
      }, function (e) { endHttp(s, 0, e); throw e; });
    };
  }
  function patchXhr() {
    var X = w.XMLHttpRequest; if (!X) return;
    var open = X.prototype.open, send = X.prototype.send, setH = X.prototype.setRequestHeader;
    X.prototype.open = function (m, u) { this.__g = { method: String(m).toUpperCase(), url: String(u) }; return open.apply(this, arguments); };
    X.prototype.send = function () {
      var g = this.__g, x = this;
      if (g && !isOwn(g.url)) {
        g.span = httpSpan(g.method, g.url, nowMs());
        if (shouldPropagate(g.url)) { try { setH.call(x, 'traceparent', '00-' + g.span.traceId + '-' + g.span.spanId + '-01'); } catch (e) {} }
        x.addEventListener('loadend', function () { endHttp(g.span, x.status, x.status === 0 ? 'network error' : null, 0); });
      }
      return send.apply(this, arguments);
    };
  }

  // ---- errors ------------------------------------------------------------------------------------
  function reportError(type, message, stack, extra) {
    var s = span('exception ' + type, 1, nowMs(), page);
    s.status = 2; s.message = String(message).slice(0, 300);
    s.attrs['rum.type'] = 'error'; s.attrs['exception.type'] = type; s.attrs['exception.message'] = String(message).slice(0, 2000); if (stack) s.attrs['exception.stacktrace'] = String(stack).slice(0, 8000);
    for (var k in extra) s.attrs[k] = extra[k];
    s.events.push({ name: 'exception', timeUnixNano: nanos(nowMs()), attributes: attrs({ 'exception.type': type, 'exception.message': String(message).slice(0, 2000), 'exception.stacktrace': stack ? String(stack).slice(0, 8000) : undefined }) });
    finish(s);
    var l = { 'exception.type': type, 'exception.message': String(message).slice(0, 2000), 'rum.type': 'error' }; if (stack) l['exception.stacktrace'] = String(stack).slice(0, 8000);
    log(17, type + ': ' + message, l);
  }
  function patchErrors() {
    w.addEventListener('error', function (e) {
      if (e && e.target && (e.target.src || e.target.href) && e.target !== w) { reportError('ResourceError', 'failed to load ' + (e.target.src || e.target.href), null, { 'resource.url': e.target.src || e.target.href }); return; }
      var err = e && e.error; reportError((err && err.name) || 'Error', (err && err.message) || (e && e.message) || 'unknown error', err && err.stack, { 'code.filepath': e && e.filename, 'code.lineno': e && e.lineno });
    });
    w.addEventListener('unhandledrejection', function (e) {
      var r = e && e.reason; reportError((r && r.name) || 'UnhandledRejection', (r && r.message) || String(r), r && r.stack);
    });
  }

  // ---- routing --------------------------------------------------------------------------------------
  function patchHistory() {
    var h = w.history; if (!h) return;
    function onNav() { var p = startPageView('navigation', nowMs()); (w.requestAnimationFrame || setTimeout)(function () { p.attrs['navigation.render_ms'] = Math.round(nowMs() - p.start); finish(p); }); }
    ['pushState', 'replaceState'].forEach(function (m) { var o = h[m]; if (!o) return; h[m] = function () { var before = w.location.href; var r = o.apply(this, arguments); if (w.location.href !== before) onNav(); return r; }; });
    w.addEventListener('popstate', onNav);
  }

  // ---- init -----------------------------------------------------------------------------------------
  function init(opts) {
    if (started) return; started = true;
    var el = d.currentScript || (function () { var s = d.getElementsByTagName('script'); return s[s.length - 1]; })();
    var ds = (el && el.dataset) || {};
    opts = opts || {};
    var scriptOrigin = (function () { try { return new URL(el.src, d.baseURI).origin; } catch (e) { return w.location.origin; } })();
    cfg = {
      key: opts.key || ds.key || '', service: opts.service || ds.service || 'browser',
      // default: the OTLP receiver next to the API that served this script — its own port when
      // served straight from :8080 (local), the proxy's /otlp path otherwise (hosted).
      endpoint: (opts.endpoint || ds.endpoint || (/:8080$/.test(scriptOrigin) ? scriptOrigin.replace(/:8080$/, ':4318') : scriptOrigin + '/otlp')).replace(/\/$/, ''),
      propagate: opts.propagate || (ds.propagate ? ds.propagate.split(/[ ,]+/) : []), version: opts.version || ds.version, env: opts.env || ds.env
    };
    cfg.propagate = cfg.propagate.map(function (o) { try { return new URL(o).origin; } catch (e) { return o; } });
    if (!cfg.key) { try { console.warn('[galileo-rum] no data-key; not started'); } catch (e) {} return; }
    try { sid = w.sessionStorage.getItem('galileo.sid'); if (!sid) { sid = hex(16); w.sessionStorage.setItem('galileo.sid', sid); } } catch (e) { sid = hex(16); }
    if (opts.user || ds.user) setUser(opts.user || ds.user, opts.userAttrs);
    var bi = browserInfo(); bname = bi['browser.name']; bmobile = !!bi['browser.mobile'];
    startPageView('pageload', perf && perf.timeOrigin ? perf.timeOrigin : Date.now());
    observeVitals(); patchFetch(); patchXhr(); patchErrors(); patchHistory();
    var pl = function () { setTimeout(sendPageload, 3000); };
    if (d.readyState === 'complete') pl(); else w.addEventListener('load', pl);
    var bye = function () { sendPageload(); if (!vitalsSent) sendVitals(); flush(); };
    w.addEventListener('pagehide', bye);
    d.addEventListener('visibilitychange', function () { if (d.visibilityState === 'hidden') bye(); });
  }
  function setUser(id, extra) { user = { id: id == null ? undefined : String(id) }; if (extra) { if (extra.email) user.email = extra.email; if (extra.name) user.name = extra.name; } }
  function event(name, extra) { var s = span(name, 1, nowMs(), page); s.attrs['rum.type'] = 'event'; for (var k in extra || {}) s.attrs[k] = extra[k]; finish(s); }

  w.galileoRum = { init: init, setUser: setUser, event: event, log: function (level, msg, a) { log(level === 'error' ? 17 : level === 'warn' ? 13 : 9, msg, a); }, flush: flush, sessionId: function () { return sid; }, _started: true };
  var tag = d.currentScript; if (tag && tag.dataset && tag.dataset.key) init();
})(window, document);
