// A Quillo program as an iOS app: the runtime serves it on 127.0.0.1, inside this
// process, and the one screen is a web view on it. See mobile/README.md.

import UIKit
import WebKit

@main
final class AppDelegate: UIResponder, UIApplicationDelegate {
    func application(_ application: UIApplication, didFinishLaunchingWithOptions launchOptions: [UIApplication.LaunchOptionsKey: Any]? = nil) -> Bool {
        true
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
        let json = try? JSONSerialization.data(withJSONObject: facts)
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

final class ProgramViewController: UIViewController, WKNavigationDelegate {
    private var web: WKWebView!
    private var origin = ""
    // `--eval <script>`: run once in the page when it has loaded, for automation
    // (screenshots, tests). Only a launch argument turns it on, which on a phone only
    // Xcode or `simctl launch` can pass.
    private var pendingEval: String? = {
        let args = ProcessInfo.processInfo.arguments
        guard let i = args.firstIndex(of: "--eval"), i + 1 < args.count else { return nil }
        return args[i + 1]
    }()

    override func viewDidLoad() {
        super.viewDidLoad()
        view.backgroundColor = .systemBackground
        web = WKWebView(frame: .zero, configuration: WKWebViewConfiguration())
        web.navigationDelegate = self
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

    // The program's own pages stay here; a link to another site opens in Safari.
    func webView(_ webView: WKWebView, decidePolicyFor action: WKNavigationAction, decisionHandler: @escaping @MainActor (WKNavigationActionPolicy) -> Void) {
        guard let url = action.request.url, let scheme = url.scheme else { return decisionHandler(.allow) }
        let mine = url.absoluteString.hasPrefix(origin) || scheme == "about" || scheme == "data"
        if mine || origin.isEmpty { return decisionHandler(.allow) }
        if scheme == "http" || scheme == "https" { UIApplication.shared.open(url) }
        decisionHandler(.cancel)
    }

    func webView(_ webView: WKWebView, didFinish navigation: WKNavigation!) {
        guard let js = pendingEval else { return }
        pendingEval = nil
        webView.evaluateJavaScript(js + "\n;0") { _, error in
            if let error { print("quillo-ios: --eval failed: \(error)") }
        }
    }

    // iOS may end the web view's process while the app is away: show the page again.
    func webViewWebContentProcessDidTerminate(_ webView: WKWebView) { webView.reload() }
}
