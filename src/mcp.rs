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
pub const INTERNAL_ERROR: i64 = -32603;

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
    /// The longest message taken, in bytes: `MOST`, or less where a policy's `max_body` says so,
    /// as it holds `quillo run`'s request bodies (found in review).
    pub most: usize,
    /// The door at `/_quillo/mcp`: a tool runs in the request the door made for its caller, its
    /// session, tenant and `request` that one's, where on stdio it runs for no one (FEEDBACK56.md,
    /// C.2 and Q-859; branch feedback56).
    pub over_http: bool,
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

/// A message as the door reads one, as deep as every reader of JSON takes it
/// (`value::json_value`): an object, else the error to answer with. `one` says how many a
/// message carries: JSON-RPC's batches left MCP in 2025-06-18.
fn message_of(text: &[u8], one: &str) -> Result<Map<String, J>, J> {
    match crate::value::json_value(text) {
        Ok(J::Object(m)) => Ok(m),
        Ok(J::Array(_)) => Err(error_message(None, fault(INVALID_REQUEST, format!("Invalid request: {}, not a batch", one)))),
        Ok(_) => Err(error_message(None, fault(INVALID_REQUEST, "Invalid request: a message is a JSON object"))),
        Err(e) => Err(error_message(None, fault(PARSE_ERROR, format!("Parse error: {}", e)))),
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

/// The most a tool's answer is, as the door takes a message in: `MOST` bytes of JSON, and
/// `JSON_NESTING` levels (`value::json_value`). Past them, a client that reads as the door does
/// could not take it, and the answer says so in its place (found in review: one 128 deep went
/// out as a silent `null`, read back by serde's own limit).
#[derive(Debug, PartialEq)]
enum Unsent {
    Large,
    Deep,
    /// It cannot be written as JSON: a function in it, said as `json_into_within` says it.
    Not(String),
}

/// `v`'s JSON, held to what the door takes (`Unsent`); its depth too where `deep`, as an error's
/// payload and a reply's body go out as text alone.
fn held(v: &Value, deep: bool) -> Result<Vec<u8>, Unsent> {
    if let Value::Str(s) = v {
        // Its JSON is longer than it is, by its quotes at least.
        if s.len() + 2 > MOST {
            return Err(Unsent::Large);
        }
    }
    let mut out = Vec::new();
    let written = v.json_into_within(&mut out, MOST, if deep { crate::value::JSON_NESTING } else { crate::builtins::JSON_DEEPEST });
    if out.len() > MOST {
        return Err(Unsent::Large);
    }
    if deep && crate::value::json_depth(&out) > crate::value::JSON_NESTING {
        return Err(Unsent::Deep);
    }
    written.map_err(Unsent::Not)?;
    Ok(out)
}

/// What the model is told in place of an answer it could not take, the reason in the log.
fn unsent(tool: &str, why: Unsent) -> J {
    let (told, logged) = match why {
        Unsent::Large => (format!("the result is too large for the client: more than {} of JSON", size_text(MOST)), format!("its result is more than {} of JSON", size_text(MOST))),
        Unsent::Deep => (format!("the result is too deep for the client: it nests more than {} deep", crate::value::JSON_NESTING), format!("its result nests more than {} deep", crate::value::JSON_NESTING)),
        Unsent::Not(why) => ("internal error".to_string(), format!("its result cannot be written as JSON: {}", why)),
    };
    eprintln!("quillo: error in tool `{}`: {}", tool, logged);
    tool_error(told)
}

/// A size as the door says it: `10 MiB`, `512 KiB`, `1000 bytes`.
fn size_text(n: usize) -> String {
    match n {
        n if n >= 1 << 20 && n % (1 << 20) == 0 => format!("{} MiB", n >> 20),
        n if n >= 1 << 10 && n % (1 << 10) == 0 => format!("{} KiB", n >> 10),
        n => format!("{} bytes", n),
    }
}

/// What an error value or a reply's body says to the model: text as it is, any other value as
/// its JSON; held to `MOST` bytes, as a result is.
fn text_of(tool: &str, v: &Value) -> Result<String, J> {
    match v {
        Value::Str(s) if s.len() <= MOST => Ok(s.to_string()),
        Value::Str(_) => Err(unsent(tool, Unsent::Large)),
        other => held(other, false).map(|b| String::from_utf8(b).unwrap_or_default()).map_err(|why| unsent(tool, why)),
    }
}

impl Door {
    pub fn new(name: impl Into<String>, tools: Vec<Tool>) -> Door {
        Door { tools, name: name.into(), most: MOST, over_http: false }
    }

    /// The answer to one message, as it came: None for a notification and for a client's
    /// response, which are never answered.
    pub fn answer(&self, it: &mut Interp, text: &[u8]) -> Option<J> {
        match message_of(text, "one message a line") {
            Ok(msg) => self.answer_message(it, &msg),
            Err(refused) => Some(refused),
        }
    }

    /// `answer`, of a message read already (the HTTP door reads it to check its headers first).
    pub fn answer_message(&self, it: &mut Interp, msg: &Map<String, J>) -> Option<J> {
        let id = match msg.get("id") {
            // A notification, the client's to send and never answered; `notifications/cancelled`
            // among them, for a call that is answered before the next line is read.
            None if msg.get("method").is_some_and(J::is_string) => return None,
            // A response: the door asks the client nothing, so there is none to wait for.
            None | Some(_) if !msg.contains_key("method") && (msg.contains_key("result") || msg.contains_key("error")) => return None,
            None => return Some(error_message(None, fault(INVALID_REQUEST, "Invalid request: `method` is a string"))),
            // A string or a number, as `RequestId` says (schema.ts), and given back as it came;
            // never null (basic/index).
            Some(id @ (J::String(_) | J::Number(_))) => id,
            Some(_) => return Some(error_message(None, fault(INVALID_REQUEST, "Invalid request: an id is a string or a number"))),
        };
        Some(match self.request(it, msg) {
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
            Ok(args) => run(it, tool, args, self.over_http),
            Err(why) => tool_error(why),
        })
    }
}

/// A tool run for a client, as a request is for a visitor: nothing of the call before it (no
/// session, cookie or tenant: on stdio there is no request, and the user who started the door
/// is the caller), and a transaction it left open rolled back. A panic in it, a bug of Quillo's,
/// is that call's `internal error`, as a request's is a 500, and the door goes on with what calls
/// change put back as it was (`Interp::put_back`); a transaction it was in has let go of what it
/// held, and raised it again for this (found in review).
/// Over HTTP (`over_http`) the request is the caller's, made by `serve_http`: its session, its
/// tenant and its `request` are the tool's, and its cookies go back as the request's do
/// (FEEDBACK56.md, Q-859; branch feedback56).
fn run(it: &mut Interp, tool: &Tool, args: Vec<Option<Value>>, over_http: bool) -> J {
    if !over_http {
        it.tenant = None;
        it.tenant_named = false;
        it.no_such_tenant = false;
        it.set_cookies.clear();
        it.session = None;
        it.session_id = None;
        it.session_ends = None;
        it.answering = true;
    }
    let before = it.entered();
    let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| call_given(it, tool, args, 0)));
    if !over_http {
        it.answering = false;
        it.set_cookies.clear();
    }
    let r = match r {
        Ok(r) => r,
        Err(panic) => {
            it.put_back(before);
            eprintln!("quillo: error in tool `{}`: a panic, a bug of Quillo's: {}", tool.name, panic_text(&*panic));
            return tool_error("internal error");
        }
    };
    crate::db::left_open(it, &format!("tool `{}`", tool.name));
    match r {
        Ok(Err(why)) => tool_error(why),
        Ok(Ok(v)) | Err(Ctrl::Return(v)) => result_of(tool, v),
        Err(Ctrl::Reply(r)) => replied(&tool.name, &r),
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
/// It is held to what the door takes in (`held`): past that, an error that says so, never a part
/// of it.
fn result_of(tool: &Tool, v: Value) -> J {
    let v = crate::server::for_visitor(v);
    match &v {
        Value::Err(e, _, _) => return text_of(&tool.name, e).map_or_else(|e| e, tool_error),
        Value::Response(r) => return replied(&tool.name, r),
        Value::Html(_) => return tool_error(NOT_A_PAGE),
        Value::Str(s) if tool.output_schema.is_none() => {
            return match s.len() <= MOST {
                true => json!({ "content": [{ "type": "text", "text": &**s }] }),
                false => unsent(&tool.name, Unsent::Large),
            }
        }
        _ => {}
    }
    let out = match held(&v, true) {
        Ok(out) => out,
        Err(why) => return unsent(&tool.name, why),
    };
    if tool.output_schema.is_none() {
        return json!({ "content": [{ "type": "text", "text": String::from_utf8(out).unwrap_or_default() }] });
    }
    // Read back as deep as it was written: serde's own limit is 127 levels.
    let value = match crate::value::json_value(&out) {
        Ok(value) => value,
        Err(why) => return unsent(&tool.name, Unsent::Not(why)),
    };
    let text = String::from_utf8(out).unwrap_or_default();
    match tool.structured(value) {
        Some(s) => json!({ "content": [{ "type": "text", "text": s.to_string() }], "structuredContent": s }),
        None => json!({ "content": [{ "type": "text", "text": text }] }),
    }
}

/// A `reply`, or a response a tool gave back: a page (a redirect, markup, bytes, a stream) is
/// none of a tool's answers; anything else is an error with its text, `session().user ?? reply
/// 401 "Log in first"` telling the model `Log in first` (A.1).
fn replied(tool: &str, r: &crate::value::Response) -> J {
    use crate::value::Body;
    match &r.body {
        _ if (300..400).contains(&r.status) => tool_error(NOT_A_PAGE),
        Body::Value(Value::Html(_)) | Body::Bytes(_) | Body::Stream { .. } => tool_error(NOT_A_PAGE),
        Body::Value(Value::Nil) => tool_error(format!("{} {}", r.status, crate::httpd::reason(r.status)).trim_end().to_string()),
        Body::Value(v) => text_of(tool, &crate::server::for_visitor(v.clone())).map_or_else(|e| e, tool_error),
    }
}

/// What a panic said, for the log.
fn panic_text(p: &(dyn std::any::Any + Send)) -> String {
    p.downcast_ref::<&str>().map(|s| s.to_string()).or_else(|| p.downcast_ref::<String>().cloned()).unwrap_or_else(|| "a panic".into())
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
/// end and given as `Err(())`, so the next one is read whole. The door is busy from its first
/// byte: a stop asked for while it is read waits for its answer, where it ended the process
/// between reading it and answering (found in review).
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
        if !any {
            BUSY.store(true, std::sync::atomic::Ordering::SeqCst);
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
/// Each message is held to `--max-compute` or the policy's `max_compute`, as a request is
/// (`httpd::watched`), and one that panics outside a tool, in the door's own code, is answered
/// `-32603` and the next read (found in review).
pub fn serve_stdio(door: &Door, it: &mut Interp, input: &mut dyn std::io::BufRead, out: &mut dyn std::io::Write) -> i32 {
    use std::sync::atomic::Ordering::SeqCst;
    READING.store(true, SeqCst);
    let watch = crate::httpd::compute_limited().then(crate::httpd::watched);
    loop {
        let line = match read_message(input, door.most) {
            Ok(Some(line)) => line,
            Ok(None) => return 0,
            Err(e) => {
                eprintln!("quillo: cannot read stdin: {}", e);
                return 1;
            }
        };
        if let Some(w) = &watch {
            w.begin();
        }
        let answer = match &line {
            Ok(text) if text.iter().all(u8::is_ascii_whitespace) => None,
            Ok(text) => {
                let before = it.entered();
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| door.answer(it, text))).unwrap_or_else(|panic| {
                    it.put_back(before);
                    eprintln!("quillo: error in the MCP door: a panic, a bug of Quillo's: {}", panic_text(&*panic));
                    let id = match crate::value::json_value(text) {
                        Ok(J::Object(m)) => m.get("id").filter(|id| id.is_string() || id.is_number()).cloned(),
                        _ => None,
                    };
                    Some(error_message(id.as_ref(), fault(INTERNAL_ERROR, "Internal error")))
                })
            }
            Err(()) => Some(error_message(None, fault(INVALID_REQUEST, format!("Invalid request: a message is at most {}", size_text(door.most))))),
        };
        if let Some(w) = &watch {
            w.end();
        }
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

// ---- HTTP: `/_quillo/mcp` (FEEDBACK56.md, C.2 to C.4, Q-859, Q-860; branch feedback56) -------
//
// MCP's Streamable HTTP transport of revision 2026-07-28 (basic/transports/streamable-http): each
// message a POST of its own, each request answered with one JSON object (no SSE, which only
// progress would need), a notification with 202, nothing kept between requests. A call comes with
// the program's session cookie, someone signed in to it, or with a token `mcp_token()` made, as
// `Authorization: Bearer` (basic/authorization lets a server use its own authentication; OAuth
// is Part G); with neither it is refused, 401 with `WWW-Authenticate: Bearer` (decided
// 2026-11-08).
//
// Another site's page cannot drive it from a visitor's browser, whose cookie would let it in:
// its `Origin` (or `Sec-Fetch-Site`) is another site's, 403, as `cross_site` reads it for server
// fns; and every call carries `Content-Type: application/json`, `MCP-Protocol-Version` and
// `Mcp-Method`, which such a page can send only after a preflight, and nothing here grants one:
// no `Access-Control-Allow-*` is ever sent.

/// The keys `_meta` carries the version and the client under (basic/index, `_meta`).
const VERSION_KEY: &str = "io.modelcontextprotocol/protocolVersion";
const CLIENT_KEY: &str = "io.modelcontextprotocol/clientInfo";

/// MCP's code for headers that do not match the body, or are missing (streamable-http, Server
/// Validation).
pub const HEADER_MISMATCH: i64 = -32020;

/// Who a call over HTTP is made by.
struct Caller {
    /// What the limit counts by: the user, a token's or the session's, in this place; else the
    /// session, or the developer's token (C.4).
    key: String,
    /// It came with a token: what it sets in a cookie has no browser to go back to.
    token: bool,
}

/// `/_quillo/mcp` (under `--base /shop`, `/shop/_quillo/mcp` to the proxy that takes the prefix
/// off): the program's tools for an AI client over HTTP. None for a program with no tools, or run
/// `--as-service`, whose tools are its app's (Part F): 404, as for any address not there.
pub fn serve_http(it: &mut Interp, req: &crate::server::HttpRequest, path: &str) -> crate::server::HttpResponse {
    use crate::server::{plain, HttpResponse};
    let door = match &it.mcp {
        Some(d) if !it.is_service => d.clone(),
        _ => return plain(404, "not found\n"),
    };
    // Servers MUST validate `Origin`, and answer another's 403 (streamable-http, Security). A
    // client that is no browser sends none, and goes on to who it is.
    if crate::server::cross_site(req) {
        return plain(403, "the MCP door takes no calls from another site's pages\n");
    }
    match req.method.as_str() {
        "POST" => {}
        // What the path takes, as Quillo answers OPTIONS elsewhere, and nothing that lets another
        // site's page send a call: no `Access-Control-Allow-*`.
        "OPTIONS" => return HttpResponse { status: 204, headers: vec![("allow".into(), "POST".into())], body: Vec::new() },
        // GET was 2025-11-25's stream, DELETE its end of a session: 405 (streamable-http,
        // Earlier Streamable HTTP Revisions).
        _ => {
            let mut r = plain(405, "the MCP door takes POST: one JSON-RPC message a request, as MCP 2026-07-28 sends them\n");
            r.headers.push(("allow".into(), "POST".into()));
            return r;
        }
    }
    if crate::server::essence(header(req, "content-type").unwrap_or("")) != "application/json" {
        return plain(415, "send the message as JSON (content-type: application/json)\n");
    }
    // The request a tool runs in: its session, tenant and `request` come from it.
    let qs = req.url.split_once('?').map_or("", |(_, q)| q);
    let mut reqmap = crate::server::build_request(req, path, &Value::map(crate::server::parse_urlencoded(qs)));
    if let Some(id) = &it.request_id {
        reqmap.insert("id".into(), Value::str(&**id));
    }
    it.current_request = Some(Value::map(reqmap));
    let resp = answer_http(it, &door, req);
    it.current_request = None;
    resp
}

fn answer_http(it: &mut Interp, door: &Door, req: &crate::server::HttpRequest) -> crate::server::HttpResponse {
    use crate::server::HttpResponse;
    // Who it is first: a stranger learns nothing of the door, not even what it would refuse.
    let caller = match caller(it, req) {
        Ok(c) => c,
        Err(refused) => return refused,
    };
    let msg = match message_of(&req.body, "one message a request") {
        Ok(m) => m,
        Err(refused) => return json_answer(400, refused),
    };
    let method = msg.get("method").and_then(J::as_str);
    // A notification is taken, 202 and no body; this revision defines none a client sends over
    // HTTP (streamable-http, Sending Messages).
    if !msg.contains_key("id") && method.is_some() {
        return HttpResponse { status: 202, headers: vec![("cache-control".into(), "no-store".into())], body: Vec::new() };
    }
    // A response, with its id or not: the client MUST NOT send one, where stdio lets it go by.
    if !msg.contains_key("method") && (msg.contains_key("result") || msg.contains_key("error")) {
        let id = msg.get("id").filter(|id| id.is_string() || id.is_number());
        return json_answer(400, error_message(id, fault(INVALID_REQUEST, "Invalid request: a client sends requests and notifications, not responses")));
    }
    // `initialize`, a client of 2025-11-25's first message, carries none of the headers: it is
    // told the version to speak (decided 2026-11-08), not that a header is missing.
    if method != Some("initialize") {
        if let Err(f) = mirrored(req, &msg) {
            let id = msg.get("id").filter(|id| id.is_string() || id.is_number());
            return json_answer(400, error_message(id, f));
        }
    }
    // Tool calls are limited (C.4, Q-860), not the lists, which run nothing of the program's.
    if method == Some("tools/call") {
        if let Err(wait) = one_more(&caller.key) {
            let most = MAX_TOOL_CALLS.load(std::sync::atomic::Ordering::Relaxed);
            let mut r = crate::server::plain(429, &format!("too many tool calls: at most {} a minute for each caller; try again in {} s\n", most, wait));
            r.headers.push(("retry-after".into(), wait.to_string()));
            return r;
        }
    }
    // `request.mcp`: who the client says it is, so that a program can say what an assistant
    // wrote. Self-reported (basic/index: for display and logs, not for security).
    let meta = msg.get("params").and_then(J::as_object).and_then(|p| p.get("_meta")).and_then(J::as_object);
    let info = meta.and_then(|m| m.get(CLIENT_KEY)).and_then(J::as_object);
    let said = |k: &str| info.and_then(|i| i.get(k)).and_then(J::as_str).map_or(Value::Nil, Value::str);
    let mut client = crate::value::Map::new();
    client.insert("client".into(), said("name"));
    client.insert("version".into(), said("version"));
    if let Some(Value::Map(m)) = &mut it.current_request {
        Arc::make_mut(m).insert("mcp".into(), Value::map(client));
    }
    let answer = door.answer_message(it, &msg);
    if caller.token {
        it.set_cookies.clear();
    }
    match answer {
        None => HttpResponse { status: 202, headers: vec![("cache-control".into(), "no-store".into())], body: Vec::new() },
        Some(a) => json_answer(status_of(&a), a),
    }
}

/// An answer's HTTP status: 200 for a result, a tool's own error among them; the protocol's
/// errors as the transport says (streamable-http: a method not found 404, the version, the
/// headers and a malformed request 400), and the door's own failure 500.
fn status_of(a: &J) -> u16 {
    match a.get("error").and_then(|e| e.get("code")).and_then(J::as_i64) {
        None => 200,
        Some(METHOD_NOT_FOUND) => 404,
        Some(INTERNAL_ERROR) => 500,
        Some(_) => 400,
    }
}

fn json_answer(status: u16, j: J) -> crate::server::HttpResponse {
    crate::server::HttpResponse {
        status,
        headers: vec![("content-type".into(), "application/json".into()), ("cache-control".into(), "no-store".into())],
        body: j.to_string().into_bytes(),
    }
}

/// A request header's value, the first of its name (names in any case, RFC 9110).
fn header<'a>(req: &'a crate::server::HttpRequest, name: &str) -> Option<&'a str> {
    req.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
}

/// A mirrored header's value: missing, held twice with two values (a proxy could route by one and
/// the door read the other), or holding what no header may, it is a mismatch.
fn given<'a>(req: &'a crate::server::HttpRequest, name: &str) -> Result<&'a str, Fault> {
    let mut values = req.headers.iter().filter(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.trim());
    let Some(v) = values.next() else {
        return Err(fault(HEADER_MISMATCH, format!("Header mismatch: the {} header is missing", name)));
    };
    if values.any(|o| o != v) {
        return Err(fault(HEADER_MISMATCH, format!("Header mismatch: the {} header is given twice, with two values", name)));
    }
    if !v.bytes().all(|b| b == b'\t' || (0x20..0x7f).contains(&b)) {
        return Err(fault(HEADER_MISMATCH, format!("Header mismatch: the {} header holds a character a header may not (send such a value as =?base64?...?=)", name)));
    }
    Ok(v)
}

