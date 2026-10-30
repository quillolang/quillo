// A Quillo program as an iOS app: the runtime serves it on 127.0.0.1, inside this
// process, and the one screen is a web view on it. See mobile/README.md.
//
// What the web view cannot reach (haptics, the battery, local notifications, keeping
// the screen on, the screen's brightness, the share sheet, a location without the web
// view's own prompt) the app offers the page as `window.quilloNative`, each call a
// Promise answered by the native side (`Bridge` below); a Quillo component calls them
// with js("quilloNative.haptic", "success") and the like.

import CoreLocation
import UIKit
import UserNotifications
import WebKit

@main
final class AppDelegate: UIResponder, UIApplicationDelegate {
    func application(_ application: UIApplication, didFinishLaunchingWithOptions launchOptions: [UIApplication.LaunchOptionsKey: Any]? = nil) -> Bool {
        UNUserNotificationCenter.current().delegate = Notices.shared
        return true
    }

    func application(_ application: UIApplication, configurationForConnecting session: UISceneSession, options: UIScene.ConnectionOptions) -> UISceneConfiguration {
        let c = UISceneConfiguration(name: "Program", sessionRole: session.role)
        c.delegateClass = SceneDelegate.self
        return c
    }
}

final class SceneDelegate: UIResponder, UIWindowSceneDelegate {
    var window: UIWindow?

    func scene(_ scene: UIScene, willConnectTo session: UISceneSession, options: UIScene.ConnectionOptions) {
        guard let scene = scene as? UIWindowScene else { return }
        Program.startOnce(screen: scene.screen)
        let window = UIWindow(windowScene: scene)
        window.rootViewController = ProgramViewController()
        window.makeKeyAndVisible()
        self.window = window
    }

    func sceneDidEnterBackground(_ scene: UIScene) { print("quillo-ios: background") }
    func sceneWillEnterForeground(_ scene: UIScene) { print("quillo-ios: foreground") }
}

/// Notifications show while the app is open too.
final class Notices: NSObject, UNUserNotificationCenterDelegate {
    static let shared = Notices()
    func userNotificationCenter(_ center: UNUserNotificationCenter, willPresent notification: UNNotification, withCompletionHandler done: @escaping (UNNotificationPresentationOptions) -> Void) {
        done([.banner, .sound, .list])
    }
}

/// What starting the program gave: the URL to open, or why it did not start.
enum Started {
    case url(URL)
    case failed(String)
}

/// The program, started once per process.
enum Program {
    nonisolated(unsafe) static var result: Started = .failed("not started")

    /// What only the app knows of the device, for the program's `platform()`.
    static func device(screen: UIScreen) -> String {
        let d = UIDevice.current
        var sys = utsname()
        uname(&sys)
        let machine = withUnsafeBytes(of: &sys.machine) { String(decoding: $0.prefix(while: { $0 != 0 }), as: UTF8.self) }
        #if targetEnvironment(simulator)
        let simulator = true
        let model = ProcessInfo.processInfo.environment["SIMULATOR_MODEL_IDENTIFIER"] ?? machine
        #else
        let simulator = false
        let model = machine
        #endif
        let info = Bundle.main.infoDictionary ?? [:]
        let size = screen.bounds.size
        let facts: [String: Any] = [
            "os": "ios",
            "os_name": d.systemName,
            "os_version": d.systemVersion,
            "device": d.userInterfaceIdiom == .pad ? "tablet" : "phone",
            "model": model,
            "model_family": d.model,
            "simulator": simulator,
            "memory": ProcessInfo.processInfo.physicalMemory,
            "cpus": ProcessInfo.processInfo.activeProcessorCount,
            "locale": Locale.preferredLanguages.first ?? Locale.current.identifier,
            "time_zone": TimeZone.current.identifier,
            "screen": ["width": Int(size.width), "height": Int(size.height), "scale": Double(screen.scale)],
            "app": [
                "id": info["CFBundleIdentifier"] as? String ?? "",
                "name": info["CFBundleDisplayName"] as? String ?? info["CFBundleName"] as? String ?? "",
                "version": info["CFBundleShortVersionString"] as? String ?? "",
                "build": info["CFBundleVersion"] as? String ?? "",
            ],
        ]
        // Keys in order: a Swift dictionary's order changes from one launch to the next.
        let json = try? JSONSerialization.data(withJSONObject: facts, options: [.sortedKeys])
        return json.flatMap { String(data: $0, encoding: .utf8) } ?? "{}"
    }

