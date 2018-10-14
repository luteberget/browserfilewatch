extern crate ws;
extern crate webbrowser;
extern crate notify;
extern crate itertools;
#[macro_use]
extern crate serde_json;
#[macro_use]
extern crate structopt;

use std::path::{Path, PathBuf};
use structopt::StructOpt;
use itertools::Itertools;
use std::sync::Arc;
use std::sync::Mutex;

#[derive(StructOpt, Debug)]
#[structopt()]
/// Browser-vis file watcher -- update D3.js data from local file.
struct Opt {
    /// Port number to bind to HTTP server.
    #[structopt(short = "p", long = "port", default_value = "3012")]
    port: u16,

    /// Verbose output
    #[structopt(short = "v", long = "verbose")]
    verbose: bool,

    /// File to watch
    #[structopt(name = "FILE", parse(from_os_str))]
    file: PathBuf,
}

struct ServerThread { }

static INDEX_HTML: &'static [u8] = include_bytes!("index.html");
static D3_JS: &'static [u8] = include_bytes!("d3/d3.js");

impl ws::Handler for ServerThread {
    fn on_message(&mut self, _msg: ws::Message) -> ws::Result<()> { Ok(()) }
    fn on_request(&mut self, req :&ws::Request) -> ws::Result<ws::Response> {
        match req.resource() {
            "/ws" => ws::Response::from_request(req),
            "/" => Ok(ws::Response::new(200, "OK", INDEX_HTML.to_vec())),
            "/d3.js" => Ok(ws::Response::new(200, "OK", D3_JS.to_vec())),
            _ => Ok(ws::Response::new(404, "Not Found", b"404 - Not Found".to_vec())),
        }
    }
}
fn update(file :&Path, buf :&mut String) -> Result<(),()> {
    use std::fs::File;
    use std::io::prelude::*;
    let mut f = File::open(file).expect("file not found");
    let mut s = String::new();
    f.read_to_string(&mut s).unwrap();

    let mut json = json!([]);
    for (line_no,line) in s.lines().enumerate() {
        if let Some((a,b)) = line.split_whitespace().next_tuple() {
            if let Ok(f) = b.parse::<f64>() {
                json.as_array_mut().unwrap().push(json!( { "letter": a, "frequency": f} ));
            } else {
                eprintln!("Warning: Failed to parse number on line {}", line_no);
            }
        } else {
            eprintln!("Warning: Failed to parse two columns on line {}", line_no);
        }
    }

    *buf = serde_json::to_string(&json).unwrap();
    Ok(())
}

fn main() {
    let opt = Opt::from_args();
    let my_addr = format!("localhost:{}",opt.port);
    let contents = Arc::new(Mutex::new(String::new()));
    let contents3 = contents.clone();
    eprintln!("Info: Loading data from file.");
    update(&opt.file, &mut *(contents.lock().unwrap())).unwrap();

    let http = ws::WebSocket::new(move |out:ws::Sender| {
        let stuff = contents3.lock().unwrap();
        eprintln!("Info: New connection ({:?}), sending current data.", out.connection_id());
        out.send((*stuff).clone()).unwrap();
        ServerThread { }
    }).unwrap();
    let broadcaster = http.broadcaster();

    use std::{thread,time};
    thread::spawn(move || {
        use std::sync::mpsc::channel;
        use notify::Watcher;
        let (watcher_tx,watcher_rx) = channel();
        let mut watcher = notify::watcher(watcher_tx, time::Duration::from_millis(100)).unwrap();

        use std::fs;
        let canonical = fs::canonicalize(&opt.file).unwrap();
        let path = canonical.parent().unwrap();
        watcher.watch(path, notify::RecursiveMode::Recursive).unwrap();

        loop {
            match watcher_rx.recv() {
                Ok(event) => {
                    match event {
                        notify::DebouncedEvent::Create(x) | 
                        notify::DebouncedEvent::Write(x) |
                        notify::DebouncedEvent::Rename(_,x) => {
                            if x == canonical {
                                eprintln!("Info: File changed event");
                                thread::sleep(time::Duration::from_millis(50));
                                let mut buf = contents.lock().unwrap();
                                eprintln!("Info: Reloading data from file.");
                                update(&opt.file, &mut *buf).unwrap();
                                eprintln!("Info: Broadcasting JSON to web socket.");
                                broadcaster.send((*buf).clone()).unwrap();
                            } else {
                                //println!("File event on non-matching file.");
                            }
                        }
                        _ => {},
                    }
                },
                Err(e) => {
                    eprintln!("Warning: File watch error: {:?}", e);
                }
            }
        }
    });

    let addr = format!("http://{}/",my_addr);

    // Open web browser after starting server.
    thread::spawn(move || {
        thread::sleep(time::Duration::from_millis(100));
        eprintln!("Info: Opening web browser, address {}", &addr);
        webbrowser::open(&addr).unwrap();
    });

    eprintln!("Info: Starting web server.");
    http.listen(my_addr).unwrap();
}
