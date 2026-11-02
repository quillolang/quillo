package org.quillolang.app

/** The Quillo runtime (libquillo_mobile.so; mobile/quillo-mobile). */
object Quillo {
    init { System.loadLibrary("quillo_mobile") }

    /**
     * Copies the program at [bundle] into `[data]/app`, starts it and gives the URL
     * the WebView opens, or "error: ..." when it did not start. [device] is a JSON
     * object of what the app knows of the device, for the program's platform().
     * The old way, a server on 127.0.0.1: kept for apps built before [open].
     */
    external fun start(bundle: String, data: String, main: String, device: String): String

    /**
     * Opens the program in this process, with no socket (FEEDBACK39.md, Q-465 to Q-470):
     * "" when it opened, else why not.
     */
    external fun open(bundle: String, data: String, main: String, device: String): String

    /**
     * One request of the WebView's, answered whole: its status (4 bytes, big-endian), the
     * length of its headers' JSON (4 bytes), that JSON, then the body.
     */
    external fun request(method: String, url: String, headersJson: String, body: ByteArray?): ByteArray

    /** A message from the page's bridge (the shell's script posts them). */
    external fun bridge(message: String)

    /** The next message for the page, waiting up to [timeoutMs]; null when none came. */
    external fun bridgeNext(timeoutMs: Int): String?

    /** The script to run before every page. */
    external fun shellJs(post: String, bodies: String): String

    /** The app went behind the home screen. */
    external fun suspend()
}
