package org.quillolang.app

import android.app.AlarmManager
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.location.Location
import android.location.LocationManager
import android.net.Uri
import android.os.BatteryManager
import android.os.Build
import android.os.CancellationSignal
import android.os.Handler
import android.os.Looper
import android.os.PowerManager
import android.os.SystemClock
import android.provider.Settings
import android.view.HapticFeedbackConstants
import android.view.WindowManager
import android.webkit.WebView
import androidx.webkit.JavaScriptReplyProxy
import androidx.webkit.WebMessageCompat
import androidx.webkit.WebViewCompat
import androidx.webkit.WebViewFeature
import org.json.JSONArray
import org.json.JSONObject

/**
 * `window.quilloNative` on the program's own pages: what a WebView cannot reach,
 * called from a component with `js("quilloNative.location")` and the rest, each a
 * Promise, as on iOS (mobile/ios/Quillo/App.swift). Only the program's origin gets
 * the object and its messages: WebViewCompat checks the frame's origin.
 */
class Bridge(private val activity: MainActivity, private val web: WebView) {
    private var attached = false

    /** Puts the bridge on the pages of [origin] (http://127.0.0.1:port). */
    fun attach(origin: String) {
        if (attached) return
        if (!WebViewFeature.isFeatureSupported(WebViewFeature.WEB_MESSAGE_LISTENER) ||
            !WebViewFeature.isFeatureSupported(WebViewFeature.DOCUMENT_START_SCRIPT)) return
        attached = true
        val only = setOf(origin)
        WebViewCompat.addWebMessageListener(web, "quilloAndroid", only) { _, msg, _, isMainFrame, reply ->
            if (isMainFrame) onMessage(msg, reply)
        }
        WebViewCompat.addDocumentStartJavaScript(web, SCRIPT, only)
    }

    fun close() {}

    private fun onMessage(msg: WebMessageCompat, reply: JavaScriptReplyProxy) {
        val m = try { JSONObject(msg.data ?: "{}") } catch (e: Exception) { return }
        val id = m.optInt("id")
        val args = m.optJSONArray("args") ?: JSONArray()
        val answer: (Any?, String?) -> Unit = { value, error ->
            val out = JSONObject().put("id", id)
            if (error != null) out.put("error", error) else out.put("value", value ?: JSONObject.NULL)
            activity.runOnUiThread { reply.postMessage(out.toString()) }
        }
        try {
            when (m.optString("name")) {
                "haptic" -> haptic(args.optString(0, "medium"), answer)
                "battery" -> battery(answer)
                "location" -> location(answer)
                "notify" -> notify(args, answer)
                "keepAwake" -> {
                    val on = if (args.length() > 0) args.optBoolean(0, true) else true
                    if (on) activity.window.addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
                    else activity.window.clearFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
                    answer(on, null)
                }
                "brightness" -> brightness(args, answer)
                "share" -> share(args, answer)
                "openSettings" -> {
                    activity.startActivity(Intent(Settings.ACTION_APPLICATION_DETAILS_SETTINGS, Uri.parse("package:${activity.packageName}")))
                    answer(true, null)
                }
                "info" -> answer(JSONObject().put("bridge", 1).put("calls", JSONArray(CALLS)), null)
                else -> answer(null, "no native call ${m.optString("name")}")
            }
        } catch (e: Exception) {
            answer(null, e.message ?: e.toString())
        }
    }

    // A tap the person feels, through the view: no vibration permission is needed.
    private fun haptic(kind: String, answer: (Any?, String?) -> Unit) {
        val c = when (kind) {
            "success" -> if (Build.VERSION.SDK_INT >= 30) HapticFeedbackConstants.CONFIRM else HapticFeedbackConstants.VIRTUAL_KEY
            "warning", "error" -> if (Build.VERSION.SDK_INT >= 30) HapticFeedbackConstants.REJECT else HapticFeedbackConstants.LONG_PRESS
            "selection" -> HapticFeedbackConstants.CLOCK_TICK
            "light", "soft" -> HapticFeedbackConstants.KEYBOARD_TAP
            "heavy" -> HapticFeedbackConstants.LONG_PRESS
            else -> HapticFeedbackConstants.VIRTUAL_KEY
        }
        web.performHapticFeedback(c)
        answer(kind, null)
    }