    static func startOnce(screen: UIScreen) {
        if case .url = result { return }
        let fm = FileManager.default
        let bundle = Bundle.main.resourceURL!.appendingPathComponent("program").path
        let data = fm.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0].appendingPathComponent("quillo")
        try? fm.createDirectory(at: data, withIntermediateDirectories: true)
        let main = (Bundle.main.object(forInfoDictionaryKey: "QuilloMain") as? String) ?? "main.qwe"
        var out = [CChar](repeating: 0, count: 4096)
        let started = Date()
        let code = quillo_start(bundle, data.path, main, device(screen: screen), &out, out.count)
        let said = out.withUnsafeBufferPointer { String(cString: $0.baseAddress!) }
        let ms = Int(Date().timeIntervalSince(started) * 1000)
        if code == 0, let url = URL(string: said) {
            print("quillo-ios: started in \(ms) ms, data in \(data.path)")
            result = .url(url)
        } else {
            print("quillo-ios: did not start: \(said)")
            result = .failed(said)
        }
    }
}

// The page's side of the bridge: window.quilloNative.<name>(...) posts to the app and
// gives a Promise of its answer.
let bridgeScript = """
window.quilloNative = (() => {
  const call = (name, ...args) => window.webkit.messageHandlers.quillo.postMessage({ name, args });
  const names = ["haptic", "battery", "location", "notify", "keepAwake", "brightness", "share", "openSettings", "info"];
  const api = Object.fromEntries(names.map((n) => [n, (...a) => call(n, ...a)]));
  // A pause, which a Quillo component cannot make itself.
  api.wait = (ms) => new Promise((done) => setTimeout(() => done(true), Math.max(0, Math.min(60000, Number(ms) || 0))));
  return Object.freeze(api);
})();
"""

final class ProgramViewController: UIViewController, WKNavigationDelegate, WKUIDelegate {
    private var web: WKWebView!
    private var origin = ""
    private var bridge: Bridge!
    // `--eval <script>`: run in the page when it has loaded, for automation
    // (screenshots, tests). Given more than once, the scripts run in turn, one per page
    // loaded, so one can go to a page and the next act on it. Only launch arguments turn
    // it on, which on a phone only Xcode or `simctl launch` can pass.
    private var pendingEvals: [String] = {
        let args = ProcessInfo.processInfo.arguments
        return args.indices.filter { args[$0] == "--eval" && $0 + 1 < args.count }.map { args[$0 + 1] }
    }()

