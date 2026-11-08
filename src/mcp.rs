//! A program's tools, as an MCP client is told them (FEEDBACK56.md, Part B; branch feedback56).
//!
//! Each `fn mcp` and `server fn mcp`, in the order the program declares them: its name, its
//! `///` as the description, its parameters as the input schema and its result as the output
//! schema, both JSON Schema 2020-12 (the dialect MCP takes when a schema names none), and the
//! hints its `with` and its code allow. The door (`quillo mcp`, `/_quillo/mcp`) lists these and
//! calls them with `call`, which runs a tool found to only read in a read-only transaction.
//!
//! ```text
//! /// The orders placed today, newest first.
//! fn mcp orders_today(limit: int = 20) -> list with db {
//!     db.query("select id, total from orders order by id desc limit ?", limit)
//! }
//! ```
//!
//! is `{ "name": "orders_today", "description": "The orders placed today, newest first.",
//! "inputSchema": { "type": "object", "properties": { "limit": { "type": "integer", "default":
//! 20 } }, "additionalProperties": false }, "outputSchema": { "type": "object", "properties":
//! { "result": { "type": "array" } }, "required": ["result"] }, "annotations": { "readOnlyHint":
//! true, "openWorldHint": false } }`: a result that is not an object is held as `result`, in
//! `structuredContent` too (`Tool::structured`).

use crate::ast::{effect_bit, effect_mask, FnDef};
use crate::check::{Analysis, ToolFacts};
use crate::describe::{literal, Schemas};
use crate::interp::{Ctrl, Interp};
use crate::record::Types;
use crate::value::Value;
use serde_json::{json, Map, Value as J};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

/// A tool, as `tools/list` gives it, and what the door needs to call it.
#[derive(Debug, Clone)]
pub struct Tool {
    pub name: String,
    pub description: Option<String>,
    pub input_schema: J,
    pub output_schema: Option<J>,
    /// Only the hints that are not MCP's defaults: `readOnlyHint: true`, `openWorldHint: false`
    /// (B.3). `destructiveHint` and `idempotentHint` are left to the defaults: nothing in Quillo
    /// says whether a write destroys, and a call sent again is a second call.
    pub annotations: Map<String, J>,
    /// Said to only read (`readOnlyHint`), and it may reach the database: its `with` names `db`,
    /// or it has none and its code was seen to call `db`. The door runs it, its arguments made
    /// too, in a read-only transaction (`call`), so a statement or a call the checker misjudged
    /// is refused by the database. A tool that reaches no database runs with none, so it works
    /// with no tenant and no database to be had (Q-858, found in review).
    pub reads_only: bool,
    /// Its result is not an object, so `outputSchema` and `structuredContent` hold it as
    /// `{ "result": ... }` (`structured`; found in review).
    pub wrapped: bool,
    pub def: Arc<FnDef>,
}

impl Tool {
    /// A result as `structuredContent`: the value, or `{ "result": value }` where it is not an
    /// object, as `outputSchema` says. None for a tool with no result type, whose answer is sent
    /// as text alone, and never a bare value that is not an object (found in review).
    pub fn structured(&self, value: J) -> Option<J> {
        match (&self.output_schema, self.wrapped) {
            (None, _) => None,
            (Some(_), true) => Some(json!({ "result": value })),
            (Some(_), false) => value.is_object().then_some(value),
        }
    }

    /// The tool's entry in `tools/list`.
    pub fn to_json(&self) -> J {
        let mut t = Map::new();
        t.insert("name".into(), json!(self.name));
        if let Some(d) = &self.description {
            t.insert("description".into(), json!(d));
        }
        t.insert("inputSchema".into(), self.input_schema.clone());
        if let Some(o) = &self.output_schema {
            t.insert("outputSchema".into(), o.clone());
        }
        if !self.annotations.is_empty() {
            t.insert("annotations".into(), J::Object(self.annotations.clone()));
        }
        J::Object(t)
    }
}

/// The tools of a program as the checker read it, in the order the program declares them.
pub fn tools(a: &Analysis) -> Vec<Tool> {
    let types: HashMap<String, Types> = HashMap::from([(String::new(), a.types.clone())]);
    a.tools.iter().map(|f| tool(f, &types)).collect()
}

