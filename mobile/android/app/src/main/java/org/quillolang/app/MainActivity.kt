package org.quillolang.app

import android.app.Activity
import android.app.ActivityManager
import android.content.Intent
import android.content.pm.PackageManager
import android.net.Uri
import android.os.Build
import android.os.Bundle
import android.provider.MediaStore
import android.util.Base64
import androidx.core.content.FileProvider
import android.util.Log
import android.view.WindowInsets
import android.view.WindowInsetsController
import android.widget.FrameLayout
import android.webkit.ConsoleMessage
import android.webkit.GeolocationPermissions
import android.webkit.PermissionRequest
import android.webkit.ValueCallback
import android.webkit.WebChromeClient
import android.webkit.WebResourceRequest
import android.webkit.WebView
import android.webkit.WebViewClient
import org.json.JSONObject
import java.io.File
import java.util.Locale
import java.util.TimeZone

/**
 * A Quillo program as an Android app: one WebView on what the runtime serves on
 * 127.0.0.1, with `window.quilloNative` on the program's own pages for what a
 * WebView cannot reach (see Bridge.kt), as the iOS app has.
 */
class MainActivity : Activity() {
    private lateinit var web: WebView
    private lateinit var bridge: Bridge
    private var origin = ""
    // `--es eval "<js>"` (and eval2, eval3, ...): run in the page as each one loads, in
    // turn, for screenshots and tests. Only a launch from adb or the IDE passes them.
    private val pendingEvals = ArrayDeque<String>()
    private var chooser: ValueCallback<Array<Uri>>? = null
    private var captured: Uri? = null

    // Runtime permissions asked for, by request code, and what to do with the answer.
    private val asked = HashMap<Int, (Boolean) -> Unit>()
    private var nextAsk = 100

    override fun onCreate(state: Bundle?) {
        super.onCreate(state)
        web = WebView(this)
        web.settings.javaScriptEnabled = true
        web.settings.domStorageEnabled = true
        web.settings.mediaPlaybackRequiresUserGesture = false
        web.settings.setGeolocationEnabled(true)
        // Android 15 draws apps edge to edge: the page keeps clear of the status bar,
        // the navigation bar and the keyboard (as a web view on iOS does by itself). A
        // WebView ignores its own padding, so a frame around it takes it.
        val frame = FrameLayout(this)
        frame.addView(web, FrameLayout.LayoutParams(FrameLayout.LayoutParams.MATCH_PARENT, FrameLayout.LayoutParams.MATCH_PARENT))
        setContentView(frame)
        // Dark icons on the bars, over a light page.
        if (Build.VERSION.SDK_INT >= 30) {
            val light = WindowInsetsController.APPEARANCE_LIGHT_STATUS_BARS or WindowInsetsController.APPEARANCE_LIGHT_NAVIGATION_BARS
            window.insetsController?.setSystemBarsAppearance(light, light)
        }
        frame.setOnApplyWindowInsetsListener { v, insets ->
            if (Build.VERSION.SDK_INT >= 30) {
                val bars = insets.getInsets(WindowInsets.Type.systemBars() or WindowInsets.Type.displayCutout() or WindowInsets.Type.ime())
                v.setPadding(bars.left, bars.top, bars.right, bars.bottom)
                WindowInsets.CONSUMED
            } else {
                @Suppress("DEPRECATION")
                v.setPadding(insets.systemWindowInsetLeft, insets.systemWindowInsetTop, insets.systemWindowInsetRight, insets.systemWindowInsetBottom)
                @Suppress("DEPRECATION")
                insets.consumeSystemWindowInsets()
            }
        }
        bridge = Bridge(this, web)
        intent?.extras?.let { x ->
            // A script may come as "base64:...", which adb's shell passes untouched.
            val script = { v: String -> if (v.startsWith("base64:")) String(Base64.decode(v.removePrefix("base64:"), Base64.DEFAULT)) else v }
            x.getString("eval")?.let { pendingEvals.add(script(it)) }
            for (i in 2..9) x.getString("eval$i")?.let { pendingEvals.add(script(it)) }
        }
        web.webViewClient = object : WebViewClient() {
            // The program's own pages stay here; a link to another site opens in the browser.
            override fun shouldOverrideUrlLoading(view: WebView, req: WebResourceRequest): Boolean {
                val url = req.url.toString()
                if (origin.isEmpty() || url.startsWith("$origin/") || url == origin) return false
                if (url.startsWith("about:") || url.startsWith("data:") || url.startsWith("blob:")) return false
                if (url.startsWith("http://") || url.startsWith("https://")) startActivity(Intent(Intent.ACTION_VIEW, Uri.parse(url)))
                return true
            }

            override fun onPageFinished(view: WebView, url: String) {
                val js = pendingEvals.removeFirstOrNull() ?: return
                view.evaluateJavascript("$js\n;0", null)
            }
        }
        web.webChromeClient = object : WebChromeClient() {
            // The camera and the microphone, for the program's own pages: the app asks the
            // person once (the system's own prompt), then the page does not ask again.
            override fun onPermissionRequest(req: PermissionRequest) {
                if (!own(req.origin.toString())) return req.deny()
                val want = req.resources.mapNotNull {
                    when (it) {
                        PermissionRequest.RESOURCE_VIDEO_CAPTURE -> android.Manifest.permission.CAMERA
                        PermissionRequest.RESOURCE_AUDIO_CAPTURE -> android.Manifest.permission.RECORD_AUDIO
                        else -> null
                    }
                }
                ask(want.toTypedArray()) { ok -> runOnUiThread { if (ok) req.grant(req.resources) else req.deny() } }
            }

            // The page's own geolocation: granted for the program's pages once the app has
            // the location permission, so the person is asked once, not twice.
            override fun onGeolocationPermissionsShowPrompt(at: String, cb: GeolocationPermissions.Callback) {
                if (!own(at)) return cb.invoke(at, false, false)
                ask(arrayOf(android.Manifest.permission.ACCESS_FINE_LOCATION, android.Manifest.permission.ACCESS_COARSE_LOCATION)) { ok -> cb.invoke(at, ok, false) }
            }

            // <input type="file">: the system's picker; with `capture` and images accepted,
            // the camera, as on iOS. The photo is written where FileProvider can hand it
            // to the page.
            override fun onShowFileChooser(view: WebView, cb: ValueCallback<Array<Uri>>, params: FileChooserParams): Boolean {
                chooser?.onReceiveValue(null)
                chooser = cb
                val images = params.acceptTypes.any { it.isEmpty() || it.startsWith("image/") }
                if (params.isCaptureEnabled && images) {
                    ask(arrayOf(android.Manifest.permission.CAMERA)) { ok ->
                        if (!ok) { chooser?.onReceiveValue(null); chooser = null; return@ask }
                        val dir = File(cacheDir, "captures").apply { mkdirs() }
                        val photo = File(dir, "photo-${System.currentTimeMillis()}.jpg")
                        val uri = FileProvider.getUriForFile(this@MainActivity, "$packageName.files", photo)
                        captured = uri
                        val shoot = Intent(MediaStore.ACTION_IMAGE_CAPTURE).putExtra(MediaStore.EXTRA_OUTPUT, uri)
                            .addFlags(Intent.FLAG_GRANT_WRITE_URI_PERMISSION or Intent.FLAG_GRANT_READ_URI_PERMISSION)
                        try { startActivityForResult(shoot, CAPTURE_REQUEST) } catch (e: Exception) { chooser?.onReceiveValue(null); chooser = null }
                    }
                    return true
                }
                return try {
                    startActivityForResult(params.createIntent(), FILE_REQUEST)
                    true
                } catch (e: Exception) {
                    chooser = null
                    false
                }
            }

            override fun onConsoleMessage(m: ConsoleMessage): Boolean {
                Log.i("quillo-page", "${m.message()} (${m.sourceId()}:${m.lineNumber()})")
                return true
            }
        }
        if (state != null) web.restoreState(state)
        val said = Program.start(this)
        if (said.startsWith("error: ")) {
            web.loadData("<meta name=viewport content='width=device-width'><body style='font:16px sans-serif;padding:24px'><h2>The program did not start</h2><pre style='white-space:pre-wrap'>${said.removePrefix("error: ")}</pre>", "text/html", "utf-8")
            return
        }
        origin = Uri.parse(said).let { "${it.scheme}://${it.host}:${it.port}" }
        bridge.attach(origin)
        if (state == null) web.loadUrl(said)
    }

