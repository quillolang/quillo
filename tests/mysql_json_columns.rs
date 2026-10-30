//! JSON columns as MySQL sends them. MariaDB's JSON is text by another name, so
//! the type only comes from MySQL itself: the server here is a stand-in, speaking
//! just enough of the protocol to hand back one result.

mod support;

use quillo::Interp;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};

fn packet(s: &mut TcpStream, seq: &mut u8, body: &[u8]) {
    let mut p = (body.len() as u32).to_le_bytes()[..3].to_vec();
    p.push(*seq);
    p.extend_from_slice(body);
    s.write_all(&p).unwrap();
    *seq = seq.wrapping_add(1);
}

fn read_packet(s: &mut TcpStream) -> Option<Vec<u8>> {
    let mut head = [0u8; 4];
    s.read_exact(&mut head).ok()?;
    let mut body = vec![0u8; u32::from_le_bytes([head[0], head[1], head[2], 0]) as usize];
    s.read_exact(&mut body).ok()?;
    Some(body)
}

const OK: &[u8] = &[0, 0, 0, 2, 0, 0, 0];
const EOF: &[u8] = &[0xfe, 0, 0, 2, 0];

fn greeting() -> Vec<u8> {
    // PROTOCOL_41, SECURE_CONNECTION, PLUGIN_AUTH and the usual others; no DEPRECATE_EOF.
    let caps: u32 = 0x1 | 0x4 | 0x8 | 0x200 | 0x2000 | 0x8000 | 0x10000 | 0x20000 | 0x40000 | 0x80000;
    let mut g = vec![10];
    g.extend_from_slice(b"8.0.36\0");
    g.extend_from_slice(&1u32.to_le_bytes());
    g.extend_from_slice(b"12345678\0");
    g.extend_from_slice(&(caps as u16).to_le_bytes());
    g.push(45);
    g.extend_from_slice(&2u16.to_le_bytes());
    g.extend_from_slice(&((caps >> 16) as u16).to_le_bytes());
    g.push(21);
    g.extend_from_slice(&[0; 10]);
    g.extend_from_slice(b"123456789012\0");
    g.extend_from_slice(b"mysql_native_password\0");
    g
}

fn text(out: &mut Vec<u8>, s: &str) {
    assert!(s.len() < 251);
    out.push(s.len() as u8);
    out.extend_from_slice(s.as_bytes());
}

/// One JSON column, called `doc`.
fn column() -> Vec<u8> {
    let mut c = Vec::new();
    for s in ["def", "shop", "t", "t", "doc", "doc"] {
        text(&mut c, s);
    }
    c.push(0x0c);
    c.extend_from_slice(&63u16.to_le_bytes());
    c.extend_from_slice(&1000u32.to_le_bytes());
    c.push(0xf5); // MYSQL_TYPE_JSON
    c.extend_from_slice(&0x90u16.to_le_bytes());
    c.extend_from_slice(&[0, 0, 0]);
    c
}

fn answers(mut s: TcpStream, rows: &'static [&'static str]) {
    let mut seq = 0;
    packet(&mut s, &mut seq, &greeting());
    if read_packet(&mut s).is_none() {
        return;
    }
    seq = 2;
    packet(&mut s, &mut seq, OK);
    while let Some(cmd) = read_packet(&mut s) {
        let mut seq = 1;
        match cmd.first() {
            Some(0x01) => return,
            Some(0x19) => {}
            // COM_STMT_PREPARE: one column, no parameters.
            Some(0x16) => {
                packet(&mut s, &mut seq, &[0, 1, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0]);
                packet(&mut s, &mut seq, &column());
                packet(&mut s, &mut seq, EOF);
            }
            // COM_STMT_EXECUTE: the rows, in the binary protocol.
            Some(0x17) => {
                packet(&mut s, &mut seq, &[1]);
                packet(&mut s, &mut seq, &column());
                packet(&mut s, &mut seq, EOF);
                for r in rows {
                    let mut row = vec![0, 0];
                    text(&mut row, r);
                    packet(&mut s, &mut seq, &row);
                }
                packet(&mut s, &mut seq, EOF);
            }
            _ => packet(&mut s, &mut seq, OK),
        }
    }
}

fn stand_in(rows: &'static [&'static str]) -> u16 {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for s in l.incoming().flatten() {
            std::thread::spawn(move || answers(s, rows));
        }
    });
    port
}

fn docs(rows: &'static [&'static str], name: &str) -> String {
    let port = stand_in(rows);
    let d = support::temp_dir(&format!("mysql-json-{}", name));
    let mut it = Interp::new(&d);
    it.load_source(&format!("db.open(\"mysql://root@127.0.0.1:{}/shop\")\nlet out = db.query(\"select doc from t\")\n", port), "app.qwe").unwrap_or_else(|e| panic!("{}", e));
    quillo::value::lookup(&it.globals, "out").unwrap().repr()
}

#[test]
fn a_json_column_comes_back_as_values() {
    assert_eq!(docs(&[r#"{"a": [1, 2]}"#, "7"], "values"), r#"[{ doc: { a: [1, 2] } }, { doc: 7 }]"#);
}

#[test]
fn what_a_json_column_holds_that_is_not_json_comes_back_as_its_text() {
    assert_eq!(docs(&["not json"], "text"), r#"[{ doc: "not json" }]"#);
}
