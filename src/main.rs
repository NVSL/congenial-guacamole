// #![deny(warnings)]
#![allow(dead_code)]
use std::path::Path;
use std::io::BufReader;
use std::fs::File;
use futures::{FutureExt, StreamExt};
use pmem::alloc::*;
use pmem::stm::Journal;
use pmem::str::String as PString;
use pmem::str::ToString;
use pmem::sync::Pack;
use pmem::sync::{Mutex as PMutex, Parc};
use pmem::*;
use serde_json::{json, Result as Rslt, Value};
use std::collections::BTreeMap;
use std::collections::HashMap;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};
use std::time::SystemTime;
use tokio::sync::{mpsc, RwLock};
use warp::ws::{Message, WebSocket};
use warp::Filter;
use md5::*;
use hex::*;
use serde::*;

mod hashmap;
mod history;
use hashmap::HashMap as PHashMap;
use history::*;

/// Our global unique user id counter.
static NEXT_USER_ID: AtomicUsize = AtomicUsize::new(1);

const fn color(r: u8, g: u8, b: u8) -> u32 {
    ((r as u32) << 16) | ((g as u32) << 8) | (b as u32)
}

static COLOR_PALLETE: [u32; 8] = [
    color(0, 0, 0),
    color(255, 0, 0),
    color(0, 255, 0),
    color(0, 0, 255),
    color(128, 128, 0),
    color(128, 0, 128),
    color(0, 128, 128),
    color(64, 64, 64),
];

#[derive(Deserialize, Debug)]
struct Server {
    host: String,
    port: u16
}

struct UserInfo {
    username: PString<P>,
    password: [u8; 16],
    color: u32,
    history: History,
}

impl RootObj<P> for UserInfo {
    fn init(j: &Journal<P>) -> Self {
        UserInfo {
            username: Default::default(),
            password: Default::default(),
            color: 0,
            history: RootObj::init(j),
        }
    }
}

/// Our state of currently connected users.
///
/// - Key is their id
/// - Value is a sender of `warp::ws::Message`
type Users = Arc<RwLock<HashMap<usize, mpsc::UnboundedSender<Result<Message, warp::Error>>>>>;

struct Database {
    data: PHashMap<[u8; 16], UserInfo>
}

impl RootObj<P> for Database {
    fn init(j: &Journal<P>) -> Self {
        Database {
            data: RootObj::init(j)
        }
    }
}

type Root = Parc<PMutex<Database, P>, P>;
type RootPack = Pack<PMutex<Database, P>, P>;

fn read_user_from_file<P: AsRef<Path>>(path: P) -> Result<Server, Box<dyn std::error::Error>> {
    // Open the file in read-only mode with buffer.
    let file = File::open(path)?;
    let reader = BufReader::new(file);

    // Read the JSON contents of the file as an instance of `User`.
    let u = serde_json::from_reader(reader)?;

    // Return the `User`.
    Ok(u)
}

#[tokio::main]
async fn main() {
    pretty_env_logger::init();

    // Keep track of all connected users, key is usize, value
    // is a websocket sender.
    let users = Users::default();
    // Turn our "state" into a new Filter...
    let users = warp::any().map(move || users.clone());

    let info = P::open::<Root>("users.pool", PO_CFNE | PO_2GB).unwrap();
    let pack = info.pack();
    let db = warp::any().map(move || pack.clone());
    // GET /chat -> websocket upgrade
    let chat = warp::path("chat")
        // The `ws()` filter will prepare Websocket handshake...
        .and(warp::ws())
        .and(users)
        .and(db)
        .map(|ws: warp::ws::Ws, users, db| {
            // This will call our function if the handshake succeeds.
            ws.on_upgrade(move |socket| user_connected(socket, users, db))
        });

    // GET / -> index html
    let index = warp::path::end().map(|| warp::reply::html(
        std::fs::read_to_string("wb.html")
        .expect("Something went wrong reading the file")));

    let routes = index.or(chat);

    let server = read_user_from_file("public_html/server.json")
        .expect("server.json does not exist");
    let arr: Vec<&str> = server.host.split(".").collect();
    let host: [u8; 4] = [
        arr[0].parse().unwrap(),
        arr[1].parse().unwrap(),
        arr[2].parse().unwrap(),
        arr[3].parse().unwrap()
        ];

    warp::serve(routes).run((host, server.port)).await;
}