    private fun battery(answer: (Any?, String?) -> Unit) {
        val i = activity.registerReceiver(null, IntentFilter(Intent.ACTION_BATTERY_CHANGED))
        val level = i?.getIntExtra(BatteryManager.EXTRA_LEVEL, -1) ?: -1
        val scale = i?.getIntExtra(BatteryManager.EXTRA_SCALE, -1) ?: -1
        val state = when (i?.getIntExtra(BatteryManager.EXTRA_STATUS, -1)) {
            BatteryManager.BATTERY_STATUS_CHARGING -> "charging"
            BatteryManager.BATTERY_STATUS_FULL -> "full"
            BatteryManager.BATTERY_STATUS_DISCHARGING, BatteryManager.BATTERY_STATUS_NOT_CHARGING -> "unplugged"
            else -> "unknown"
        }
        val power = activity.getSystemService(Context.POWER_SERVICE) as PowerManager
        answer(JSONObject()
            .put("level", if (level >= 0 && scale > 0) level.toDouble() / scale else JSONObject.NULL)
            .put("state", state)
            .put("low_power", power.isPowerSaveMode), null)
    }

    private fun location(answer: (Any?, String?) -> Unit) {
        activity.ask(arrayOf(android.Manifest.permission.ACCESS_FINE_LOCATION, android.Manifest.permission.ACCESS_COARSE_LOCATION)) { ok ->
            if (!ok) return@ask answer(null, "location is turned off for this app")
            val lm = activity.getSystemService(Context.LOCATION_SERVICE) as LocationManager
            val provider = listOf(LocationManager.GPS_PROVIDER, LocationManager.NETWORK_PROVIDER).firstOrNull { lm.isProviderEnabled(it) }
                ?: return@ask answer(null, "location services are turned off")
            val send = { p: Location? ->
                if (p == null) answer(null, "no location: none came")
                else answer(JSONObject()
                    .put("lat", p.latitude).put("lon", p.longitude).put("accuracy", p.accuracy.toDouble())
                    .put("altitude", p.altitude).put("speed", p.speed.toDouble()).put("course", p.bearing.toDouble())
                    .put("at", p.time), null)
            }
            try {
                if (Build.VERSION.SDK_INT >= 30) {
                    lm.getCurrentLocation(provider, CancellationSignal(), activity.mainExecutor) { p -> send(p ?: lm.getLastKnownLocation(provider)) }
                } else {
                    @Suppress("DEPRECATION")
                    lm.requestSingleUpdate(provider, { p -> send(p) }, Looper.getMainLooper())
                }
            } catch (e: SecurityException) {
                answer(null, "location is turned off for this app")
            }
        }
    }

    // A local notification in `seconds`, delivered by the system's alarm even when the
    // app is not on the screen (NotifyReceiver).
    private fun notify(args: JSONArray, answer: (Any?, String?) -> Unit) {
        val title = args.optString(0, "")
        val body = args.optString(1, "")
        val seconds = args.optDouble(2, 0.0).coerceIn(0.0, 86_400.0 * 30)
        val perms = if (Build.VERSION.SDK_INT >= 33) arrayOf(android.Manifest.permission.POST_NOTIFICATIONS) else arrayOf()
        activity.ask(perms) { ok ->
            if (!ok) return@ask answer(null, "notifications are turned off for this app")
            val id = (SystemClock.elapsedRealtime() % Int.MAX_VALUE).toInt()
            val i = Intent(activity, NotifyReceiver::class.java).putExtra("title", title).putExtra("body", body).putExtra("id", id)
            val pi = PendingIntent.getBroadcast(activity, id, i, PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT)
            if (seconds <= 0.0) {
                activity.sendBroadcast(i)
            } else {
                val am = activity.getSystemService(Context.ALARM_SERVICE) as AlarmManager
                am.setAndAllowWhileIdle(AlarmManager.ELAPSED_REALTIME_WAKEUP, SystemClock.elapsedRealtime() + (seconds * 1000).toLong(), pi)
            }
            answer(id.toString(), null)
        }
    }

