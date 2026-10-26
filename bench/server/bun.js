// The server benchmark on Bun.serve (bun bun.js PORT [processes]).
import { spawn } from "bun";
import { fortunesPage, work } from "./fortunes.js";

const port = Number(process.argv[2] || 8080), procs = Number(process.argv[3] || 1);
if (procs > 1 && !process.env.BENCH_CHILD) {
  for (let i = 0; i < procs; i++) spawn([process.execPath, import.meta.path, String(port), "1"], { env: { ...process.env, BENCH_CHILD: "1" }, stdio: ["ignore", "inherit", "inherit"] });
} else {
  Bun.serve({
    port, hostname: "127.0.0.1", reusePort: true,
    fetch(req) {
      const path = req.url.slice(req.url.indexOf("/", 8));
      switch (path) {
        case "/text": return new Response("Hello, World!", { headers: { "Content-Type": "text/plain; charset=utf-8" } });
        case "/json": return Response.json({ message: "Hello, World!" });
        case "/fortunes": return new Response(fortunesPage(), { headers: { "Content-Type": "text/html; charset=utf-8" } });
        case "/compute": return Response.json({ total: work(10000) });
        default: return new Response(null, { status: 404 });
      }
    },
  });
}