    override func viewDidLoad() {
        super.viewDidLoad()
        view.backgroundColor = .systemBackground
        let config = WKWebViewConfiguration()
        // A <video> of the camera plays in the page, not in the full-screen player.
        config.allowsInlineMediaPlayback = true
        config.mediaTypesRequiringUserActionForPlayback = []
        bridge = Bridge(owner: self)
        config.userContentController.addScriptMessageHandler(bridge, contentWorld: .page, name: "quillo")
        config.userContentController.addUserScript(WKUserScript(source: bridgeScript, injectionTime: .atDocumentStart, forMainFrameOnly: true))
        web = WKWebView(frame: .zero, configuration: config)
        web.navigationDelegate = self
        web.uiDelegate = self
        web.translatesAutoresizingMaskIntoConstraints = false
        web.isInspectable = true
        view.addSubview(web)
        let g = view.safeAreaLayoutGuide
        NSLayoutConstraint.activate([
            web.topAnchor.constraint(equalTo: g.topAnchor),
            web.bottomAnchor.constraint(equalTo: view.bottomAnchor),
            web.leadingAnchor.constraint(equalTo: view.leadingAnchor),
            web.trailingAnchor.constraint(equalTo: view.trailingAnchor),
        ])
        switch Program.result {
        case .url(let url):
            origin = "\(url.scheme ?? "http")://\(url.host ?? "127.0.0.1"):\(url.port ?? 80)"
            web.load(URLRequest(url: url))
        case .failed(let why):
            web.loadHTMLString("<meta name=viewport content='width=device-width'><body style='font:16px -apple-system;padding:24px'><h2>The program did not start</h2><pre style='white-space:pre-wrap'>\(why)</pre></body>", baseURL: nil)
        }
    }

    func isOwn(_ origin: WKSecurityOrigin) -> Bool { origin.host == "127.0.0.1" }

    // The program's own pages stay here; a link to another site opens in Safari.
    func webView(_ webView: WKWebView, decidePolicyFor action: WKNavigationAction, decisionHandler: @escaping @MainActor (WKNavigationActionPolicy) -> Void) {
        guard let url = action.request.url, let scheme = url.scheme else { return decisionHandler(.allow) }
        let mine = url.absoluteString.hasPrefix(origin) || scheme == "about" || scheme == "data" || scheme == "blob"
        if mine || origin.isEmpty { return decisionHandler(.allow) }
        if scheme == "http" || scheme == "https" { UIApplication.shared.open(url) }
        decisionHandler(.cancel)
    }

    func webView(_ webView: WKWebView, didFinish navigation: WKNavigation!) {
        guard !pendingEvals.isEmpty else { return }
        let js = pendingEvals.removeFirst()
        webView.evaluateJavaScript(js + "\n;0") { _, error in
            if let error { print("quillo-ios: --eval failed: \(error)") }
        }
    }

    // iOS may end the web view's process while the app is away: show the page again.
    func webViewWebContentProcessDidTerminate(_ webView: WKWebView) { webView.reload() }

    // The camera and the microphone, for the program's own pages: the app has asked
    // the person already (Info.plist says why), so the page does not ask again.
    func webView(_ webView: WKWebView, requestMediaCapturePermissionFor origin: WKSecurityOrigin, initiatedByFrame frame: WKFrameInfo, type: WKMediaCaptureType, decisionHandler: @escaping @MainActor (WKPermissionDecision) -> Void) {
        decisionHandler(isOwn(origin) ? .grant : .deny)
    }

    // Motion and orientation (DeviceOrientationEvent.requestPermission()), likewise.
    func webView(_ webView: WKWebView, requestDeviceOrientationAndMotionPermissionFor origin: WKSecurityOrigin, initiatedByFrame frame: WKFrameInfo, decisionHandler: @escaping @MainActor (WKPermissionDecision) -> Void) {
        decisionHandler(isOwn(origin) ? .grant : .deny)
    }

    // alert, confirm and prompt do nothing in a web view unless the app shows them.
    func webView(_ webView: WKWebView, runJavaScriptAlertPanelWithMessage message: String, initiatedByFrame frame: WKFrameInfo, completionHandler: @escaping @MainActor () -> Void) {
        let a = UIAlertController(title: nil, message: message, preferredStyle: .alert)
        a.addAction(UIAlertAction(title: "OK", style: .default) { _ in completionHandler() })
        present(a, animated: true)
    }

    func webView(_ webView: WKWebView, runJavaScriptConfirmPanelWithMessage message: String, initiatedByFrame frame: WKFrameInfo, completionHandler: @escaping @MainActor (Bool) -> Void) {
        let a = UIAlertController(title: nil, message: message, preferredStyle: .alert)
        a.addAction(UIAlertAction(title: "Cancel", style: .cancel) { _ in completionHandler(false) })
        a.addAction(UIAlertAction(title: "OK", style: .default) { _ in completionHandler(true) })
        present(a, animated: true)
    }