/// A value in the form a client sends what a header cannot hold, `=?base64?<base64 of UTF-8>?=`,
/// read; any other as it is (streamable-http, Value Encoding). None when it is not base64 text.
fn sentinel(v: &str) -> Option<String> {
    let Some(inner) = v.strip_prefix("=?base64?").and_then(|r| r.strip_suffix("?=")) else { return Some(v.to_string()) };
    // Base64's own alphabet, padded or not: `-` and `_` are another's.
    if inner.contains(['-', '_']) {
        return None;
    }
    let url: String = inner.trim_end_matches('=').chars().map(|c| match c {
        '+' => '-',
        '/' => '_',
        c => c,
    }).collect();
    String::from_utf8(crate::crypto::unb64url(&url)?).ok()
}

/// The headers the transport mirrors from the body, each checked against it (streamable-http,
/// Request Metadata and Server Validation): `MCP-Protocol-Version` as `_meta`'s version,
/// `Mcp-Method` as the method, and on `tools/call` `Mcp-Name` as the tool's name. Header values
/// are compared as they are, in their case. A client of an earlier revision that says its version
/// in the header alone is told the one to speak.
fn mirrored(req: &crate::server::HttpRequest, msg: &Map<String, J>) -> Result<(), Fault> {
    let mismatch = |m: String| Err(fault(HEADER_MISMATCH, format!("Header mismatch: {}", m)));
    let params = msg.get("params").and_then(J::as_object);
    let in_body = params.and_then(|p| p.get("_meta")).and_then(J::as_object).and_then(|m| m.get(VERSION_KEY)).and_then(J::as_str);
    let version = given(req, "MCP-Protocol-Version")?;
    match in_body {
        Some(b) if b != version => return mismatch(format!("MCP-Protocol-Version header value '{}' does not match body value '{}'", version, b)),
        None if version != PROTOCOL => return Err(unsupported(version)),
        _ => {}
    }
    // Without a method the door says what is wrong with the message.
    let Some(method) = msg.get("method").and_then(J::as_str) else { return Ok(()) };
    let m = given(req, "Mcp-Method")?;
    if m != method {
        return mismatch(format!("Mcp-Method header value '{}' does not match body value '{}'", m, method));
    }
    if method == "tools/call" {
        let raw = given(req, "Mcp-Name")?;
        let Some(name) = sentinel(raw) else {
            return mismatch("the Mcp-Name header's =?base64? value is not base64 of UTF-8 text".into());
        };
        // A name that is no string is the door's to refuse, as a malformed call.
        if let Some(b) = params.and_then(|p| p.get("name")).and_then(J::as_str) {
            if name != b {
                return mismatch(format!("Mcp-Name header value '{}' does not match body value '{}'", raw, b));
            }
        }
    }
    Ok(())
}