async fn user_connected(ws: WebSocket, users: Users, root: RootPack) {
    // Use a counter to assign a new unique ID for this user.
    let my_id = NEXT_USER_ID.fetch_add(1, Ordering::Relaxed);
    let mut user_id: [u8; 16] = [0; 16];

    eprintln!("new chat user: {}", my_id);

    // Split the socket into a sender and receive of messages.
    let (user_ws_tx, mut user_ws_rx) = ws.split();

    // Use an unbounded channel to handle buffering and flushing of messages
    // to the websocket...
    let (tx, rx) = mpsc::unbounded_channel();
    tokio::task::spawn(rx.forward(user_ws_tx).map(|result| {
        if let Err(e) = result {
            eprintln!("websocket send error: {}", e);
        }
    }));

    // Save the sender in our list of connected users.
    users.write().await.insert(my_id, tx);

    // Return a `Future` that is basically a state machine managing
    // this specific user's connection.

    // Make an extra clone to give to our disconnection handler...
    let users2 = users.clone();

    // Every time the user sends a message, broadcast it to
    // all other users...
    while let Some(result) = user_ws_rx.next().await {
        let msg = match result {
            Ok(msg) => msg,
            Err(e) => {
                eprintln!("websocket error(uid={}): {}", my_id, e);
                break;
            }
        };
        user_id = user_message(my_id, user_id, msg, &users, &root).await;
    }

    // user_ws_rx stream will keep processing as long as the user stays
    // connected. Once they disconnect, then...
    user_disconnected(my_id, &users2).await;
}