    func webView(_ webView: WKWebView, runJavaScriptTextInputPanelWithPrompt prompt: String, defaultText: String?, initiatedByFrame frame: WKFrameInfo, completionHandler: @escaping @MainActor (String?) -> Void) {
        let a = UIAlertController(title: nil, message: prompt, preferredStyle: .alert)
        a.addTextField { $0.text = defaultText }
        a.addAction(UIAlertAction(title: "Cancel", style: .cancel) { _ in completionHandler(nil) })
        a.addAction(UIAlertAction(title: "OK", style: .default) { _ in completionHandler(a.textFields?.first?.text) })
        present(a, animated: true)
    }
}

/// The native side of window.quilloNative: each call answered once, with a value or
/// an error message (the page's Promise rejects, and Quillo's js() gives an error value).
final class Bridge: NSObject, WKScriptMessageHandlerWithReply, CLLocationManagerDelegate {
    private weak var owner: UIViewController?
    private let places = CLLocationManager()
    private var waitingForPlace: [(Any?, String?) -> Void] = []

    init(owner: UIViewController) {
        self.owner = owner
        super.init()
        places.delegate = self
        places.desiredAccuracy = kCLLocationAccuracyBest
    }

    func userContentController(_ controller: WKUserContentController, didReceive message: WKScriptMessage, replyHandler: @escaping @MainActor (Any?, String?) -> Void) {
        // Only the program's own pages reach the app.
        guard message.frameInfo.securityOrigin.host == "127.0.0.1" else { return replyHandler(nil, "not from the program") }
        let body = message.body as? [String: Any] ?? [:]
        let name = body["name"] as? String ?? ""
        let args = body["args"] as? [Any] ?? []
        switch name {
        case "haptic": haptic(args.first as? String ?? "medium", replyHandler)
        case "battery": battery(replyHandler)
        case "location": location(replyHandler)
        case "notify": notify(args, replyHandler)
        case "keepAwake":
            UIApplication.shared.isIdleTimerDisabled = (args.first as? Bool) ?? true
            replyHandler(UIApplication.shared.isIdleTimerDisabled, nil)
        case "brightness": brightness(args, replyHandler)
        case "share": share(args, replyHandler)
        case "openSettings":
            UIApplication.shared.open(URL(string: UIApplication.openSettingsURLString)!)
            replyHandler(true, nil)
        case "info": replyHandler(["bridge": 1, "calls": ["haptic", "battery", "location", "notify", "keepAwake", "brightness", "share", "openSettings"]], nil)
        default: replyHandler(nil, "no native call \(name)")
        }
    }

    private func haptic(_ kind: String, _ reply: @escaping (Any?, String?) -> Void) {
        switch kind {
        case "success", "warning", "error":
            let g = UINotificationFeedbackGenerator()
            g.notificationOccurred(kind == "success" ? .success : kind == "warning" ? .warning : .error)
        case "selection":
            UISelectionFeedbackGenerator().selectionChanged()
        default:
            let style: UIImpactFeedbackGenerator.FeedbackStyle = kind == "light" ? .light : kind == "heavy" ? .heavy : kind == "rigid" ? .rigid : kind == "soft" ? .soft : .medium
            UIImpactFeedbackGenerator(style: style).impactOccurred()
        }
        reply(kind, nil)
    }

    private func battery(_ reply: @escaping (Any?, String?) -> Void) {
        let d = UIDevice.current
        d.isBatteryMonitoringEnabled = true
        let state: String
        switch d.batteryState {
        case .charging: state = "charging"
        case .full: state = "full"
        case .unplugged: state = "unplugged"
        default: state = "unknown"
        }
        // -1 when the system does not say (the simulator).
        let level: Any = d.batteryLevel < 0 ? NSNull() : Double(d.batteryLevel)
        reply(["level": level, "state": state, "low_power": ProcessInfo.processInfo.isLowPowerModeEnabled], nil)
    }