/// Who the call is made by: a token, when it comes as `Authorization: Bearer` (another scheme, a
/// staging site's Basic say, is no token here, and the cookie is read); else the session the
/// cookie carries, someone signed in to it. Neither: 401, the door closed as middleware fails
/// closed. A token the door cannot look up for its store failing: 503, never let in.
fn caller(it: &mut Interp, req: &crate::server::HttpRequest) -> Result<Caller, crate::server::HttpResponse> {
    let bearer = req.headers.iter().filter(|(k, _)| k.eq_ignore_ascii_case("authorization")).find_map(|(_, v)| {
        let v = v.trim();
        let (scheme, rest) = v.split_once(' ').unwrap_or((v, ""));
        scheme.eq_ignore_ascii_case("bearer").then(|| rest.trim())
    });
    if let Some(token) = bearer {
        return match token_caller(it, token) {
            Ok(Some(c)) => Ok(c),
            Ok(None) => Err(unauthorized(true)),
            Err(why) => {
                crate::httpd::error_line(&format!("the MCP door cannot read its tokens: {}", why));
                Err(crate::server::plain(503, "the MCP door cannot check tokens now\n"))
            }
        };
    }
    match crate::session::signed_in(it) {
        Some((data, owner)) => {
            // The limit counts by the user, as a token's does: a new sign-in is no new minute
            // (C.4, found in review).
            let key = match user_of(&data) {
                Some(u) => format!("user {} {}", crate::session::place(it), u),
                None => format!("session {}", owner),
            };
            it.session = Some(data);
            Ok(Caller { key, token: false })
        }
        None => Err(unauthorized(false)),
    }
}