/// The tools of the program in `path`, read without running it; its errors, as `quillo check`
/// says them, when it has any: a program with errors does not run.
pub fn of_source(path: &Path) -> Result<Vec<Tool>, String> {
    let a = crate::check::check_file(path);
    let errors: Vec<String> = a.errors().map(|d| d.to_string()).collect();
    if !errors.is_empty() {
        return Err(errors.join("\n"));
    }
    Ok(tools(&a))
}

/// `tools/list`'s list.
pub fn list(tools: &[Tool]) -> J {
    J::Array(tools.iter().map(Tool::to_json).collect())
}

fn tool(f: &ToolFacts, types: &HashMap<String, Types>) -> Tool {
    let d = &f.def;
    // The arguments come by name, as an object with no other keys: each parameter, its
    // default said, and required without one, as the door refuses a call without it (B.1).
    let mut input = Schemas::for_tools(types, true);
    let mut props = Map::new();
    let mut required = Vec::new();
    for p in &d.params {
        let mut s = p.ty.as_ref().map_or(json!({}), |t| input.ty(t));
        match p.default.as_ref() {
            Some(e) => {
                if let Some(v) = literal(e) {
                    s["default"] = v;
                }
            }
            None => required.push(json!(p.name.as_str())),
        }
        props.insert(p.name.to_string(), s);
    }
    let mut input_schema = json!({ "type": "object" });
    if !props.is_empty() {
        input_schema["properties"] = J::Object(props);
    }
    if !required.is_empty() {
        input_schema["required"] = J::Array(required);
    }
    input_schema["additionalProperties"] = json!(false);
    let defs = input.written();
    if !defs.is_empty() {
        input_schema["$defs"] = J::Object(defs);
    }
    // The result is held to its types' names, as a server fn's is: a map type's fields are not
    // said to be required (Q-856), a record's are, and nothing is said of other keys, which
    // nothing checks. Its root is an object, as MCP's revision 2025-11-25 and its SDKs require:
    // a record's `$ref` says so beside it, anything else is held as `result` (found in review).
    let mut wrapped = false;
    let output_schema = d.ret.as_ref().map(|r| {
        let mut out = Schemas::for_tools(types, false);
        let mut s = out.ty(r);
        let record = s.get("$ref").and_then(J::as_str).and_then(|k| k.strip_prefix("#/$defs/")).is_some_and(|k| a_record(types, k)) && s.as_object().is_some_and(|o| o.len() == 1);
        if record {
            s["type"] = json!("object");
        } else if s.get("type") != Some(&json!("object")) {
            wrapped = true;
            s = json!({ "type": "object", "properties": { "result": s }, "required": ["result"] });
        }
        let defs = out.written();
        if !defs.is_empty() {
            s["$defs"] = J::Object(defs);
        }
        s
    });
    let declared = d.effects.as_deref().map(effect_mask);
    let (read_only, closed) = hints(declared, f);
    let mut annotations = Map::new();
    if read_only {
        annotations.insert("readOnlyHint".into(), json!(true));
    }
    if closed {
        annotations.insert("openWorldHint".into(), json!(false));
    }
    Tool {
        name: d.name.clone(),
        description: d.doc.clone(),
        input_schema,
        output_schema,
        annotations,
        // A `with` that names `db` may reach it whatever the walk found: a call it misjudged is
        // what the transaction is for. One without `db` is held at run time; none, as the walk
        // saw it (found in review).
        reads_only: read_only && declared.map_or(f.db, |m| m & effect_bit("db") != 0),
        wrapped,
        def: d.clone(),
    }
}

/// Whether the program's type of key `k` (the program's own, prefix "") is a record.
fn a_record(types: &HashMap<String, Types>, k: &str) -> bool {
    types.get("").and_then(|t| t.get(k)).is_some_and(|td| td.is_record())
}