    // The app window's brightness, 0 to 1: read, or set while the app is on the screen.
    private fun brightness(args: JSONArray, answer: (Any?, String?) -> Unit) {
        val lp = activity.window.attributes
        if (args.length() > 0) {
            lp.screenBrightness = args.optDouble(0, 0.5).coerceIn(0.0, 1.0).toFloat()
            activity.window.attributes = lp
        }
        val now = if (lp.screenBrightness >= 0) lp.screenBrightness.toDouble()
            else Settings.System.getInt(activity.contentResolver, Settings.System.SCREEN_BRIGHTNESS, 128) / 255.0
        answer(now, null)
    }

    private fun share(args: JSONArray, answer: (Any?, String?) -> Unit) {
        val parts = (0 until args.length()).map { args.optString(it) }.filter { it.isNotEmpty() }
        if (parts.isEmpty()) return answer(null, "nothing to share")
        val send = Intent(Intent.ACTION_SEND).setType("text/plain").putExtra(Intent.EXTRA_TEXT, parts.joinToString("\n"))
        activity.startActivity(Intent.createChooser(send, null))
        answer(true, null)
    }

    companion object {
        val CALLS = listOf("haptic", "battery", "location", "notify", "keepAwake", "brightness", "share", "openSettings")

        // The page's side: each call a Promise, answered by the app's reply.
        val SCRIPT = """
            window.quilloNative = (() => {
              const port = window.quilloAndroid;
              if (!port) return undefined;
              let next = 0;
              const waiting = new Map();
              port.onmessage = (e) => {
                const m = JSON.parse(e.data);
                const w = waiting.get(m.id);
                if (!w) return;
                waiting.delete(m.id);
                if (m.error != null) w.no(new Error(m.error)); else w.ok(m.value);
              };
              const call = (name, ...args) => new Promise((ok, no) => {
                const id = ++next;
                waiting.set(id, { ok, no });
                port.postMessage(JSON.stringify({ id, name, args }));
              });
              const names = ["haptic", "battery", "location", "notify", "keepAwake", "brightness", "share", "openSettings", "info"];
              const api = Object.fromEntries(names.map((n) => [n, (...a) => call(n, ...a)]));
              api.wait = (ms) => new Promise((done) => setTimeout(() => done(true), Math.max(0, Math.min(60000, Number(ms) || 0))));
              return Object.freeze(api);
            })();
        """.trimIndent()
    }
}

/** Shows a notification the bridge scheduled. */
class NotifyReceiver : BroadcastReceiver() {
    override fun onReceive(ctx: Context, i: Intent) {
        val nm = ctx.getSystemService(Context.NOTIFICATION_SERVICE) as NotificationManager
        if (Build.VERSION.SDK_INT >= 26) nm.createNotificationChannel(NotificationChannel("program", "From the program", NotificationManager.IMPORTANCE_DEFAULT))
        val open = PendingIntent.getActivity(ctx, 0, Intent(ctx, MainActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_SINGLE_TOP), PendingIntent.FLAG_IMMUTABLE)
        @Suppress("DEPRECATION")
        val b = if (Build.VERSION.SDK_INT >= 26) android.app.Notification.Builder(ctx, "program") else android.app.Notification.Builder(ctx)
        val n = b.setContentTitle(i.getStringExtra("title")).setContentText(i.getStringExtra("body"))
            .setSmallIcon(android.R.drawable.ic_dialog_info).setContentIntent(open).setAutoCancel(true).build()
        try { nm.notify(i.getIntExtra("id", 1), n) } catch (e: SecurityException) {}
    }
}
