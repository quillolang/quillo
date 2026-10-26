// The server benchmark on Node's http module (node.js PORT [processes]).
import http from "node:http";
import cluster from "node:cluster";
import { fortunesPage, work } from "./fortunes.js";

const port = Number(process.argv[2] || 8080), procs = Number(process.argv[3] || 1);
if (procs > 1 && cluster.isPrimary) {
  for (let i = 0; i < procs; i++) cluster.fork();
} else {
  const send = (res, type, body) => { res.writeHead(200, { "Content-Type": type, "Content-Length": Buffer.byteLength(body) }); res.end(body); };
  http.createServer((req, res) => {
    switch (req.url) {
      case "/text": return send(res, "text/plain; charset=utf-8", "Hello, World!");
      case "/json": return send(res, "application/json", JSON.stringify({ message: "Hello, World!" }));
      case "/fortunes": return send(res, "text/html; charset=utf-8", fortunesPage());
      case "/compute": return send(res, "application/json", JSON.stringify({ total: work(10000) }));
      default: res.writeHead(404); res.end();
    }
  }).listen(port, "127.0.0.1");
}