/// Whether a tool only reads, and whether it stays inside the program, by its `with` (`declared`)
/// and what its code was found to do (B.3, Q-858). A `with` is held at run time, so what it does
/// not name the tool cannot do; what it names is looked into:
///
/// - It only reads when its `with` has none of `fs`, `net` and `http`, and the checker sees all
///   its code (`ToolFacts::uncertain`) and no write in it (`ToolFacts::writes`): with `db`, each
///   statement is a `select` written in the program, and the door runs it in a read-only
///   transaction.
/// - It stays inside the program when its `with` has no `net`, and no `http` or only calls of
///   the program's own services whose code was read and stays inside too, the checker seeing all
///   its code: a native plugin's function is not seen, whatever its file says (found in review).
/// - Without a `with`, it may do anything: both are MCP's defaults, unless the checker sees all
///   its code and it uses no effect, writes nothing and calls nothing outside.
fn hints(declared: Option<u8>, f: &ToolFacts) -> (bool, bool) {
    let [fs, net, http] = [effect_bit("fs"), effect_bit("net"), effect_bit("http")];
    let sure = f.uncertain.is_none();
    match declared {
        None => {
            let pure = sure && f.effects == 0 && f.writes.is_none() && f.open.is_none();
            (pure, pure)
        }
        Some(m) => (m & (fs | net | http) == 0 && sure && f.writes.is_none(), m & net == 0 && sure && f.open.is_none()),
    }
}

/// Calls the tool as the door does, with its arguments in order as they came: each made to its
/// type (`server::make_args`, which runs a record's defaults) under the tool's `with`, as its
/// body is, then the call, both in the read-only transaction of a tool said to only read that may
/// reach the database (`Tool::reads_only`; Q-858). `Ok(Err(why))` is an argument refused, with
/// the door's message; an error value in `Ok(Ok(..))` is the tool's own error; a default that
/// fails, an effect its `with` does not allow among them, fails the call as the body would.
pub fn call(it: &mut Interp, tool: &Tool, args: Vec<Value>, line: u32) -> Result<Result<Value, String>, Ctrl> {
    call_given(it, tool, args.into_iter().map(Some).collect(), line)
}

/// `call`, each argument given or not: one not given (None) takes its parameter's default, as
/// MCP's arguments come by name and may leave out one before another (`by_name`; FEEDBACK56.md,
/// C.1; branch feedback56).
pub fn call_given(it: &mut Interp, tool: &Tool, args: Vec<Option<Value>>, line: u32) -> Result<Result<Value, String>, Ctrl> {
    let c = match crate::value::lookup(&it.globals, tool.name.as_str()) {
        Some(Value::Func(c)) => c,
        _ => return crate::interp::fail(line, format!("the program has no tool `{}`", tool.name)),
    };
    let def = tool.def.clone();
    let mut refused = None;
    let mut args = Some(args);
    let mut run = |it: &mut Interp| {
        // What the `with` allows, as the call narrows it (`Interp::call_def`), while the
        // arguments are made (found in review).
        let narrowed = def.effects.as_deref().map(|e| {
            let mask = effect_mask(e) & it.allowed;
            (std::mem::replace(&mut it.allowed, mask), std::mem::replace(&mut it.restrictor, Some(def.clone())))
        });
        let made = made_args(it, &def, args.take().unwrap_or_default());
        if let Some((allowed, who)) = narrowed {
            it.allowed = allowed;
            it.restrictor = who;
        }
        match made {
            Ok(Ok(made)) => it.call_closure(&c, made, line),
            Ok(Err(why)) => {
                refused = Some(why);
                Ok(Value::Nil)
            }
            Err((_, failed)) => Err(failed),
        }
    };
    let answer = if tool.reads_only { reading_only(it, line, &mut run)? } else { run(it)? };
    Ok(match refused {
        Some(why) => Err(why),
        None => Ok(answer),
    })
}

/// Runs `body` in a read-only transaction on the program's database, so the database refuses
/// any write in it: SQLite's `query_only`, PostgreSQL's `begin read only`, MySQL's and MariaDB's
/// `start transaction read only`, each driver's own (src/db.rs). The refusal is a database error
/// whose `db.error_kind` is `"read_only"` on each. The door keeps its own records outside it.
pub fn reading_only(it: &mut Interp, line: u32, body: &mut dyn FnMut(&mut Interp) -> Result<Value, Ctrl>) -> Result<Value, Ctrl> {
    let opts = crate::txn::Options { read_only: true, ..Default::default() };
    crate::txn::run(it, None, &opts, line, body)
}