/// 401, with the challenge RFC 6750 gives: `Bearer` for no credentials, `invalid_token` for a
/// token that is not good (basic/authorization: invalid or expired tokens MUST receive 401).
fn unauthorized(bad_token: bool) -> crate::server::HttpResponse {
    let (challenge, text) = if bad_token {
        ("Bearer error=\"invalid_token\"", "this token is not good here: it was revoked, its session ended, it expired, or it is for another site or tenant\n")
    } else {
        ("Bearer", "the MCP door takes a call from someone signed in: the program's session cookie, or a token mcp_token() made, as Authorization: Bearer <token>\n")
    };
    let mut r = crate::server::plain(401, text);
    r.headers.push(("www-authenticate".into(), challenge.into()));
    r
}

// ---- Who is calling: tokens (Q-859) -------------------------------------------------------------
//
// `mcp_token()`, where someone is signed in, makes a token tied to that session and gives it
// once. Sessions are signed cookies, kept nowhere on the server; a token has to be, to be taken
// back. What is kept is the token signed with the session key (`session::digest`), never the
// token, beside the session it stands for (its data and its hidden id, the owner `end_session`
// ends it by), the place it is good in (the mount point and the tenant, as a session's signature
// binds them), whom it was made for, and when it ends. It is kept where Quillo keeps its own
// tables (`db::system_on`): the program's database, or its system database; a program with
// neither keeps its tokens in memory, so that they end when the server stops, as its sessions do
// without `QUILLO_SECRET`.
//
// Found in review: a row is sealed as a cookie is, everything it decides signed with the session
// key, so that whoever can write the table cannot make one good token another user's, tenant's or
// time's; the table is made by `mcp_token()` alone, and everything else asks whether it is there
// first, of the catalog, so that a logout never depends on it; and rows are matched in Rust, as
// MySQL's and MariaDB's usual collations take `ANA` for `ana` and `zoe` for `zoë`.

