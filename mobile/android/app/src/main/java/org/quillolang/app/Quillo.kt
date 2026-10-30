package org.quillolang.app

/** The Quillo runtime (libquillo_mobile.so; mobile/quillo-mobile). */
object Quillo {
    init { System.loadLibrary("quillo_mobile") }

    /**
     * Copies the program at [bundle] into `[data]/app`, starts it and gives the URL
     * the WebView opens, or "error: ..." when it did not start. [device] is a JSON
     * object of what the app knows of the device, for the program's platform().
     */
    external fun start(bundle: String, data: String, main: String, device: String): String
}