/// `server::make_args` for arguments that may each be left out: the same refusals, said the same
/// way (one too many, a missing one without a default, one of the wrong type), each given one made
/// by `server::make_arg`, and one left out kept None for its default.
fn made_args(it: &mut Interp, def: &FnDef, args: Vec<Option<Value>>) -> Result<Result<Vec<Option<Value>>, String>, (String, Ctrl)> {
    if args.len() > def.params.len() {
        return Ok(Err(format!("`{}` takes {} arguments, got {}", def.name, def.params.len(), args.len())));
    }
    if let Some(p) = def.params.iter().enumerate().find(|(i, p)| p.default.is_none() && !matches!(args.get(*i), Some(Some(_)))).map(|(_, p)| p) {
        return Ok(Err(format!("missing argument `{}` for `{}`", p.name, def.name)));
    }
    let mut out = Vec::with_capacity(args.len());
    for (v, p) in args.into_iter().zip(&def.params) {
        out.push(match v {
            Some(v) => match crate::server::make_arg(it, def, p, v)? {
                Ok(x) => Some(x),
                Err(why) => return Ok(Err(why)),
            },
            None => None,
        });
    }
    Ok(Ok(out))
}

/// A call's arguments as MCP sends them, an object by name (B.1), in the order of the tool's
/// parameters: one left out is None, for its default. A name the tool has no parameter of is
/// refused, as its schema's `additionalProperties: false` says, with the names it takes.
pub fn by_name(tool: &Tool, args: &Map<String, J>) -> Result<Vec<Option<Value>>, String> {
    let def = &tool.def;
    if let Some(k) = args.keys().find(|k| !def.params.iter().any(|p| p.name.as_str() == k.as_str())) {
        let names: Vec<String> = def.params.iter().map(|p| format!("`{}`", p.name)).collect();
        return Err(match names.len() {
            0 => format!("`{}` takes no arguments, got `{}`", def.name, k),
            _ => format!("`{}` has no argument `{}`: it takes {}", def.name, k, names.join(", ")),
        });
    }
    let mut out: Vec<Option<Value>> = def.params.iter().map(|p| args.get(p.name.as_str()).map(Value::from_json)).collect();
    while matches!(out.last(), Some(None)) {
        out.pop();
    }
    Ok(out)
}

// ---- The door: MCP's messages (FEEDBACK56.md, Part C; branch feedback56) ----------------------
//
// As MCP's revision 2026-07-28 says them (schema/2026-07-28/schema.ts; basic/index,
// basic/versioning, basic/transports/stdio, server/discover, server/tools,
// server/utilities/caching and pagination): every request carries its version and the client's
// capabilities in `_meta`, every result its `resultType`, and nothing is kept between requests.
// `answer` is the transport's half: a line of stdin for `quillo mcp`, a body over HTTP later.

/// The revision the door answers, and the only one (the user, 2026-11-08: a client of 2025-11-25,
/// which opens with `initialize`, is told the version it should speak, with no fallback).
pub const PROTOCOL: &str = "2026-07-28";

/// JSON-RPC's codes, and MCP's own in the range it keeps (`-32020` to `-32099`).
pub const PARSE_ERROR: i64 = -32700;
pub const INVALID_REQUEST: i64 = -32600;
pub const METHOD_NOT_FOUND: i64 = -32601;
pub const INVALID_PARAMS: i64 = -32602;
pub const UNSUPPORTED_PROTOCOL_VERSION: i64 = -32022;

/// How long a client may keep `server/discover`'s and `tools/list`'s answers. A program's tools
/// are read when the door starts and change only when it is started again, so an hour; and they
/// are the same for every caller, so `public`.
pub const TTL_MS: u64 = 3_600_000;

/// What a tool that answered with a page is told (A.1).
pub const NOT_A_PAGE: &str = "a tool answers with data, not a page";

/// A program's door: its tools, in source order, and who it is.
pub struct Door {
    pub tools: Vec<Tool>,
    /// `serverInfo`'s name: the program's, as `quillo docs` titles it (its file's name).
    pub name: String,
}