const TOKEN_PURPOSE: &str = "quillo mcp token";
/// What a token's row is sealed for.
const ROW_PURPOSE: &str = "quillo mcp token row";
const TOKENS: &str = "_quillo_mcp_tokens";
/// How long a token is good unless `max_age` or the session's own says: a session's 30 days.
const TOKEN_AGE: u64 = 30 * 86_400;
const TOKEN_COLUMNS: &str = "select hash, id, owner, place, who, session, made, expires, seal from _quillo_mcp_tokens";

/// The statement that makes the table: its text compared as bytes on MySQL and MariaDB too.
fn make_tokens(mysql: bool) -> String {
    format!(
        "create table if not exists _quillo_mcp_tokens (hash varchar(64) primary key, id varchar(32) not null, owner text not null, \
         place text not null, who text not null, session text not null, made bigint not null, expires bigint not null, seal varchar(64) not null){}",
        if mysql { " character set utf8mb4 collate utf8mb4_bin" } else { "" }
    )
}

/// A token as it is kept: all but the token.
#[derive(Debug, Clone)]
struct Kept {
    hash: String,
    id: String,
    owner: String,
    place: String,
    who: String,
    session: String,
    made: i64,
    expires: i64,
    seal: String,
}

impl Kept {
    /// Everything the row decides, signed with the session key.
    fn sealed_as(&self) -> String {
        crate::session::digest(ROW_PURPOSE, &json!([self.hash, self.id, self.owner, self.place, self.who, self.session, self.made, self.expires]).to_string())
    }

    /// Kept as it was made: its seal holds.
    fn sound(&self) -> bool {
        crate::crypto::same(self.sealed_as().as_bytes(), self.seal.as_bytes())
    }
}

/// The tokens of a program with no database for Quillo's own tables, by their hash: a request is
/// one look-up, never a copy of them all (second review).
static MEMORY: std::sync::Mutex<Option<HashMap<String, Kept>>> = std::sync::Mutex::new(None);

/// The most tokens kept in memory for one user, and in all (second review).
const MEMORY_PER_USER: usize = 100;
const MEMORY_IN_ALL: usize = 10_000;

fn with_memory<T>(f: impl FnOnce(&mut HashMap<String, Kept>) -> T) -> T {
    let mut m = MEMORY.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    f(m.get_or_insert_with(HashMap::new))
}

fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as i64)
}

/// A statement on Quillo's own tables there: its value, or the database's error.
fn sql(it: &mut Interp, on: Option<usize>, query: bool, args: Vec<Value>) -> Result<Value, String> {
    let r = if query { crate::db::query_on(it, on, args) } else { crate::db::exec_on(it, on, args) };
    match r {
        Ok(Value::Err(e, _, _)) => Err(e.display()),
        Ok(v) => Ok(v),
        Err(Ctrl::Error(e)) => Err(e.msg.clone()),
        Err(_) => Err("the statement was stopped".into()),
    }
}

fn rows(v: &Value) -> Vec<Kept> {
    let Value::List(l) = v else { return Vec::new() };
    l.iter()
        .filter_map(|r| {
            let Value::Map(m) = r else { return None };
            let text = |k: &str| m.get(k).map(|v| v.display()).unwrap_or_default();
            let int = |k: &str| match m.get(k) {
                Some(Value::Int(n)) => *n,
                Some(v) => v.display().parse().unwrap_or(0),
                None => 0,
            };
            Some(Kept {
                hash: text("hash"),
                id: text("id"),
                owner: text("owner"),
                place: text("place"),
                who: text("who"),
                session: text("session"),
                made: int("made"),
                expires: int("expires"),
                seal: text("seal"),
            })
        })
        .collect()
}

/// Where the tokens are, to read or take away.
enum Store {
    Memory,
    /// A database whose table was never made: there are none.
    NoTable,
    /// The database, as `db::exec_on` names it.
    Db(Option<usize>),
}

/// A database the program has that cannot be opened is that error, never memory (second review).
fn kept_on(it: &mut Interp) -> Result<Store, String> {
    match crate::db::system_store(it, false)? {
        crate::db::OwnStore::Nowhere => Ok(Store::Memory),
        crate::db::OwnStore::NotYet => Ok(Store::NoTable),
        crate::db::OwnStore::On(on) => Ok(if crate::db::own_table_there(it, on, TOKENS)? { Store::Db(on) } else { Store::NoTable }),
    }
}

/// Keeps a token's row, sealed, the rows of tokens that ended taken away. The one place the table
/// is made.
fn keep(it: &mut Interp, mut k: Kept) -> Result<(), String> {
    let now = now_ms();
    k.seal = k.sealed_as();
    let on = match crate::db::system_store(it, true)? {
        crate::db::OwnStore::On(on) => Some(on),
        crate::db::OwnStore::Nowhere => None,
        crate::db::OwnStore::NotYet => return Err("the system database could not be made".into()),
    };
    match on {
        None => {
            // Bounded: a program with no database keeps at most so many, a user's and in all.
            with_memory(|m| {
                m.retain(|_, x| x.expires > now);
                let theirs = m.values().filter(|x| x.who == k.who && x.place == k.place).count();
                if theirs >= MEMORY_PER_USER {
                    return Err(format!("this user has {} already, the most kept in memory for one user, as the program has no database: revoke one, or give the program a database (db.open, db.system)", MEMORY_PER_USER));
                }
                if m.len() >= MEMORY_IN_ALL {
                    return Err(format!("{} are kept already, the most kept in memory, as the program has no database: give the program a database (db.open, db.system)", MEMORY_IN_ALL));
                }
                m.insert(k.hash.clone(), k);
                Ok(())
            })?;
            static SAID: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
            if !SAID.swap(true, std::sync::atomic::Ordering::Relaxed) {
                eprintln!("quillo: MCP tokens are kept in memory, as the program has no database: they end when the server stops (db.system(...) keeps them)");
            }
            Ok(())
        }
        Some(on) => {
            if !crate::db::own_table_there(it, on, TOKENS)? {
                let mysql = crate::db::kind_on(it, on);
                crate::db::own_table_make(it, on, TOKENS, &make_tokens(mysql)).map_err(|e| {
                    format!("its table, {}, could not be made ({}): the program's database user needs the right to create a table there, once", TOKENS, e)
                })?;
            }
            sql(it, on, false, vec![Value::str("delete from _quillo_mcp_tokens where expires <= ?"), Value::Int(now)])?;
            let insert = "insert into _quillo_mcp_tokens (hash, id, owner, place, who, session, made, expires, seal) values (?, ?, ?, ?, ?, ?, ?, ?, ?)";
            let args = vec![
                Value::str(insert),
                Value::str(&k.hash),
                Value::str(&k.id),
                Value::str(&k.owner),
                Value::str(&k.place),
                Value::str(&k.who),
                Value::str(&k.session),
                Value::Int(k.made),
                Value::Int(k.expires),
                Value::str(&k.seal),
            ];
            sql(it, on, false, args).map(|_| ())
        }
    }
}

