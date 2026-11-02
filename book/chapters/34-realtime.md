# Real time: UDP, channels, media {#realtime}

Some data cannot wait for a page to ask: a sensor's reading, a player's move, a cursor, a
camera's picture. A Quillo program takes UDP packets on a port, passes messages between
the server and its pages as they happen, plays a camera's stream in any page, and lets
pages send their own camera and microphone to each other. There is no message broker, no
socket server beside the app, and no media server: it is the same program, with the same
functions and the same checks.

> **Try it on your machine.** These programs need ports, a camera or a sender of their
> own, so each part below has a command to run. Open the pages at
> `http://127.0.0.1:8080` rather than `localhost`: see
> [Which way a page connects](#which-way-a-page-connects).

## Packets in

`udp.listen` takes the packets sent to a port, and hands each to a function:

```quillo
let readings = []

udp.listen(9000, (p) => {
    let r = json.parse(p.text ?? "")
    if is_err(r) { return udp.reply(p, "not JSON") }
    readings = readings + [r]
    udp.reply(p, "ok")
})

route GET /readings = readings
```

```sh
quillo run app.qwe
echo '{"id": "boiler", "v": 71.2}' | nc -u -w1 127.0.0.1 9000     # ok
echo 'hello' | nc -u -w1 127.0.0.1 9000                           # not JSON
curl 127.0.0.1:8080/readings                                      # [{"id":"boiler","v":71.2}]
```

- **What a packet is.** `p.text` is the packet as text (`null` when it is not UTF-8),
  `p.bytes` its bytes as a list of numbers, `p.from` the `{ host, port }` it came from,
  `p.port` the port it came to, and `p.at` when, in milliseconds since 1970.
- **A packet is handled like a request.** One at a time, in the order they came. Waiting
  on the database or `http.*` holds up no one, and the handler sees the program's
  globals, so a page following `readings` with [`live`](#live-data)
  shows each packet as it lands. An error is logged, and the next packet goes on.
- **Packets wait their turn**, up to 10,000 (`{ queue: 50000 }` for more). A full queue
  drops its oldest packet, as UDP itself would.
- **The port is taken when the program starts**, so one already in use is an error at
  once, not when the first packet comes. It stays open across `--reload` and `--watch`,
  and the new version's handler gets the packets. With `--instances`, the instances
  share the socket.

## Packets out

```quillo
fn log_line(text: str) with net = udp.send("10.0.0.5", 514, `<14>${text}`)

udp.send("10.0.0.7", 9000, { id: "door", open: true })   // a map, as JSON
udp.send("10.0.0.7", 9000, [104, 105])                     // numbers, as bytes
```

- `udp.send(host, port, data)` sends one packet: text as it is, a list of numbers as
  bytes, anything else as JSON, up to 65,507 bytes. `udp.reply(p, data)` answers a
  packet, from the port it came to, as most protocols expect.
- `{ from: 9000 }` sends from a port the program listens on, so the answer comes back to
  that port's handler.
- Both give `null`, or an error value when the host has no address or the system refuses
  the packet: `err("cannot find the address of no-such-host.invalid")`. A function that
  declares its effects needs `net` for them, as for `udp.listen`, which opens a port (see
  [Effects](#effects)).

## Channels: the server and its pages

A channel carries messages between the server and its pages as they happen. A page opens
it in a component; the program says, at the top, what to do with what pages send.

```quillo
channel.on("chat", (msg, peer) => {
    let line = { from: peer.session.name, text: msg.text }
    channel.broadcast("chat", line, { reliable: true })
}, { join: (peer) => peer.session.name != null })

component Chat() {
    state lines = []
    state draft = ""
    let chat = channel.open("chat", (m) => { lines = lines + [m] })
    <form @submit={() => {
        chat.send({ text: draft }, { reliable: true })
        draft = ""
    }}>
        <ul>{for l in lines => <li><b>{l.from}</b> {l.text}</li>}</ul>
        <input bind:value={draft}/>
    </form>
}
```

- **In a component**, `channel.open(name, f)` opens the channel for as long as the code
  that called it stands: `f` gets each message the server sends on it, and `.send(value)`
  sends one to the server. When the component leaves the page, the page leaves the
  channel. While the server renders the page, it opens nothing.
- **On the server**, `channel.on(name, f)` gets each message from a page as `f(msg,
  peer)`, where `peer` is `{ id, session }`. `session()` inside `f` is that page's
  session too (as it was when the page connected). Messages are handled like UDP
  packets: one at a time, in order, with waits holding up no one.
- **Sending**: `channel.broadcast(name, value)` sends to every page in the channel and
  gives how many; `{ except: peer }` leaves one out. `channel.send(name, peer, value)`
  sends to one page, by its peer or its id, and gives whether it was still there.
  `channel.peers(name)` gives the ids of the pages in it. None of them waits: a page
  that cannot keep up misses messages rather than holding up the program.

### Who may join

Anyone who can open a page can ask to join any channel, so a channel whose messages are
not for everyone needs `join`. It gets the peer and says yes or no; above, only pages
whose session has a name are let in. A page turned away says so in its console:

```
quillo: the server did not let this page join channel chat
```

A channel that no `channel.on` names can be joined by anyone, to hear what the program
broadcasts on it; what pages send on it goes nowhere. `leave(peer)` hears that a page
left the channel, or closed.

## Lost, late, or in order

Not every message has to arrive. A cursor's position or a sensor's reading is out of date
by the time the next one comes; a chat line or a "you won" must arrive, and in order.
Channels let each message say which it is:

```quillo
channel.on("cursors", (msg, peer) => {
    let at = { id: peer.id, x: msg.x, y: msg.y }
    channel.broadcast("cursors", at, { except: peer })         // a datagram
}, { leave: (peer) => {
    let gone = { id: peer.id, gone: true }
    channel.broadcast("cursors", gone, { reliable: true })     // on a stream
} })

component Cursors() {
    state others = {}
    let room = channel.open("cursors", (m) => {
        others = m.gone ? remove(others, m.id) : set(others, m.id, m)
    })
    <div.board @pointermove={(e) => room.send({ x: e.clientX, y: e.clientY })}>
        {for c in values(others) =>
            <i.cursor key={c.id} style={`left:${c.x}px;top:${c.y}px`}/>}
    </div>
}
```

| Message | Over WebTransport | Over a WebSocket |
|---|---|---|
| Small (about 1,100 bytes or less, as JSON) | A datagram: as soon as it can, but it may come out of order or not at all | Reliably, in order |
| Larger, or sent with `{ reliable: true }` | On a stream: all of them, in order | Reliably, in order |

The positions go as datagrams. A lost one is replaced by the next, a moment later, and
nothing waits for it, which is what keeps cursors smooth on a poor connection.
"Gone" is sent reliably, so no page keeps a cursor for someone who left. Either side can
ask for `reliable`: `room.send(v, { reliable: true })` in a page,
`channel.broadcast(name, v, { reliable: true })` on the server.

## From a device to every page

UDP and channels together pass a device's packets on to pages as they come. No
`channel.on` is needed when pages only listen:

```quillo
udp.listen(9000, (p) => {
    let r = json.parse(p.text ?? "")
    if !is_err(r) { channel.broadcast("readings", r) }
})

component Gauge() {
    state last = null
    channel.open("readings", (r) => { last = r })
    <p>{last == null ? "Waiting for a reading" : `${last.id}: ${last.v}`}</p>
}
```

The [Telemetry](https://github.com/quillolang/examples/blob/main/telemetry.qwe) example goes further: four pretend sensors
send readings twice a second, the server keeps the last minute of each, and a page starts
from that (a `server fn` sets its first state) and follows the channel from there.

```sh
quillo run telemetry.qwe --watch     # in quillolang/examples; then open http://127.0.0.1:8080
quillo run sensors.qwe               # in another terminal
```

## A camera in the page

`media.rtp` takes a camera's or an encoder's RTP stream, and `<quillo-media>` plays it in
any page, a fraction of a second behind:

```quillo
media.rtp("cam", { video: 5004, audio: 5006 })

route GET / = <html><body><quillo-media stream="cam" controls muted/></body></html>
```

Send it a test picture and a tone with ffmpeg:

```sh
ffmpeg -re -f lavfi -i testsrc=size=1280x720:rate=30 -f lavfi -i sine=frequency=440 \
  -map 0:v -c:v libx264 -tune zerolatency -profile:v baseline -pix_fmt yuv420p -g 60 \
  -f rtp rtp://127.0.0.1:5004 -map 1:a -c:a libopus -f rtp rtp://127.0.0.1:5006
```

On a laptop, the picture shows about 0.3 seconds after the page opens, at the stream's
30 frames a second.

- **Nothing is decoded or encoded on the server.** Video is H.264 and audio Opus, each on
  a port of its own. The server puts the packets back in order, makes frames of them,
  and passes each frame on to every page watching. There are no HLS segments and no
  transcoding, so the delay is the network's and the encoder's, not seconds of buffer.
- **A new viewer sees a picture at once.** The server keeps the video since the last
  keyframe, so a page that starts watching starts there. A page that falls behind skips
  to the next keyframe, and so does a stream that lost a packet of video. Keyframes every
  second or two (`-g 60` at 30 fps) keep both short; `-tune zerolatency` keeps the
  encoder from holding frames back.
- **The page decodes with the browser's own decoders** (WebCodecs), from a small script
  (4.5 KB compressed) that loads only on pages with `<quillo-media>`. Its `data-state`
  is `connecting`, `waiting`, `playing`, `ended` or `unsupported`, for styling. `controls`
  adds a button for sound and `muted` starts without it: browsers play sound only after a
  click on the page.
- **A dropped connection comes back.** The page opens it again (after half a second, then
  longer, up to 15 seconds) and goes on watching. With no frames for 3 seconds (the
  camera stopped, the connection is down), its `data-state` is `waiting`.
- **How it is going**: `media.streams()` gives each stream's viewers and, per track,
  `frames`, `fps`, `kbps` and `lost` packets.

```quillo
server fn live_streams() -> list = media.streams()

component Stats() {
    state streams = live_streams()
    every(1000, () => { streams = live_streams() ?? streams })
    <p>{for s in streams =>
        <span key={s.name}>{s.video.fps} fps · {s.viewers} watching</span>}</p>
}
```

The [Camera](https://github.com/quillolang/examples/blob/main/media.qwe) example is a whole page, with the command for a Mac's
own camera and microphone.

## Pages that send: a video call

A page can send too. `media.on` lets pages send a stream (or every stream a name ending
in `*` starts), and `<quillo-media publish>` sends the page's camera and microphone to it,
for every `<quillo-media stream>` of that name to play. A video call is a stream per
person:

```quillo
media.on("call:*", { publish: (peer) => session().user != null })

component Call(room: str, me: str, others: list) {
    <div>
        <quillo-media publish={`call:${room}:${me}`}/>
        {for p in others => <quillo-media key={p} stream={`call:${room}:${p}`}/>}
    </div>
}
```

- **Encoded in the browser, passed on by the server.** The page encodes with WebCodecs
  (H.264 baseline and Opus, what the server passes on) and sends the frames on its
  channel connection. The server sends each person's picture on to everyone watching,
  decoding nothing, as it does a camera's RTP: what video services call an SFU.
- **`publish(peer)` says who may**, as a channel's `join` does, with the page's
  `session()`. Without it, any page may.
- **Not only the camera.** `no-video` or `no-audio` leaves one out. Given a MediaStream as
  its `source` (a canvas's, or the screen's:
  `el.source = js("navigator.mediaDevices.getDisplayMedia")` in `@mount`), it sends that
  instead.
- **How the sending goes.** Its `data-state` is `starting`, `sending`, `refused` (no
  camera, or `publish` said no), `ended` or `unsupported`, and `el.mediaStats` counts
  what it encoded and sent.
- **Networks that block UDP** get no TURN relay: the page's WebSocket carries the frames
  instead (see [Which way a page connects](#which-way-a-page-connects)).

## Ending a stream

Streams can be made while the program runs: `media.rtp` in a route or a channel's handler
makes one for each room, once for a name. `media.close(name)` ends one: it stops taking
RTP, lets its ports go for another stream to take, and tells its pages, whose `data-state`
turns `ended`. It gives true if there was such a stream.

## Which way a page connects

A page opens one connection for all its channels and streams, and opens it again (and
joins its channels again) when it drops:

- **WebTransport** where it can: HTTP/3 over UDP, on the UDP port with the HTTP port's
  number. This is what carries datagrams.
- **A WebSocket** where it cannot: the browser lacks WebTransport, UDP is blocked on the
  way, or the server cannot be reached over it within a second and a half. Everything
  still arrives, reliably and in order, so a page works the same, only without
  datagrams. A page that fell back keeps the WebSocket until it is loaded again.

In the Network panel, the connection is the request to `/_quillo/wt`; if its type is
**websocket**, the page fell back.

On your own machine, open `http://127.0.0.1:8080` rather than `localhost`. `quillo run`
listens on 127.0.0.1, and browsers try `localhost` over IPv6 first for UDP. They find
nothing there and use a WebSocket instead. `--host ::1` serves `localhost` over IPv6, if
you would rather keep the name.

## In production

- **Open the UDP port.** WebTransport uses the HTTP port's number over UDP (443 for 443),
  or the one `--wt-port` gives. Firewalls and cloud security groups often open only TCP.
- **A certificate.** The server makes one of its own, which browsers accept by its hash,
  and a new one before each ends (browsers take such certificates for two weeks at
  most). `--tls-cert` and `--tls-key` give it a real one.
- **Behind a proxy or a CDN** that passes HTTP only, open the UDP port to the server
  itself, or pages use the WebSocket through the proxy.
- **One server's pages.** Broadcasts reach the pages connected to this server. With
  `--metrics`, `quillo_channel_pages` counts them, `quillo_channel_dropped_total` the
  messages dropped because a page's queue was full, and `quillo_udp_received_total` and
  `quillo_udp_dropped_total` the packets.
- `--no-wt` turns WebTransport off, and every page uses the WebSocket.

## Limits, and what it does not do

- **Messages are values, as JSON**, up to 1 MiB each: there are no binary messages
  (bytes go as a list of numbers). Each page has room for 4,096 messages on their way to
  it; when that fills, a plain message is dropped, and a `reliable` one closes the
  connection so the page joins again rather than miss it. Nothing is kept for a page
  while it is away.
- **A page's session is the one it connected with.** Someone who signs in afterwards is
  seen so by `join` and `channel.on` once the page reconnects. A page that reconnects has
  a new `peer.id`.
- **No presence list** beyond `channel.peers(name)` and what `join` and `leave` record.
- **One server's pages.** Channels and media streams are kept in memory by the server
  the page is connected to: servers behind limer share live data, not channels or
  streams, and a restart starts them empty (pages reconnect and join again by
  themselves).
- **No WebRTC and no TURN.** Pages talk to the server, never to each other directly; the
  server relays video. Streams are not recorded; have the encoder write a copy.
- **UDP is plain**, IPv4 unless `host: "::"`, without multicast or DTLS. RTP has no
  RTCP or SRTP, and any sender that reaches a port is taken: keep media ports to the
  camera's network.
- **No WebSocket of your own.** Channels are the way a page's messages reach the
  program.

## Which one to use

| You want | Use |
|---|---|
| Pages to show data as it changes in the database or in a variable | [`live(load)`](#live-data) |
| One answer sent as it is made: a reply word by word, a job's progress | [`stream`, `events`](#streaming) with `listen` or `receive` |
| Messages both ways, many a second, between the server and its pages | Channels |
| Packets from devices or other programs | `udp.listen` |
| A camera or an encoder, live | `media.rtp` and `<quillo-media>` |
| Pages' own cameras, microphones or screens, to each other: a video call | `media.on` and `<quillo-media publish>` |

The reference for each is under Routes: [UDP](#udp),
[Channels](#channels) and [Media](#media).