/// An error answered in place of a result.
struct Fault {
    code: i64,
    message: String,
    data: Option<J>,
}

fn fault(code: i64, message: impl Into<String>) -> Fault {
    Fault { code, message: message.into(), data: None }
}

/// A JSON-RPC error. The id is left out where it could not be read: MCP's ids are never null.
fn error_message(id: Option<&J>, f: Fault) -> J {
    let mut e = json!({ "code": f.code, "message": f.message });
    if let Some(d) = f.data {
        e["data"] = d;
    }
    match id {
        Some(id) => json!({ "jsonrpc": "2.0", "id": id, "error": e }),
        None => json!({ "jsonrpc": "2.0", "error": e }),
    }
}

/// The version error: what the client asked for, and what the door speaks.
fn unsupported(requested: &str) -> Fault {
    Fault {
        code: UNSUPPORTED_PROTOCOL_VERSION,
        message: format!("Unsupported protocol version: this server speaks MCP {} only", PROTOCOL),
        data: Some(json!({ "supported": [PROTOCOL], "requested": requested })),
    }
}

/// A tool's answer that is an error the model reads: its own `err(v)`, an argument refused, a
/// `reply`, a page, a failure said as `internal error` (B.1).
fn tool_error(text: impl Into<String>) -> J {
    json!({ "content": [{ "type": "text", "text": text.into() }], "isError": true })
}

/// What an error value or a reply's body says to the model: text as it is, any other value as
/// its JSON.
fn text_of(v: &Value) -> String {
    match v {
        Value::Str(s) => s.to_string(),
        other => other.json_text(),
    }
}

impl Door {
    pub fn new(name: impl Into<String>, tools: Vec<Tool>) -> Door {
        Door { tools, name: name.into() }
    }

    /// The answer to one message, as it came: None for a notification and for a client's
    /// response, which are never answered.
    pub fn answer(&self, it: &mut Interp, text: &[u8]) -> Option<J> {
        // As deep as every reader of JSON takes it (`value::json_value`).
        let msg = match crate::value::json_value(text) {
            Ok(J::Object(m)) => m,
            // One message a line: JSON-RPC's batches left MCP in 2025-06-18.
            Ok(J::Array(_)) => return Some(error_message(None, fault(INVALID_REQUEST, "Invalid request: one message a line, not a batch"))),
            Ok(_) => return Some(error_message(None, fault(INVALID_REQUEST, "Invalid request: a message is a JSON object"))),
            Err(e) => return Some(error_message(None, fault(PARSE_ERROR, format!("Parse error: {}", e)))),
        };
        let id = match msg.get("id") {
            // A notification, the client's to send and never answered; `notifications/cancelled`
            // among them, for a call that is answered before the next line is read.
            None if msg.get("method").is_some_and(J::is_string) => return None,
            // A response: the door asks the client nothing, so there is none to wait for.
            None | Some(_) if !msg.contains_key("method") && (msg.contains_key("result") || msg.contains_key("error")) => return None,
            None => return Some(error_message(None, fault(INVALID_REQUEST, "Invalid request: `method` is a string"))),
            // A string or an integer; never null (basic/index).
            Some(id @ J::String(_)) => id,
            Some(id @ J::Number(n)) if n.is_i64() || n.is_u64() => id,
            Some(_) => return Some(error_message(None, fault(INVALID_REQUEST, "Invalid request: an id is a string or an integer"))),
        };
        Some(match self.request(it, &msg) {
            // Every result says it is complete (MCP 2026-07-28 requires `resultType`), and who
            // answered it (servers SHOULD say `serverInfo` in every result).
            Ok(result) => {
                let mut r = Map::new();
                r.insert("resultType".into(), json!("complete"));
                if let J::Object(fields) = result {
                    r.extend(fields);
                }
                r.insert("_meta".into(), json!({ "io.modelcontextprotocol/serverInfo": { "name": self.name, "version": crate::version() } }));
                json!({ "jsonrpc": "2.0", "id": id, "result": r })
            }
            Err(f) => error_message(Some(id), f),
        })
    }