/// The token of `hash`, sound and kept here; None where there is none, or no table of them.
fn kept_by_hash(it: &mut Interp, hash: &str) -> Result<Option<Kept>, String> {
    let found: Vec<Kept> = match kept_on(it)? {
        Store::Memory => with_memory(|m| m.get(hash).cloned()).into_iter().collect(),
        Store::NoTable => return Ok(None),
        Store::Db(on) => rows(&sql(it, on, true, vec![Value::str(format!("{} where hash = ?", TOKEN_COLUMNS)), Value::str(hash)])?),
    };
    Ok(found.into_iter().find(|k| crate::crypto::same(k.hash.as_bytes(), hash.as_bytes()) && k.sound()))
}

/// Which tokens a call names, in this place, still good: a session's (made in it for the user it
/// is signed in as, or made for that user, so that they are found after its cookie has gone or
/// the user signs in again), `{ user }`'s, or (`end_session`) those made in it for its user alone.
/// A session keeps its id from no user to a user (Q-887), so its id alone is not who it is: Bob,
/// signing in on the browser Alice left without `end_session`, has her session's id, and her
/// tokens are not his (second review).
enum Whose {
    Session { owner: Option<String>, user: Option<String> },
    User(String),
    Made { owner: String, user: Option<String> },
}

impl Whose {
    fn names(&self, k: &Kept) -> bool {
        let made_in = |owner: &str, user: &Option<String>| owner == k.owner && user.as_deref() == Some(k.who.as_str());
        match self {
            Whose::Session { owner, user } => owner.as_deref().is_some_and(|o| made_in(o, user)) || user.as_deref() == Some(k.who.as_str()),
            Whose::User(u) => *u == k.who,
            Whose::Made { owner, user } => made_in(owner, user),
        }
    }

    /// What a database is asked for first, compared exactly afterwards (`names`).
    fn prefilter(&self) -> (&'static str, Vec<&str>) {
        match self {
            Whose::Session { owner, user } => ("(owner = ? or who = ?)", vec![owner.as_deref().unwrap_or(""), user.as_deref().unwrap_or("")]),
            Whose::User(u) => ("who = ?", vec![u.as_str()]),
            Whose::Made { owner, .. } => ("owner = ?", vec![owner.as_str()]),
        }
    }
}

/// The tokens `whose` names in `place`, still good, oldest first: compared exactly here, whatever
/// the database's collation took for the same. `sound`: only those whose seal holds, for using and
/// listing; taking away takes those sealed under another key too, so that a token a logout or a
/// revoke ended stays ended when the key is put back (second review).
fn kept_of(it: &mut Interp, whose: &Whose, place: &str, sound: bool) -> Result<Vec<Kept>, String> {
    let now = now_ms();
    let fits = |k: &Kept| k.place == place && k.expires > now && whose.names(k) && (!sound || k.sound());
    let mut found: Vec<Kept> = match kept_on(it)? {
        Store::Memory => with_memory(|m| m.values().filter(|k| fits(k)).cloned().collect()),
        Store::NoTable => return Ok(Vec::new()),
        Store::Db(on) => {
            let (cond, keys) = whose.prefilter();
            let mut args = vec![Value::str(format!("{} where {} and place = ? and expires > ?", TOKEN_COLUMNS, cond))];
            args.extend(keys.into_iter().map(Value::str));
            args.push(Value::str(place));
            args.push(Value::Int(now));
            rows(&sql(it, on, true, args)?).into_iter().filter(|k| fits(k)).collect()
        }
    };
    found.sort_by(|a, b| (a.made, &a.id).cmp(&(b.made, &b.id)));
    Ok(found)
}

/// Takes away the tokens `whose` names in `place`, all of them or the one of `id`: how many.
fn take_away(it: &mut Interp, whose: &Whose, place: &str, id: Option<&str>) -> Result<i64, String> {
    let doomed: Vec<String> = kept_of(it, whose, place, false)?.into_iter().filter(|k| id.map_or(true, |id| k.id == id)).map(|k| k.hash).collect();
    if doomed.is_empty() {
        return Ok(0);
    }
    match kept_on(it)? {
        Store::Memory => with_memory(|m| doomed.iter().for_each(|h| drop(m.remove(h)))),
        Store::NoTable => return Ok(0),
        Store::Db(on) => {
            for h in &doomed {
                sql(it, on, false, vec![Value::str("delete from _quillo_mcp_tokens where hash = ?"), Value::str(h)])?;
            }
        }
    }
    Ok(doomed.len() as i64)
}

/// `end_session()`'s part: the tokens made in each of `sessions` (the one the request came with,
/// and the one it has now: its owner and its user) for its user end in this place.
pub(crate) fn end_tokens(it: &mut Interp, sessions: &[(String, Option<Value>)]) -> Result<(), String> {
    if sessions.is_empty() {
        return Ok(());
    }
    let place = crate::session::place(it);
    for (owner, user) in sessions {
        let user = user.as_ref().map(|u| crate::interp::island_json(u).to_string());
        take_away(it, &Whose::Made { owner: owner.clone(), user }, &place, None)?;
    }
    Ok(())
}

