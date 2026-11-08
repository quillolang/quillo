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
    let f = match crate::value::lookup(&it.globals, tool.name.as_str()) {
        Some(f @ Value::Func(_)) => f,
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
        let made = crate::server::make_args(it, &def, args.take().unwrap_or_default());
        if let Some((allowed, who)) = narrowed {
            it.allowed = allowed;
            it.restrictor = who;
        }
        match made {
            Ok(Ok(made)) => it.call(&f, made, line),
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
