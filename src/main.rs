// #![deny(warnings)]
#![allow(dead_code)]
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

struct UserInfo {
    name: PString<P>,
    pass: PString<P>,
    color: u32,
    history: History,
}

impl RootObj<P> for UserInfo {
    fn init(j: &Journal<P>) -> Self {
        UserInfo {
            name: Default::default(),
            pass: Default::default(),
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
type Database = Parc<PMutex<PHashMap<usize, UserInfo>, P>, P>;
type DatabasePack = Pack<PMutex<PHashMap<usize, UserInfo>, P>, P>;

#[tokio::main]
async fn main() {
    pretty_env_logger::init();

    // Keep track of all connected users, key is usize, value
    // is a websocket sender.
    let users = Users::default();
    // Turn our "state" into a new Filter...
    let users = warp::any().map(move || users.clone());

    let info = P::open::<Database>("users.pool", PO_CFNE | PO_2GB).unwrap();
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
    let index = warp::path::end().map(|| warp::reply::html(INDEX_HTML));

    let routes = index.or(chat);

    warp::serve(routes).run(([10, 1, 1, 62], 3030)).await;
}

async fn user_connected(ws: WebSocket, users: Users, data: DatabasePack) {
    // Use a counter to assign a new unique ID for this user.
    let my_id = NEXT_USER_ID.fetch_add(1, Ordering::Relaxed);

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

    // Save the sender's name in our list of connected users, if not already set.
    P::transaction(|j| {
        if let Some(data) = data.unpack(j) {
            data.lock(j).or_insert(
                &my_id,
                UserInfo {
                    name: format!("User<#{}>", my_id).to_pstring(j),
                    pass: "0000".to_pstring(j),
                    color: COLOR_PALLETE[(my_id - 1) % 8],
                    history: RootObj::init(j),
                },
                j,
            );
        }
    })
    .unwrap();

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
        user_message(my_id, msg, &users, &data).await;
    }

    // user_ws_rx stream will keep processing as long as the user stays
    // connected. Once they disconnect, then...
    user_disconnected(my_id, &users2).await;
}

async fn user_message(my_id: usize, msg: Message, users: &Users, data: &DatabasePack) {
    // Skip any non-Text messages...
    let msg = if let Ok(s) = msg.to_str() {
        s
    } else {
        return;
    };

    if let Ok(v) = serde_json::from_str(msg) as Rslt<Value> {
        let cmd = &v["type"];
        if cmd == "get_id" {
            let tx = &users.read().await[&my_id];
            if let Err(disconnected) = tx.send(Ok(Message::text(format!(
                "{{ \"user\": \"{}\", \"type\": \"my_id\" }}",
                my_id
            )))) {
                eprintln!("User<#{}> is disconnected!", disconnected);
            }
        } else if cmd == "get_name" {
            let tx = &users.read().await[&my_id];
            let tx = AssertTxInSafe(tx);
            if let Err(e) = P::transaction(|j| {
                if let Some(data) = data.unpack(j) {
                    if let Some(r) = data.lock(j).get_ref(my_id) {
                        if let Err(disconnected) = tx.send(Ok(Message::text(format!(
                            "{{ \"user\": \"{}\",
                        \"type\": \"my_name\", \"data\": \"{}\" }}",
                            my_id, r.name
                        )))) {
                            eprintln!("User<#{}> is disconnected!", disconnected);
                        }
                    }
                }
            }) {
                eprintln!("Error: {}", e);
            }
        } else if cmd == "get_color" {
            let tx = &users.read().await[&my_id];
            let tx = AssertTxInSafe(tx);
            if let Err(e) = P::transaction(|j| {
                if let Some(data) = data.unpack(j) {
                    if let Some(r) = data.lock(j).get_ref(my_id) {
                        if let Err(disconnected) = tx.send(Ok(Message::text(format!(
                            "{{ \"user\": \"{}\",
                        \"type\": \"my_color\", \"data\": {} }}",
                            my_id, r.color
                        )))) {
                            eprintln!("User<#{}> is disconnected!", disconnected);
                        }
                    }
                }
            }) {
                eprintln!("Error: {}", e);
            }
        } else if cmd == "set_name" {
            if let Err(e) = P::transaction(|j| {
                if let Some(data) = data.unpack(j) {
                    if !data.lock(j).update_inplace_mut(&my_id, j, |w| {
                        w.name = v["data"].to_pstring(j);
                    }) {
                        eprintln!("User<#{}> does not exist!", my_id);
                    }
                }
            }) {
                eprintln!("Error: {}", e);
            }
        } else if cmd == "set_color" {
            if let Err(e) = P::transaction(|j| {
                if let Some(data) = data.unpack(j) {
                    let s = &v["data"].as_str().unwrap()[1..];
                    let c = u32::from_str_radix(s, 16).unwrap();
                    if !data.lock(j).update_inplace_mut(&my_id, j, |w| w.color = c) {
                        eprintln!("User<#{}> does not exist!", my_id);
                    }
                }
            }) {
                eprintln!("Error: {}", e);
            }
        } else if cmd == "undo" || cmd == "redo" || cmd == "redraw" {
            let res = if cmd == "redraw" {
                Ok(true)
            } else {
                    P::transaction(|j| {
                    let mut done = false;
                    if let Some(data) = data.unpack(j) {
                        if !data.lock(j).update_inplace(&my_id, |w| {
                            done = if cmd == "undo" {
                                w.history.undo()
                            } else {
                                w.history.redo()
                            };
                        }) {
                            eprintln!("User<#{}> does not exist!", my_id);
                        }
                    }
                    done
                })
            };
            if let Ok(done) = res {
                if done {
                    if let Ok(msg) = P::transaction(|j| {
                        let mut global_history = BTreeMap::<SystemTime, Value>::new();
                        if let Some(data) = data.unpack(j) {
                            data.lock(j).foreach(|_, data| {
                                let mut curr = data.history.head();
                                let last = data.history.last_timestamp(j);
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
                        for (_, tx) in users.read().await.iter() {
                            if let Err(disconnected) = tx.send(Ok(Message::text(msg.clone()))) {
                                eprintln!("User<#{}> is disconnected!", disconnected);
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
                            if let Some(data) = data.unpack(j) {
                                let info = data.lock(j);
                                let c = if let Some(r) = info.get_ref(my_id) {
                                    r.color
                                } else {
                                    0
                                };
                                if !info.update_inplace(&my_id, |w| w.history.add(j, &arr, c)) {
                                    eprintln!("User<#{}> does not exist!", my_id);
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
}

async fn user_disconnected(my_id: usize, users: &Users) {
    eprintln!("good bye user: {}", my_id);

    // Stream closed up, so remove from the user list
    users.write().await.remove(&my_id);
}

static INDEX_HTML: &str = r#"<!DOCTYPE html>
<html lang="en">
    <head>
        <title>Whiteboard &amp; Chat</title>
        <meta name="viewport" content="width=device-width, initial-scale=1">
        <link rel="stylesheet" href="https://fonts.googleapis.com/icon?family=Material+Icons">
    </head>
    <body>
        <div class="container" style="display: flex; height: 100px;">
            <div style="width: 80%;">
                <input type="color" id="cbox" name="cbox" value="\#000000">
                <input type="button" id="undo" name="undo" class="material-icons" value="undo">
                <input type="button" id="redo" name="redo" class="material-icons" value="redo">
                <br>
                <canvas id="drawCanvas" width="800" height="600"
                style="border:1px solid #000000;"></canvas>
            </div>
            <div style="flex-grow: 1;">
                <h1>Chat</h1>
                <div id="chat">
                    <p><em>Connecting...</em></p>
                </div>
                <input type="text" id="text" />
                <button type="button" id="send">Send</button>
            </div>
        </div>
        <script type="text/javascript">
            const chat = document.getElementById('chat');
            const text = document.getElementById('text');
            const uri = 'ws://' + location.host + '/chat';
            var connected = false;

            function say(user, content) {
                const line = document.createElement('p');
                line.innerText = `${user}: ${content}`;
                chat.appendChild(line);
            }

            var ws;
            var name;
            var color = '#000000';

            function message(data) {
                var msg = JSON.parse(data);
                if (msg.type == 'text') {
                    say(msg.name, msg.data);
                } else if (msg.type == 'my_name') {
                    name = msg.data;
                } else if (msg.type == 'my_color') {
                    color = msg.data;
                    cbox.value = "\#" + color.toString(16).padStart(6, "0");
                } else if (msg.type == 'draw_tmp') {
                    ctx.lineWidth = '0.5';
                    drawOnCanvas(msg.color, msg.data, false);
                } else if (msg.type == 'draw') {
                    ctx.lineWidth = '3';
                    drawOnCanvas(msg.color, msg.data, true);
                } else if (msg.type == 'redraw') {
                    ctx.clearRect(0, 0, canvas.width, canvas.height);
                    ctx.lineWidth = '3';
                    msg.data.forEach(function (item) {
                        drawOnCanvas(item.color, item.data, true);
                    });
                }  
            }

            function connect() {
                ws = new WebSocket(uri);
                ws.onopen = function() {
                    connected = true;
                    chat.innerHTML = '<p><em>Connected!</em></p>';
                    ws.send('{ "type": "get_name" }');
                    ws.send('{ "type": "get_color" }');
                    ws.send('{ "type": "redraw" }');
                };
    
                ws.onmessage = function(msg) {
                    message(msg.data);
                };
    
                ws.onclose = function() {
                    connected = false;
                    chat.getElementsByTagName('em')[0].innerText = 'Disconnected!';
                    setTimeout(connect(), 1000);
                };
            }

            connect();

            send.onclick = function() {
                const msg = {
                    name: name,
                    type: 'text',
                    data: text.value
                };
                ws.send(JSON.stringify(msg));
                say('You', text.value);
                text.value = '';
            };

            var canvas = document.getElementById('drawCanvas');
            var ctx = canvas.getContext('2d');
            ctx.lineWidth = '3';
            canvas.addEventListener('mousedown', startDraw, false);
            canvas.addEventListener('mousemove', draw, false);
            canvas.addEventListener('mouseup', endDraw, false);
            cbox.addEventListener('change', setcolor, false);
            undo.addEventListener('click', function(e) {
                ws.send(JSON.stringify({
                    type: "undo",
                }));
            }, false);
            redo.addEventListener('click', function(e) {
                ws.send(JSON.stringify({
                    type: "redo",
                }));
            }, false);

            // create a flag
            var isActive = false;

            // array to collect coordinates
            var plots = [];
            var plots_tmp = [];

            function setcolor(e) {
                ws.send(JSON.stringify({
                    type: "set_color",
                    data: cbox.value,
                }));
                ws.send('{ "type": "get_color" }');
            }

            function draw(e) {
                if(!isActive || !connected) return;
                // cross-browser canvas coordinates
                var x = e.offsetX || e.layerX - canvas.offsetLeft;
                var y = e.offsetY || e.layerY - canvas.offsetTop;

                plots.push({x: x, y: y});
                plots_tmp.push({x: x, y: y});
                //drawOnCanvas(color, plots);
                ws.send(JSON.stringify({
                    type: "draw_tmp",
                    color: color,
                    data: plots_tmp
                }));
                while (plots_tmp.length > 2) {
                    plots_tmp.shift();
                }
            }

            function drawOnCanvas(color, plots) {
                if (plots.length == 0) return;
                ctx.beginPath();
                ctx.moveTo(plots[0].x, plots[0].y);

                for(var i=1; i<plots.length; i++) {
                    ctx.lineTo(plots[i].x, plots[i].y);
                }
                ctx.strokeStyle = "\#" + color.toString(16).padStart(6, "0");
                ctx.stroke();
            }

            function startDraw(e) {
                isActive = true;
                plots = [];
                plots_tmp = [];
                ctx.lineWidth = '0.5';
            }

            function endDraw(e) {
                isActive = false;
                ws.send(JSON.stringify({
                    type: "draw",
                    color: color,
                    data: plots
                }));
                plots = [];
                plots_tmp = [];
            }
        </script>
    </body>
</html>
"#;