/// The caller a token names, the session it stands for made the call's; None when it is no
/// token kept here, its row is not as it was made, it ended, or it is for another place. The
/// limit counts by its user, as a cookie caller's (C.4).
fn token_caller(it: &mut Interp, token: &str) -> Result<Option<Caller>, String> {
    let hash = crate::session::digest(TOKEN_PURPOSE, token);
    if DEV_HASH.get().is_some_and(|h| crate::crypto::same(h.as_bytes(), hash.as_bytes())) {
        // The developer's, under `--watch`: no session, the program's own checks as for anyone.
        crate::session::take_on(it, Value::map(crate::value::Map::new()), "");
        return Ok(Some(Caller { key: "token dev".into(), token: true }));
    }
    let Some(k) = kept_by_hash(it, &hash)? else { return Ok(None) };
    let place = crate::session::place(it);
    if k.expires <= now_ms() || k.place != place {
        return Ok(None);
    }
    let data = crate::value::json_value(k.session.as_bytes()).map(|j| crate::interp::from_island_json(&j)).unwrap_or_else(|_| Value::map(crate::value::Map::new()));
    crate::session::take_on(it, data, &k.owner);
    Ok(Some(Caller { key: format!("user {} {}", place, k.who), token: true }))
}

pub const BUILTINS: &[(&str, crate::value::BuiltinFn)] = &[("mcp_token", mcp_token), ("mcp_tokens", mcp_tokens), ("revoke_mcp_token", revoke_mcp_token)];

/// A builtin's options: none, or a map of the keys it takes.
fn options_of<'a>(v: Option<&'a Value>, f: &str, takes: &[&str], line: u32) -> Result<Option<&'a crate::value::Map>, Ctrl> {
    match v {
        None | Some(Value::Nil) => Ok(None),
        Some(Value::Map(m)) => match m.keys().find(|k| !takes.contains(&&***k)) {
            Some(k) => crate::interp::fail(line, format!("`{}` has no option `{}`: it takes {}", f, &**k, takes.iter().map(|t| format!("`{}`", t)).collect::<Vec<_>>().join(" and "))),
            None => Ok(Some(m)),
        },
        Some(v) => crate::interp::fail(line, format!("`{}` expects options as a map, got {}", f, v.type_name())),
    }
}

/// The session's user, as a token's `who` holds one: None when it has none.
fn user_of(session: &Value) -> Option<String> {
    match session {
        Value::Map(m) => m.get("user").filter(|u| !matches!(u, Value::Nil)).map(|u| crate::interp::island_json(u).to_string()),
        _ => None,
    }
}

/// Which tokens `mcp_tokens` and `revoke_mcp_token` name: `{ user }`'s, or the session's (by its
/// owner or its user); None when there is neither.
fn whose(it: &mut Interp, o: Option<&crate::value::Map>) -> Option<Whose> {
    if let Some(u) = o.and_then(|m| m.get("user")).filter(|u| !matches!(u, Value::Nil)) {
        return Some(Whose::User(crate::interp::island_json(u).to_string()));
    }
    let owner = crate::session::current_owner(it);
    let user = user_of(&crate::session::current(it));
    (owner.is_some() || user.is_some()).then_some(Whose::Session { owner, user })
}

/// Whether the request being answered is a call at the MCP door.
fn at_the_door(it: &mut Interp) -> bool {
    matches!(it.request_value(), Some(Value::Map(m)) if matches!(m.get("mcp"), Some(Value::Map(_))))
}

/// `mcp_token({ user, max_age })`: a token for the door, tied to the session of the request it is
/// made in, given once. Someone is signed in: `session().user`, or `user` where the program names
/// its caller otherwise. Good for `max_age`; else for as long as the session, where the program
/// gave the session a `max_age`; else 30 days; and until `end_session()` or `revoke_mcp_token`.
fn mcp_token(it: &mut Interp, args: Vec<Value>, line: u32) -> Result<Value, Ctrl> {
    use crate::interp::fail;
    let o = options_of(args.first(), "mcp_token", &["user", "max_age"], line)?;
    if !it.answering || it.request_value().is_none() {
        return fail(line, "`mcp_token` is made while a request is answered, where someone is signed in: the token stands for that session");
    }
    if it.mcp.is_none() {
        return fail(line, "`mcp_token`: this program has no tools (`fn mcp name(...)`), so there is no door to make a token for");
    }
    if at_the_door(it) {
        return fail(line, "`mcp_token` is not for a tool: a token is made where the user is signed in, on a page of the program's, and pasted into the AI client");
    }
    let session = crate::session::current(it);
    let user = match o.and_then(|m| m.get("user")) {
        Some(u) => u.clone(),
        None => match &session {
            Value::Map(m) => m.get("user").cloned().unwrap_or(Value::Nil),
            _ => Value::Nil,
        },
    };
    if matches!(user, Value::Nil) {
        return fail(line, "`mcp_token`: no one is signed in (`session().user` is null): sign in first, or say who it is for with `{ user: ... }`");
    }
    let Some(owner) = crate::session::current_owner(it) else {
        return fail(line, "`mcp_token`: there is no session to tie the token to: `set_session` first");
    };
    let made = now_ms();
    let age_ms = match o.and_then(|m| m.get("max_age")).filter(|v| !matches!(v, Value::Nil)) {
        Some(v) => match crate::builtins::duration_seconds(v) {
            Some(n) if n > 0 => n.min(366 * 86_400) as i64 * 1000,
            _ => return fail(line, format!("`mcp_token`'s max_age is seconds or a duration like \"30d\", got {}", v.repr())),
        },
        // As long as the session has left, where the program said how long it lasts.
        None => match crate::session::given_life(it) {
            Some(ends) => (ends as i64 * 1000).saturating_sub(made).clamp(1000, 366 * 86_400 * 1000),
            None => TOKEN_AGE as i64 * 1000,
        },
    };
    let token = format!("qmcp_{}", crate::crypto::b64url(&crate::crypto::secret_bytes(32)));
    let k = Kept {
        hash: crate::session::digest(TOKEN_PURPOSE, &token),
        id: crate::service::new_id(16),
        owner,
        place: crate::session::place(it),
        who: crate::interp::island_json(&user).to_string(),
        session: crate::interp::island_json(&session).to_string(),
        made,
        expires: made.saturating_add(age_ms),
        seal: String::new(),
    };
    if let Err(why) = keep(it, k) {
        return fail(line, format!("`mcp_token`: the token could not be kept: {}", why));
    }
    Ok(Value::str(token))
}