    fn request(&self, it: &mut Interp, msg: &Map<String, J>) -> Result<J, Fault> {
        if msg.get("jsonrpc").and_then(J::as_str) != Some("2.0") {
            return Err(fault(INVALID_REQUEST, "Invalid request: `jsonrpc` is \"2.0\""));
        }
        let Some(method) = msg.get("method").and_then(J::as_str) else {
            return Err(fault(INVALID_REQUEST, "Invalid request: `method` is a string"));
        };
        let empty = Map::new();
        let params = match msg.get("params") {
            None => &empty,
            Some(J::Object(p)) => p,
            Some(_) => return Err(fault(INVALID_PARAMS, "Invalid params: `params` is an object")),
        };
        let meta = params.get("_meta").and_then(J::as_object);
        let version = meta.and_then(|m| m.get("io.modelcontextprotocol/protocolVersion")).and_then(J::as_str);
        // A client of 2025-11-25 and before: told the version to speak, which the spec asks of a
        // server that answers only the new one (basic/versioning, Backward Compatibility). Sent
        // with 2026-07-28's `_meta`, it is a method that revision does not have, and the client is
        // not told to try again with the version it sent.
        if method == "initialize" && version != Some(PROTOCOL) {
            return Err(unsupported(params.get("protocolVersion").and_then(J::as_str).or(version).unwrap_or("")));
        }
        if let Some(v) = version.filter(|v| *v != PROTOCOL) {
            return Err(unsupported(v));
        }
        if version.is_none() || !meta.and_then(|m| m.get("io.modelcontextprotocol/clientCapabilities")).is_some_and(J::is_object) {
            return Err(fault(INVALID_PARAMS, "Invalid params: `_meta` carries io.modelcontextprotocol/protocolVersion and io.modelcontextprotocol/clientCapabilities"));
        }
        match method {
            "server/discover" => Ok(self.discover()),
            "tools/list" => self.list(params),
            "tools/call" => self.call(it, params),
            other => Err(fault(METHOD_NOT_FOUND, format!("Method not found: {}", other))),
        }
    }

    /// `server/discover`: the versions, the one capability, and how long they hold.
    pub fn discover(&self) -> J {
        let mut d = json!({
            "supportedVersions": [PROTOCOL],
            "capabilities": { "tools": {} },
            "ttlMs": TTL_MS,
            "cacheScope": "public",
        });
        // Part D, the lines Quillo writes and the main file's `//!` block, goes here as
        // `instructions` (slice 6).
        if let Some(text) = self.instructions() {
            d["instructions"] = json!(text);
        }
        d
    }

    /// What the model is told of the server as a whole: nothing yet (Part D, slice 6).
    fn instructions(&self) -> Option<String> {
        None
    }

    /// `tools/list`: every tool in one page, in source order. There is no next page, so a cursor
    /// is one the door never gave (server/utilities/pagination).
    fn list(&self, params: &Map<String, J>) -> Result<J, Fault> {
        if params.get("cursor").is_some() {
            return Err(fault(INVALID_PARAMS, "Invalid cursor"));
        }
        Ok(json!({ "tools": list(&self.tools), "ttlMs": TTL_MS, "cacheScope": "public" }))
    }

    /// `tools/call`: a tool by name, its arguments by name. A name the door has no tool of, or a
    /// request not of `tools/call`'s shape, is a protocol error; everything the tool says, its
    /// arguments refused among it, is a result (server/tools, Error Handling).
    fn call(&self, it: &mut Interp, params: &Map<String, J>) -> Result<J, Fault> {
        let Some(name) = params.get("name").and_then(J::as_str) else {
            return Err(fault(INVALID_PARAMS, "Invalid params: `name` is the tool's name, a string"));
        };
        let Some(tool) = self.tools.iter().find(|t| t.name == name) else {
            return Err(fault(INVALID_PARAMS, format!("Unknown tool: {}", name)));
        };
        let empty = Map::new();
        let args = match params.get("arguments") {
            None => &empty,
            Some(J::Object(a)) => a,
            Some(_) => return Err(fault(INVALID_PARAMS, "Invalid params: `arguments` is an object, each argument by its name")),
        };
        Ok(match by_name(tool, args) {
            Ok(args) => run(it, tool, args),
            Err(why) => tool_error(why),
        })
    }
}