    private fun own(url: String) = origin.isNotEmpty() && (url == origin || url == "$origin/" || url.startsWith("$origin/"))

    /** Asks for [perms] the person has not granted yet; [done] gets whether all were. */
    fun ask(perms: Array<String>, done: (Boolean) -> Unit) {
        val missing = perms.filter { checkSelfPermission(it) != PackageManager.PERMISSION_GRANTED }
        if (missing.isEmpty()) return done(true)
        val code = nextAsk++
        asked[code] = done
        requestPermissions(missing.toTypedArray(), code)
    }

    override fun onRequestPermissionsResult(code: Int, perms: Array<out String>, results: IntArray) {
        super.onRequestPermissionsResult(code, perms, results)
        // Location is granted when either fine or coarse is.
        val ok = results.isNotEmpty() && (results.all { it == PackageManager.PERMISSION_GRANTED } ||
            (perms.all { it.contains("LOCATION") } && results.any { it == PackageManager.PERMISSION_GRANTED }))
        asked.remove(code)?.invoke(ok)
    }

    @Deprecated("Activity results still come here on the platform's own Activity.")
    override fun onActivityResult(code: Int, result: Int, data: Intent?) {
        super.onActivityResult(code, result, data)
        if (code == FILE_REQUEST) {
            chooser?.onReceiveValue(WebChromeClient.FileChooserParams.parseResult(result, data))
            chooser = null
        } else if (code == CAPTURE_REQUEST) {
            val uri = captured
            chooser?.onReceiveValue(if (result == RESULT_OK && uri != null) arrayOf(uri) else null)
            chooser = null
            captured = null
        }
    }

    override fun onSaveInstanceState(out: Bundle) { super.onSaveInstanceState(out); web.saveState(out) }

    override fun onDestroy() { bridge.close(); super.onDestroy() }

    @Deprecated("The system back button goes back in the program's pages first.")
    override fun onBackPressed() { if (web.canGoBack()) web.goBack() else @Suppress("DEPRECATION") super.onBackPressed() }

    companion object { const val FILE_REQUEST = 7; const val CAPTURE_REQUEST = 8 }
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
        Log.i("quillo", "${if (result.startsWith("error: ")) "did not start: $result" else "started"} in ${System.currentTimeMillis() - started} ms: data in ${data.path}")
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
        val mem = ActivityManager.MemoryInfo().also { (activity.getSystemService(Activity.ACTIVITY_SERVICE) as ActivityManager).getMemoryInfo(it) }
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