/// `mcp_tokens({ user })`: the session's tokens (made in it, or for the user it is signed in as),
/// or all made for `user`, still good, oldest first: `{ id, user, made, expires }` each, the times
/// in milliseconds as `now()` gives them. Never the token itself, which only its holder has.
fn mcp_tokens(it: &mut Interp, args: Vec<Value>, line: u32) -> Result<Value, Ctrl> {
    let o = options_of(args.first(), "mcp_tokens", &["user"], line)?;
    let Some(w) = whose(it, o) else { return Ok(Value::list(Vec::new())) };
    let place = crate::session::place(it);
    let found = match kept_of(it, &w, &place, true) {
        Ok(f) => f,
        Err(why) => return crate::interp::fail(line, format!("`mcp_tokens`: the tokens could not be read: {}", why)),
    };
    Ok(Value::list(
        found
            .into_iter()
            .map(|k| {
                let mut m = crate::value::Map::new();
                m.insert("id".into(), Value::str(&k.id));
                m.insert("user".into(), crate::value::json_value(k.who.as_bytes()).map(|j| crate::interp::from_island_json(&j)).unwrap_or(Value::Nil));
                m.insert("made".into(), Value::Int(k.made));
                m.insert("expires".into(), Value::Int(k.expires));
                Value::map(m)
            })
            .collect(),
    ))
}

/// `revoke_mcp_token(id, { user })`: the token of that id among the session's (as `mcp_tokens()`
/// finds them) or `user`'s ends: whether there was one.
fn revoke_mcp_token(it: &mut Interp, args: Vec<Value>, line: u32) -> Result<Value, Ctrl> {
    let id = match args.first() {
        Some(Value::Str(s)) => s.to_string(),
        other => return crate::interp::fail(line, format!("`revoke_mcp_token` takes a token's id, as `mcp_tokens()` gives it, got {}", other.map_or("nothing", |v| v.type_name()))),
    };
    let o = options_of(args.get(1), "revoke_mcp_token", &["user"], line)?;
    let Some(w) = whose(it, o) else { return Ok(Value::Bool(false)) };
    let place = crate::session::place(it);
    match take_away(it, &w, &place, Some(&id)) {
        Ok(n) => Ok(Value::Bool(n > 0)),
        Err(why) => crate::interp::fail(line, format!("`revoke_mcp_token`: the token could not be taken away: {}", why)),
    }
}

// ---- Limits (C.4, Q-860) ------------------------------------------------------------------------

/// Tool calls each caller (a user, a token, a session) may make a minute at the door, as the spec
/// says servers MUST rate limit tool calls (server/tools, Security): `--max-tool-calls`, or less
/// where a policy's `max_tool_calls` says.
pub static MAX_TOOL_CALLS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(60);

/// The times of each caller's calls in the last minute, oldest first, and when callers not heard
/// from in a minute were last let go.
struct Calls {
    by: HashMap<String, std::collections::VecDeque<std::time::Instant>>,
    swept: std::time::Instant,
}

static CALLS: std::sync::Mutex<Option<Calls>> = std::sync::Mutex::new(None);

/// One more call by `key` now, or how many seconds until one leaves the minute (`Retry-After`).
/// A call refused is not counted.
fn one_more(key: &str) -> Result<(), u64> {
    const MINUTE: std::time::Duration = std::time::Duration::from_secs(60);
    let most = MAX_TOOL_CALLS.load(std::sync::atomic::Ordering::Relaxed).max(1);
    let now = std::time::Instant::now();
    let mut all = CALLS.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let calls = all.get_or_insert_with(|| Calls { by: HashMap::new(), swept: now });
    // Callers not heard from in a minute are let go, at most once a minute, not at each call once
    // there are many (found in review).
    if now.duration_since(calls.swept) >= MINUTE {
        calls.by.retain(|_, t| t.back().is_some_and(|b| now.duration_since(*b) < MINUTE));
        calls.swept = now;
    }
    let times = calls.by.entry(key.to_string()).or_default();
    while times.front().is_some_and(|t| now.duration_since(*t) >= MINUTE) {
        times.pop_front();
    }
    if times.len() >= most {
        let left = MINUTE.saturating_sub(times.front().map_or(MINUTE, |t| now.duration_since(*t)));
        return Err(left.as_millis().div_ceil(1000).max(1) as u64);
    }
    times.push_back(now);
    Ok(())
}

// ---- The program's door, and the developer's token under `--watch` ------------------------------

/// Under `--watch` on 127.0.0.1: the door's address, once the server listens, and the token
/// printed for it, as Jupyter prints one, so a developer connects their editor's AI without a
/// route of their own first (C.3). Only its hash is kept, as a token's is.
static DEV_URL: std::sync::OnceLock<String> = std::sync::OnceLock::new();
static DEV_HASH: std::sync::OnceLock<String> = std::sync::OnceLock::new();

/// The server listens at `url` (http://127.0.0.1:PORT) under `--watch`: the door's token is said
/// once the program has tools, now (`it`, the program as it loaded) or after a reload, a first
/// load that failed among them. The address is where this server answers, `/_quillo/mcp` under
/// `--base` too, whose proxy takes the base off (found in review).
pub fn dev_door(url: String, it: Option<&Interp>) {
    let _ = DEV_URL.set(url);
    if let Some(it) = it {
        say_dev_door(it);
    }
}

fn say_dev_door(it: &Interp) {
    let (Some(url), Some(door)) = (DEV_URL.get(), it.mcp.as_ref()) else { return };
    if door.tools.is_empty() || DEV_HASH.get().is_some() {
        return;
    }
    let token = format!("qmcp_{}", crate::crypto::b64url(&crate::crypto::secret_bytes(32)));
    if DEV_HASH.set(crate::session::digest(TOKEN_PURPOSE, &token)).is_err() {
        return;
    }
    let at = format!("{}/_quillo/mcp", url);
    eprintln!("quillo: MCP door for AI clients at {}, with the header Authorization: Bearer {} (good while this runs)", at, token);
    eprintln!("quillo:   claude mcp add --transport http {} {} --header \"Authorization: Bearer {}\"", door.name, at, token);
}

/// The program's door over HTTP, `it.mcp`: its tools as the checker read them (`tools`), or,
/// where the program was not checked as it loaded (`--no-check`, `--split`'s app checked before),
/// read now when it declares any. None for a program with no tools, or one run as a service.
pub fn open_door(it: &mut Interp, path: &Path, tools: Option<Vec<Tool>>) {
    it.mcp = None;
    if it.is_service {
        return;
    }
    let declares = || it.program.iter().any(|stmts| stmts.iter().any(|s| matches!(s, crate::ast::Stmt::Fn(d) if d.mcp)));
    let tools = match tools {
        Some(t) => t,
        None if declares() => match of_source(path) {
            Ok(t) => t,
            Err(e) => {
                eprintln!("quillo: the MCP door stays closed: the program's tools could not be read:\n{}", e);
                return;
            }
        },
        None => return,
    };
    if tools.is_empty() {
        return;
    }
    let name = crate::service::served_as().filter(|n| !n.is_empty()).map(str::to_string).unwrap_or_else(|| path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default());
    let mut door = Door::new(name, tools);
    door.over_http = true;
    it.mcp = Some(Arc::new(door));
    say_dev_door(it);
}