/// A tool run for a client, as a request is for a visitor: nothing of the call before it (no
/// session, cookie or tenant: on stdio there is no request, and the user who started the door
/// is the caller), and a transaction it left open rolled back.
fn run(it: &mut Interp, tool: &Tool, args: Vec<Option<Value>>) -> J {
    it.tenant = None;
    it.tenant_named = false;
    it.no_such_tenant = false;
    it.set_cookies.clear();
    it.session = None;
    it.session_id = None;
    it.answering = true;
    let r = call_given(it, tool, args, 0);
    it.answering = false;
    it.set_cookies.clear();
    crate::db::left_open(it, &format!("tool `{}`", tool.name));
    match r {
        Ok(Err(why)) => tool_error(why),
        Ok(Ok(v)) | Err(Ctrl::Return(v)) => result_of(tool, v),
        Err(Ctrl::Reply(r)) => replied(&r),
        Err(Ctrl::Error(e)) => {
            eprintln!("quillo: error in tool `{}`: {:#}", tool.name, e);
            tool_error("internal error")
        }
    }
}

/// What a tool gave back, as `tools/call`'s result. An error a builtin made says `internal
/// error`, its text in the log (`server::for_visitor`, as `/_quillo/rpc` hides it); the tool's
/// own `err(v)` is said. A value is its JSON: `structuredContent` and the same JSON as text where
/// the tool says its result's type, text alone where it does not (a text as it is).
fn result_of(tool: &Tool, v: Value) -> J {
    let v = crate::server::for_visitor(v);
    match &v {
        Value::Err(e, _, _) => return tool_error(text_of(e)),
        Value::Response(r) => return replied(r),
        Value::Html(_) => return tool_error(NOT_A_PAGE),
        _ => {}
    }
    let mut out = Vec::new();
    if let Err(why) = v.json_into_within(&mut out, crate::builtins::JSON_MOST, crate::builtins::JSON_DEEPEST) {
        eprintln!("quillo: error in tool `{}`: its result cannot be written as JSON: {}", tool.name, why);
        return tool_error("internal error");
    }
    let text = String::from_utf8(out).unwrap_or_default();
    if tool.output_schema.is_none() {
        let text = match &v {
            Value::Str(s) => s.to_string(),
            _ => text,
        };
        return json!({ "content": [{ "type": "text", "text": text }] });
    }
    let value: J = serde_json::from_str(&text).unwrap_or(J::Null);
    match tool.structured(value) {
        Some(s) => json!({ "content": [{ "type": "text", "text": s.to_string() }], "structuredContent": s }),
        None => json!({ "content": [{ "type": "text", "text": text }] }),
    }
}

/// A `reply`, or a response a tool gave back: a page (a redirect, markup, bytes, a stream) is
/// none of a tool's answers; anything else is an error with its text, `session().user ?? reply
/// 401 "Log in first"` telling the model `Log in first` (A.1).
fn replied(r: &crate::value::Response) -> J {
    use crate::value::Body;
    match &r.body {
        _ if (300..400).contains(&r.status) => tool_error(NOT_A_PAGE),
        Body::Value(Value::Html(_)) | Body::Bytes(_) | Body::Stream { .. } => tool_error(NOT_A_PAGE),
        Body::Value(Value::Nil) => tool_error(format!("{} {}", r.status, crate::httpd::reason(r.status)).trim_end().to_string()),
        Body::Value(v) => tool_error(text_of(&crate::server::for_visitor(v.clone()))),
    }
}

// ---- stdio: `quillo mcp file.qwe` (C.1) ---------------------------------------------------------

/// A call is being answered: a stop waits for its answer.
static BUSY: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
/// A stop was asked for.
static STOP: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
/// The door reads messages: the program's top level has run.
static READING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// The longest message taken, as the HTTP door takes a body.
const MOST: usize = crate::server::MAX_BODY;

