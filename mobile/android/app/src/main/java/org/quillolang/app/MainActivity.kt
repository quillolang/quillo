package org.quillolang.app

import android.app.Activity
import android.app.ActivityManager
import android.content.Intent
import android.net.Uri
import android.os.Build
import android.os.Bundle
import android.util.Log
import android.webkit.WebResourceRequest
import android.webkit.WebView
import android.webkit.WebViewClient
import org.json.JSONObject
import java.io.File
import java.util.Locale
import java.util.TimeZone

/** A Quillo program as an Android app: one WebView on what the runtime serves. */
class MainActivity : Activity() {
    private lateinit var web: WebView
    private var origin = ""

    override fun onCreate(state: Bundle?) {
        super.onCreate(state)
        web = WebView(this)
        web.settings.javaScriptEnabled = true
        web.settings.domStorageEnabled = true
        setContentView(web)
        web.webViewClient = object : WebViewClient() {
            // The program's own pages stay here; a link to another site opens in the browser.
            override fun shouldOverrideUrlLoading(view: WebView, req: WebResourceRequest): Boolean {
                val url = req.url.toString()
                if (origin.isEmpty() || url.startsWith(origin)) return false
                if (url.startsWith("http://") || url.startsWith("https://")) startActivity(Intent(Intent.ACTION_VIEW, Uri.parse(url)))
                return true
            }
        }
        if (state != null) { web.restoreState(state); return }
        val said = Program.start(this)
        if (said.startsWith("error: ")) {
            web.loadData("<meta name=viewport content='width=device-width'><body style='font:16px sans-serif;padding:24px'><h2>The program did not start</h2><pre style='white-space:pre-wrap'>${said.removePrefix("error: ")}</pre>", "text/html", "utf-8")
        } else {
            origin = Uri.parse(said).let { "${it.scheme}://${it.host}:${it.port}" }
            web.loadUrl(said)
        }
    }

    override fun onSaveInstanceState(out: Bundle) { super.onSaveInstanceState(out); web.saveState(out) }

    @Deprecated("The system back button goes back in the program's pages first.")
    override fun onBackPressed() { if (web.canGoBack()) web.goBack() else super.onBackPressed() }
}

/** The program, started once per process. */
object Program {
    private var said: String? = null

    fun start(activity: Activity): String {
        said?.let { return it }
        // Assets are not files: the program is unpacked into the app's files first.
        val bundle = File(activity.filesDir, "bundle")
        bundle.deleteRecursively()
        unpack(activity, "program", bundle)
        val data = File(activity.filesDir, "quillo").apply { mkdirs() }
        val started = System.currentTimeMillis()
        val result = Quillo.start(bundle.path, data.path, "main.qwe", device(activity))
        Log.i("quillo", "${if (result.startsWith("error: ")) "did not start" else "started"} in ${System.currentTimeMillis() - started} ms: data in ${data.path}")
        said = result
        return result
    }

    private fun unpack(activity: Activity, from: String, to: File) {
        val names = activity.assets.list(from) ?: return
        if (names.isEmpty()) {
            to.parentFile?.mkdirs()
            activity.assets.open(from).use { i -> to.outputStream().use { o -> i.copyTo(o) } }
            return
        }
        to.mkdirs()
        for (n in names) unpack(activity, "$from/$n", File(to, n))
    }

    /** What only the app knows of the device, for the program's platform(). */
    private fun device(activity: Activity): String {
        val metrics = activity.resources.displayMetrics
        val mem = ActivityManager.MemoryInfo().also { (activity.getSystemService(ACTIVITY_SERVICE) as ActivityManager).getMemoryInfo(it) }
        val pkg = activity.packageManager.getPackageInfo(activity.packageName, 0)
        val emulator = Build.FINGERPRINT.startsWith("generic") || Build.FINGERPRINT.contains("emulator") || Build.MODEL.contains("sdk_gphone") || Build.HARDWARE.contains("ranchu")
        val tablet = activity.resources.configuration.smallestScreenWidthDp >= 600
        return JSONObject()
            .put("os", "android")
            .put("os_name", "Android")
            .put("os_version", Build.VERSION.RELEASE)
            .put("sdk", Build.VERSION.SDK_INT)
            .put("device", if (tablet) "tablet" else "phone")
            .put("model", Build.MODEL)
            .put("model_family", Build.MANUFACTURER)
            .put("simulator", emulator)
            .put("memory", mem.totalMem)
            .put("cpus", Runtime.getRuntime().availableProcessors())
            .put("locale", Locale.getDefault().toLanguageTag())
            .put("time_zone", TimeZone.getDefault().id)
            .put("screen", JSONObject()
                .put("width", (metrics.widthPixels / metrics.density).toInt())
                .put("height", (metrics.heightPixels / metrics.density).toInt())
                .put("scale", metrics.density.toDouble()))
            .put("app", JSONObject()
                .put("id", activity.packageName)
                .put("name", activity.applicationInfo.loadLabel(activity.packageManager).toString())
                .put("version", pkg.versionName ?: "")
                .put("build", if (Build.VERSION.SDK_INT >= 28) pkg.longVersionCode.toString() else @Suppress("DEPRECATION") pkg.versionCode.toString()))
            .toString()
    }
}
