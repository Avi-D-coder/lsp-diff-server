mod chars_diff;
use chars_diff::{Changes, Incremental, RopeSlice};
mod rope_diff;
use rope_diff::Full;

use std::collections::HashMap;
use std::env;
use std::io::{stdin, BufRead, Read, Stdin, Write};
use std::process::{ChildStdin, Command, Stdio};
use std::str;
use std::thread;
use std::time::Duration;

use lazy_static::lazy_static;
use lsp_types::*;
use ropey::Rope;
use serde::{de::IgnoredAny, Deserialize, Serialize};

lazy_static! {
    static ref STDIN: Stdin = stdin();
}

fn main() {
    let server = env::args()
        .nth(1)
        .expect("Provide lsp command as first argument.");

    let server = Command::new(&server)
        .args(env::args().skip(2))
        .stdin(Stdio::piped())
        .spawn()
        .unwrap_or_else(|_| panic!("Unable to start server with command: '{}'", server));
    let mut server_stdin = server.stdin.expect("server stdin failed");

    let mut url_text: HashMap<Url, Rope> = HashMap::with_capacity(20);

    let open = |DidOpenTextDocumentParams { text_document }, url_text: &mut HashMap<Url, Rope>| {
        url_text.insert(text_document.uri, Rope::from(text_document.text));
    };

    let close = |DidCloseTextDocumentParams { text_document },
                 url_text: &mut HashMap<Url, Rope>| {
        url_text.remove(&text_document.uri);
    };

    let change = |DidChangeTextDocumentParams {
                      text_document,
                      content_changes,
                  },
                  server_stdin: &mut ChildStdin,
                  url_text: &mut HashMap<Url, Rope>| {
        let rope = url_text.get_mut(&text_document.uri).unwrap_or_else(|| {
            panic!(
                "Error: Change to unopened text_document\n {}",
                text_document.uri
            )
        });
        let content_changes = content_changes
            .into_iter()
            .flat_map(|change| match change.range {
                None => {
                    // Slice is for future compatibility
                    let ch = Full::diff(rope.slice(..), change.text.as_str());
                    for c in &ch {
                        // TODO Handle Unicode consistently.
                        let Range { start, end } = c.range.unwrap();
                        let start_offset =
                            rope.line_to_char(start.line as usize) + start.character as usize;

                        if start < end {
                            let end_offset =
                                rope.line_to_char(end.line as usize) + end.character as usize;
                            rope.remove(start_offset..end_offset);
                        }
                        if !change.text.is_empty() {
                            rope.insert(start_offset, change.text.as_str());
                        }
                    }
                    ch
                }
                Some(range) => {
                    if cfg!(debug_assertions) {
                        let mut rope = rope.clone();
                        let mut rope2 = rope.clone();
                        dbg!(&change);
                        let ch = with_change(change.text.as_str(), range, &mut rope, char_diff);

                        for change in ch {
                            dbg!(&change);
                            with_change(
                                change.text.as_str(),
                                change.range.unwrap(),
                                &mut rope2,
                                |_, _, _, _, _| (),
                            );
                        }

                        debug_assert_eq!(String::from(rope), String::from(rope2));
                    };

                    with_change(change.text.as_str(), range, rope, char_diff)
                }
            })
            .collect();

        let did_change = serde_json::to_string(&NotiS::new(Change(DidChangeTextDocumentParams {
            text_document,
            content_changes,
        })))
        .unwrap();

        write!(server_stdin, "Content-Length: {}\r\n\r\n", did_change.len()).unwrap();
        server_stdin.write_all(did_change.as_bytes()).unwrap();
    };

    handle_rpc_msgs(&mut server_stdin, &mut url_text, change, open, close)
}