/// The process's stdout, kept for MCP's messages alone (stdio: the server MUST NOT write anything
/// else there). `print` goes to stderr, and on Unix descriptor 1 becomes a copy of stderr, so that
/// whatever else writes to stdout (a native plugin, a library) reaches stderr too, and what the
/// program starts inherits that; the messages go out on a copy of the old one, closed in children.
pub fn take_stdout() -> Box<dyn std::io::Write + Send> {
    crate::builtins::PRINT_TO_STDERR.store(true, std::sync::atomic::Ordering::Relaxed);
    #[cfg(unix)]
    {
        use std::os::unix::io::FromRawFd;
        // SAFETY: descriptors 1 and 2 are the process's own; `fd` is a new one, owned by the
        // file made of it.
        unsafe {
            let fd = libc::fcntl(1, libc::F_DUPFD_CLOEXEC, 3);
            if fd >= 0 {
                if libc::dup2(2, 1) >= 0 {
                    return Box::new(std::fs::File::from_raw_fd(fd));
                }
                libc::close(fd);
            }
        }
    }
    Box::new(std::io::stdout())
}

/// SIGTERM or Ctrl-C (`ctrlc`'s handler): before the door reads, and the second time, the process
/// ends at once, as `quillo run`'s does; while it waits for a message, it ends; while a call is
/// answered, it ends once the answer is out, or after `httpd::DRAIN` (C.1).
pub fn stop_asked() {
    use std::sync::atomic::Ordering::SeqCst;
    if STOP.swap(true, SeqCst) || !READING.load(SeqCst) {
        std::process::exit(130);
    }
    if !BUSY.load(SeqCst) {
        std::process::exit(0);
    }
    eprintln!("quillo: stopping once the call in progress is answered (again to stop now)");
    std::thread::spawn(|| {
        std::thread::sleep(crate::httpd::DRAIN + std::time::Duration::from_millis(500));
        eprintln!("quillo: stopping with a call in progress");
        std::process::exit(0);
    });
}

/// One line of stdin, at most `most` bytes kept: None at the end. A line past it is read to its
/// end and given as `Err(())`, so the next one is read whole.
fn read_message(input: &mut dyn std::io::BufRead, most: usize) -> std::io::Result<Option<Result<Vec<u8>, ()>>> {
    let (mut line, mut over, mut any) = (Vec::new(), false, false);
    loop {
        let chunk = match input.fill_buf() {
            Ok(c) => c,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        };
        if chunk.is_empty() {
            break;
        }
        any = true;
        let (take, end) = match chunk.iter().position(|b| *b == b'\n') {
            Some(i) => (i, true),
            None => (chunk.len(), false),
        };
        if !over && line.len() + take > most {
            over = true;
            line = Vec::new();
        }
        if !over {
            line.extend_from_slice(&chunk[..take]);
        }
        input.consume(take + end as usize);
        if end {
            break;
        }
    }
    Ok(any.then_some(if over { Err(()) } else { Ok(line) }))
}

/// Reads newline-delimited JSON-RPC from `input` and answers each request on `out`, a line
/// each, until `input` ends (stdio: the server SHOULD exit when its stdin closes), `out` cannot be
/// written, or a stop is asked for. The exit status.
pub fn serve_stdio(door: &Door, it: &mut Interp, input: &mut dyn std::io::BufRead, out: &mut dyn std::io::Write) -> i32 {
    use std::sync::atomic::Ordering::SeqCst;
    READING.store(true, SeqCst);
    loop {
        let line = match read_message(input, MOST) {
            Ok(Some(line)) => line,
            Ok(None) => return 0,
            Err(e) => {
                eprintln!("quillo: cannot read stdin: {}", e);
                return 1;
            }
        };
        BUSY.store(true, SeqCst);
        let answer = match &line {
            Ok(text) if text.iter().all(u8::is_ascii_whitespace) => None,
            Ok(text) => door.answer(it, text),
            Err(()) => Some(error_message(None, fault(INVALID_REQUEST, format!("Invalid request: a message is at most {} MiB", MOST >> 20)))),
        };
        if let Some(a) = answer {
            let mut text = a.to_string().into_bytes();
            text.push(b'\n');
            if out.write_all(&text).and_then(|_| out.flush()).is_err() {
                // The client is gone (stdio: the server MAY end by closing its output).
                return 0;
            }
        }
        BUSY.store(false, SeqCst);
        if STOP.load(SeqCst) {
            return 0;
        }
    }
}