async fn user_message(my_id: usize, user: [u8; 16], msg: Message, users: &Users, root: &RootPack) -> [u8; 16] {
    // Skip any non-Text messages...
    let msg = if let Ok(s) = msg.to_str() {
        s
    } else {
        return [0; 16];
    };

    if let Ok(v) = serde_json::from_str(msg) as Rslt<Value> {
        let cmd = &v["type"];
        if cmd == "login" || cmd == "new_user" {
            // Save the sender's name in our list of connected users, if not already set.
            let tx = &users.read().await[&my_id];
            let tx = AssertTxInSafe(tx);
            return P::transaction(|j| {
                if let Some(root) = root.unpack(j) {
                    let mut root = root.lock(j);
                    let name = v["username"].as_str().unwrap();
                    let pass = v["password"].as_str().unwrap();
                    let password = *compute(pass);

                    println!("received user: {}", name);
                    println!("received pass: {:?}", pass);
                    let user_id = *compute(name);
                    if let Some(u) = root.data.get_ref(user_id) {
                        if u.password == password {
                            println!("Logged in");
                            if let Err(disconnected) = tx.send(Ok(Message::text(format!(
                                "{{\"type\": \"login\", \"user\": \"{}\", \"name\": \"{}\", \"color\": \"{}\"}}",
                                user.encode_hex::<String>(), u.username, u.color
                            )))) {
                                eprintln!("User<#{}> is disconnected!", disconnected);
                            }
                            user_id
                        } else {
                            println!("Wrong password");
                            if let Err(disconnected) = tx.send(Ok(Message::text(format!(
                                "{{\"type\": \"wrong\"}}"
                            )))) {
                                eprintln!("User<#{}> is disconnected!", disconnected);
                            }
                            [0; 16]
                        }
                    } else if cmd == "new_user" {
                        root.data.put(
                            user_id,
                            UserInfo {
                                username: name.to_pstring(j),
                                password,
                                color: COLOR_PALLETE[(my_id - 1) % 8],
                                history: RootObj::init(j),
                            },
                            j,
                        );
                        user_id
                    } else {
                        println!("User doesn't exist");
                        if let Err(disconnected) = tx.send(Ok(Message::text(format!(
                            "{{\"type\": \"not_exists\"}}"
                        )))) {
                            eprintln!("User<#{}> is disconnected!", disconnected);
                        }
                        [0; 16]
                    }
                } else {
                    user
                }
            })
            .unwrap();
        } else if cmd == "set_user" {
            let user = *compute(v["data"].as_str().unwrap());
            let tx = &users.read().await[&my_id];
            let tx = AssertTxInSafe(tx);
            match P::transaction(|j| {
                if let Some(root) = root.unpack(j) {
                    if let Some(u) = root.lock(j).data.get_ref(user) {
                        u.color
                    } else { 0 }
                } else { 0 }
            }) {
                Err(e) => eprintln!("Error: {}", e),
                Ok(c) => {
                    let _ = tx.send(Ok(Message::text(
                        serde_json::to_string(&json!({
                            "type": "my_color",
                            "data": c
                        })).unwrap())));
                }
            }
            return user;
        } else if cmd == "set_color" {
            if let Err(e) = P::transaction(|j| {
                if let Some(root) = root.unpack(j) {
                    let s = &v["data"].as_str().unwrap()[1..];
                    let c = u32::from_str_radix(s, 16).unwrap();
                    if !root.lock(j).data.update_inplace_mut(&user, j, |w| w.color = c) {
                        eprintln!("User does not exist!");
                    }
                }
            }) {
                eprintln!("Error: {}", e);
            }
        } else if cmd == "undo" || cmd == "redo" || cmd == "redraw" || cmd == "clear" || cmd == "refresh" {
            let res = if cmd == "redraw" || cmd == "refresh" {
                Ok(true)
            } else {
                P::transaction(|j| {
                    let mut done = false;
                    if let Some(root) = root.unpack(j) {
                        if !root.lock(j).data.update_inplace(&user, |w| {
                            done = if cmd == "clear" {
                                w.history.clear()
                            } else if cmd == "undo" {
                                w.history.undo()
                            } else {
                                w.history.redo()
                            };
                        }) {
                            eprintln!("User does not exist!");
                        }
                    }
                    done
                })
            };
            if let Ok(done) = res {
                if done {
                    if let Ok(msg) = P::transaction(|j| {
                        let mut global_history = BTreeMap::<SystemTime, Value>::new();
                        if let Some(root) = root.unpack(j) {
                            root.lock(j).data.foreach(|_, root| {
                                let mut curr = root.history.head();
                                let last = root.history.last_timestamp(j);
                                while let Some(item) = curr.upgrade(j) {
                                    if item.timestamp() <= last {
                                        global_history.insert(item.timestamp(), item.as_json());
                                    } else {
                                        break;
                                    }
                                    curr = item.next();
                                }
                            });
                        }
                        let mut lst = vec![];
                        for (_, item) in global_history {
                            lst.push(item);
                        }
                        serde_json::to_string(&json!({
                            "type": "redraw",
                            "data": lst
                        }))
                        .unwrap()
                    }) {
                        let to_all = cmd != "refresh";
                        for (&id, tx) in users.read().await.iter() {
                            if to_all || id == my_id {
                                if let Err(disconnected) = tx.send(Ok(Message::text(msg.clone()))) {
                                    eprintln!("User<#{}> is disconnected!", disconnected);
                                }
                            }
                        }
                    }
                }
            } else if let Err(e) = res {
                eprintln!("Error: {}", e);
            }
        } else {
            if cmd == "draw" {
                if let Some(points) = v["data"].as_array() {
                    if !points.is_empty() {
                        let mut arr = Vec::<(i32, i32)>::with_capacity(points.len());
                        for p in points {
                            arr.push((
                                p["x"].as_i64().unwrap() as i32,
                                p["y"].as_i64().unwrap() as i32,
                            ));
                        }
                        if let Err(e) = P::transaction(|j| {
                            if let Some(root) = root.unpack(j) {
                                let root = root.lock(j);
                                let c = if let Some(r) = root.data.get_ref(user) {
                                    r.color
                                } else {
                                    0
                                };
                                if !root.data.update_inplace(&user, |w| w.history.add(j, &arr, c)) {
                                    eprintln!("User does not exist!");
                                }
                            }
                        }) {
                            eprintln!("Error: {}", e);
                        }
                    }
                }
            }

            // New message from this user, send it to everyone else (except same uid)...
            for (_, tx) in users.read().await.iter() {
                if let Err(disconnected) = tx.send(Ok(Message::text(msg.clone()))) {
                    // The tx is disconnected, our `user_disconnected` code
                    // should be happening in another task, nothing more to
                    // do here.
                    eprintln!("User<#{}> is disconnected!", disconnected);
                }
            }
        }
    } else {
        eprintln!("received data is not a json object!");
    }
    user
}

async fn user_disconnected(my_id: usize, users: &Users) {
    eprintln!("good bye user: {}", my_id);

    // Stream closed up, so remove from the user list
    users.write().await.remove(&my_id);
}