fn handle_rpc_msgs(
    server_stdin: &mut ChildStdin,
    url_text: &mut HashMap<Url, Rope>,
    mut change: impl FnMut(DidChangeTextDocumentParams, &mut ChildStdin, &mut HashMap<Url, Rope>),
    open: fn(DidOpenTextDocumentParams, &mut HashMap<Url, Rope>),
    close: fn(DidCloseTextDocumentParams, &mut HashMap<Url, Rope>),
) {
    let mut stdin = STDIN.lock();

    let mut msg_spill = vec![0; 10_000];

    loop {
        let buf = stdin.fill_buf().unwrap();
        let buf = unsafe { str::from_utf8_unchecked(buf) };

        if buf.is_empty() {
            thread::sleep(Duration::from_millis(40));
            continue;
        }

        let (content_len, end_ptr) =
            buf.lines()
                .take_while(|l| !l.is_empty())
                .fold((0, 0), |(con_len, _), l| {
                    (
                        if l.starts_with("Content-Length: ") {
                            (&l[16..]).parse().unwrap()
                        } else {
                            con_len
                        },
                        l.as_ptr() as usize + l.len(),
                    )
                });

        // Headers are terminated with \r\n\r\n
        let header_end = (end_ptr + 4) - buf.as_ptr() as usize;

        if buf.len() >= header_end + content_len {
            // We have the whole message.
            let consume = header_end + content_len;
            let mut send = || server_stdin.write_all(&buf.as_bytes()[..consume]).unwrap();
            match serde_json::from_str(&buf[header_end..consume]) {
                Ok(Change(c)) => change(c, server_stdin, url_text),
                Ok(Open(o)) => {
                    send();
                    open(o, url_text);
                }
                Ok(Close(c)) => {
                    send();
                    close(c, url_text);
                }
                _ => send(),
            }
            stdin.consume(consume);
        } else {
            let msg = &mut msg_spill[..header_end + content_len];
            stdin.read_exact(msg).unwrap();

            // duplicated match is needed to convince borrow checker.
            let mut send = || server_stdin.write_all(msg).unwrap();
            match serde_json::from_slice(&msg[header_end..]) {
                Ok(Change(c)) => change(c, server_stdin, url_text),
                Ok(Open(o)) => {
                    send();
                    open(o, url_text);
                }
                Ok(Close(c)) => {
                    send();
                    close(c, url_text);
                }
                _ => send(),
            }
        };
    }
}

fn with_change<R: std::fmt::Debug>(
    change_text: &str,
    range: Range,
    rope: &mut Rope,
    with: fn(&mut Rope, Range, &str, usize, usize) -> R,
) -> R {
    let start_offset =
        rope.line_to_char(range.start.line as usize) + range.start.character as usize;
    let end_offset = rope.line_to_char(range.end.line as usize) + range.end.character as usize;
    let ret = with(rope, range, change_text, start_offset, end_offset);
    // TODO Handle Unicode consistently.
    dbg!("\n\n\nChange\n");
    if start_offset < end_offset {
        dbg!("START_OFFSET < END_OFFSET");
        dbg!(&rope.line(dbg!(range.start.line) as usize));
        rope.remove(start_offset..end_offset);
        dbg!(&rope.line(range.start.line as usize));
    }
    if !change_text.is_empty() {
        dbg!("CHANGE_TEXT NOT EMPTY");
        dbg!(range.start.line);
        dbg!(&rope.line(range.start.line as usize));
        rope.insert(start_offset, change_text);
        dbg!(&rope.line(range.start.line as usize));
    };

    dbg!("END\n");
    ret
}

fn char_diff(
    rope: &mut Rope,
    range: Range,
    change_text: &str,
    start_offset: usize,
    end_offset: usize,
) -> Changes {
    let old_slice = RopeSlice {
        slice: rope.slice(start_offset..end_offset),
        absolute_pos: ropey::Position {
            line: rope.char_to_line(start_offset),
            character: dbg!(range.start.character) as usize,
        },
    };

    Incremental::diff(old_slice, change_text)
}

#[derive(Deserialize, Serialize, Debug)]
#[serde(tag = "method", content = "params")]
enum Did {
    #[serde(rename = "textDocument/didChange")]
    Change(DidChangeTextDocumentParams),
    #[serde(rename = "textDocument/didOpen")]
    Open(DidOpenTextDocumentParams),
    #[serde(rename = "textDocument/didClose")]
    Close(DidCloseTextDocumentParams),
}
use Did::*;

#[derive(Deserialize, Debug)]
struct Noti {
    jsonrpc: IgnoredAny,
    #[serde(flatten)]
    params: Did,
}

#[derive(Deserialize, Serialize, Debug)]
struct NotiS {
    jsonrpc: &'static str,
    #[serde(flatten)]
    params: Did,
}

impl NotiS {
    fn new(params: Did) -> Self {
        NotiS {
            jsonrpc: "2.0",
            params,
        }
    }
}

#[test]
fn parse_test() {
    let line = r#"{"jsonrpc":"2.0","method":"textDocument/didChange","params":{"textDocument":{"version":2,"uri":"file:///home/host/haskell-ide-engine/src/Haskell/Ide/Engine/Channel.hs"},"contentChanges":[{"range":{"start":{"line":25,"character":0},"end":{"line":25,"character":1}},"rangeLength":1,"text":"l"}]}}"#;
    let _: Noti = serde_json::from_str(line).unwrap();
}