    private func location(_ reply: @escaping (Any?, String?) -> Void) {
        switch places.authorizationStatus {
        case .denied, .restricted: return reply(nil, "location is turned off for this app")
        default: break
        }
        waitingForPlace.append(reply)
        if places.authorizationStatus == .notDetermined {
            places.requestWhenInUseAuthorization()
        } else {
            places.requestLocation()
        }
    }

    func locationManagerDidChangeAuthorization(_ manager: CLLocationManager) {
        guard !waitingForPlace.isEmpty else { return }
        switch manager.authorizationStatus {
        case .authorizedWhenInUse, .authorizedAlways: manager.requestLocation()
        case .denied, .restricted: answerPlace(nil, "location is turned off for this app")
        default: break
        }
    }

    func locationManager(_ manager: CLLocationManager, didUpdateLocations locations: [CLLocation]) {
        guard let p = locations.last else { return }
        answerPlace([
            "lat": p.coordinate.latitude, "lon": p.coordinate.longitude, "accuracy": p.horizontalAccuracy,
            "altitude": p.altitude, "speed": p.speed, "course": p.course, "at": p.timestamp.timeIntervalSince1970 * 1000,
        ], nil)
    }

    func locationManager(_ manager: CLLocationManager, didFailWithError error: Error) {
        answerPlace(nil, "no location: \(error.localizedDescription)")
    }

    private func answerPlace(_ value: Any?, _ error: String?) {
        let waiting = waitingForPlace
        waitingForPlace = []
        for r in waiting { r(value, error) }
    }

    private func notify(_ args: [Any], _ reply: @escaping (Any?, String?) -> Void) {
        let title = args.first as? String ?? ""
        let body = args.count > 1 ? (args[1] as? String ?? "") : ""
        let seconds = args.count > 2 ? max(1, (args[2] as? Double) ?? 1) : 1
        let center = UNUserNotificationCenter.current()
        center.requestAuthorization(options: [.alert, .sound, .badge]) { ok, error in
            DispatchQueue.main.async {
                guard ok else { return reply(nil, error?.localizedDescription ?? "notifications are turned off for this app") }
                let content = UNMutableNotificationContent()
                content.title = title
                content.body = body
                content.sound = .default
                let id = UUID().uuidString
                let req = UNNotificationRequest(identifier: id, content: content, trigger: UNTimeIntervalNotificationTrigger(timeInterval: seconds, repeats: false))
                center.add(req) { error in
                    DispatchQueue.main.async { error == nil ? reply(id, nil) : reply(nil, error!.localizedDescription) }
                }
            }
        }
    }

    private func brightness(_ args: [Any], _ reply: @escaping (Any?, String?) -> Void) {
        guard let screen = owner?.view.window?.windowScene?.screen else { return reply(nil, "no screen") }
        if let v = args.first as? Double { screen.brightness = CGFloat(min(1, max(0, v))) }
        reply(Double(screen.brightness), nil)
    }

    private func share(_ args: [Any], _ reply: @escaping (Any?, String?) -> Void) {
        guard let owner else { return reply(nil, "no screen") }
        let items: [Any] = args.compactMap { a in
            if let s = a as? String, let u = URL(string: s), u.scheme == "https" { return u }
            return a as? String
        }
        let sheet = UIActivityViewController(activityItems: items, applicationActivities: nil)
        sheet.completionWithItemsHandler = { activity, done, _, error in
            if let error { return reply(nil, error.localizedDescription) }
            reply(["shared": done, "to": activity?.rawValue as Any? ?? NSNull()], nil)
        }
        sheet.popoverPresentationController?.sourceView = owner.view
        owner.present(sheet, animated: true)
    }
}
